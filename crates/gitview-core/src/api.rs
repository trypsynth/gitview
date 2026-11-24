use std::{fmt::Write, sync::OnceLock, thread, time::Duration};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ureq::RequestBuilder;

use crate::{
	Error,
	models::{
		Branch, CloseReason, Comment, Commit, CommitDetails, Email, Issue, IssueState, Notification, Profile,
		ProfileUpdate, Repository, RepositoryKind, RepositoryOrder, SearchOrder, SocialAccount, StarredOrder,
		SubjectDetails, User, UserProfile,
	},
	page::{At, Connection, Cursor, Kind, Page, link},
};

const API_URL: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
// Asks GitHub to render the markdown for us, so bodies arrive as `body_html` with the links,
// mentions and task lists already in place.
const HTML_ACCEPT: &str = "application/vnd.github.full+json";
const USER_AGENT: &str = concat!("gitview/", env!("CARGO_PKG_VERSION"));
/// How many items a page of a list read [newest first](Client::newest_first), or of a GraphQL
/// connection, holds: the most GitHub allows.
const PAGE_SIZE: usize = 100;
/// How long to wait before asking again after GitHub has a server error.
const SERVER_ERROR_WAIT: Duration = Duration::from_secs(1);
/// How many pages of a repository's branches are listed, a hundred to a page. Past that, the rest
/// are left out rather than waited for.
const BRANCH_PAGES: usize = 10;

#[derive(Serialize)]
struct NewComment<'a> {
	body: &'a str,
}

#[derive(Serialize)]
struct StateChange {
	state: &'static str,
	#[serde(skip_serializing_if = "Option::is_none")]
	state_reason: Option<&'static str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	duplicate_issue_id: Option<u64>,
}

#[derive(Serialize)]
struct Subscription {
	subscribed: bool,
}

#[derive(Serialize)]
struct SocialAccountUrls<'a> {
	account_urls: &'a [String],
}

#[derive(Deserialize)]
struct SearchResults<T> {
	items: Vec<T>,
}

/// `text` made safe to put in a URL's query, such as a branch named `fix/a+b`: every byte but
/// letters, digits and `-._~` is percent-encoded.
fn query_value(text: &str) -> String {
	let mut encoded = String::with_capacity(text.len());
	for byte in text.bytes() {
		if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
			encoded.push(char::from(byte));
		} else {
			// Writing to a String can't fail.
			let _ = write!(encoded, "%{byte:02X}");
		}
	}
	encoded
}

/// The organization a 403's body names, when it refused because the organization limits what
/// third-party apps can see.
fn restricting_organization(body: &str) -> Option<String> {
	#[derive(Deserialize)]
	struct Refusal {
		message: String,
	}
	let message = serde_json::from_str::<Refusal>(body).ok()?.message;
	if !message.contains("OAuth App access restrictions") {
		return None;
	}
	// "... the `accessware` organization has enabled OAuth App access restrictions, ..."
	let (_, rest) = message.split_once('`')?;
	let (organization, _) = rest.split_once('`')?;
	Some(organization.to_owned())
}

#[derive(Serialize)]
struct GraphRequest<'a> {
	query: &'a str,
	variables: ConnectionVariables<'a>,
}

#[derive(Serialize)]
struct ConnectionVariables<'a> {
	login: &'a str,
	after: Option<&'a str>,
}

#[derive(Deserialize)]
struct GraphResponse<T> {
	data: Option<UserData<T>>,
	#[serde(default)]
	errors: Vec<GraphError>,
}

#[derive(Deserialize)]
struct GraphError {
	message: String,
}

#[derive(Deserialize)]
struct UserData<T> {
	user: Option<UserList<T>>,
}

/// The one connection asked for, under the alias `list`.
#[derive(Deserialize)]
struct UserList<T> {
	list: GraphList<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphList<T> {
	nodes: Vec<T>,
	page_info: PageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
	has_next_page: bool,
	end_cursor: Option<String>,
}

pub struct Client {
	agent: ureq::Agent,
	token: String,
	/// The signed-in user's login, once asked for.
	login: OnceLock<String>,
}

impl Client {
	#[must_use]
	pub fn new(token: String) -> Self {
		let agent = ureq::Agent::config_builder().user_agent(USER_AGENT).build().into();
		Self { agent, token, login: OnceLock::new() }
	}

	pub fn current_user(&self) -> Result<User, Error> {
		self.get("/user")
	}

	/// The signed-in user's login, asked for once and then remembered.
	pub fn login(&self) -> Result<String, Error> {
		if let Some(login) = self.login.get() {
			return Ok(login.clone());
		}
		let login = self.current_user()?.login;
		Ok(self.login.get_or_init(|| login).clone())
	}

	/// Up to 10 people and organizations whose login matches `query`, best match first.
	pub fn search_users(&self, query: &str) -> Result<Vec<User>, Error> {
		// A login is letters, digits and hyphens, so anything else can only fail to match, and
		// what's left needs no escaping.
		let query: String = query.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-').collect();
		if query.is_empty() {
			return Ok(Vec::new());
		}
		Ok(self.fetch_page(&format!("/search/users?q={query}+in:login&per_page=10"), Kind::Search)?.items)
	}

	/// The signed-in user's profile. Fails with [`Error::NeedsProfileAccess`] when the sign-in
	/// predates Gitview asking for the `user` scope, since saving would fail anyway.
	pub fn profile(&self) -> Result<Profile, Error> {
		let mut response = self
			.authorize(self.agent.get(format!("{API_URL}/user")))
			.header("Accept", "application/vnd.github+json")
			.call()?;
		let has_user_scope = response
			.headers()
			.get("X-OAuth-Scopes")
			.and_then(|scopes| scopes.to_str().ok())
			.is_some_and(|scopes| scopes.split(',').any(|scope| scope.trim() == "user"));
		if !has_user_scope {
			return Err(Error::NeedsProfileAccess);
		}
		Ok(response.body_mut().read_json()?)
	}

	pub fn update_profile(&self, update: &ProfileUpdate) -> Result<Profile, Error> {
		Ok(self.authorize(self.agent.patch(format!("{API_URL}/user"))).send_json(update)?.body_mut().read_json()?)
	}

	/// Every address on the account, including the ones that are not verified.
	pub fn emails(&self) -> Result<Vec<Email>, Error> {
		self.get("/user/emails?per_page=100")
	}

	pub fn social_accounts(&self) -> Result<Vec<SocialAccount>, Error> {
		self.get("/user/social_accounts?per_page=100")
	}

	pub fn add_social_accounts(&self, urls: &[String]) -> Result<(), Error> {
		self.authorize(self.agent.post(format!("{API_URL}/user/social_accounts")))
			.send_json(SocialAccountUrls { account_urls: urls })?;
		Ok(())
	}

	pub fn remove_social_accounts(&self, urls: &[String]) -> Result<(), Error> {
		self.authorize(self.agent.delete(format!("{API_URL}/user/social_accounts")))
			.force_send_body()
			.send_json(SocialAccountUrls { account_urls: urls })?;
		Ok(())
	}

	/// Unread notifications, or every notification when `all` is set.
	pub fn notifications(&self, all: bool) -> Result<Page<Notification>, Error> {
		self.fetch_page(&format!("/notifications?per_page=50&all={all}"), Kind::Plain)
	}

	/// Whatever a notification's subject URL points at, for the kinds that are not issues.
	pub fn subject(&self, url: &str) -> Result<SubjectDetails, Error> {
		// The URL comes from the notification payload, so check where it points before the
		// token travels with it.
		let path = url.strip_prefix(API_URL).ok_or(Error::NotGitHub)?;
		self.get_rendered(path)
	}

	pub fn mark_notification_read(&self, id: &str) -> Result<(), Error> {
		self.authorize(self.agent.patch(format!("{API_URL}/notifications/threads/{id}"))).send_empty()?;
		Ok(())
	}

	/// Moves a notification out of the inbox, as the Done button on GitHub does.
	pub fn mark_notification_done(&self, id: &str) -> Result<(), Error> {
		self.authorize(self.agent.delete(format!("{API_URL}/notifications/threads/{id}"))).call()?;
		Ok(())
	}

	/// Stops notifications for a thread until the user comments or is mentioned in it.
	pub fn unsubscribe(&self, id: &str) -> Result<(), Error> {
		self.authorize(self.agent.delete(format!("{API_URL}/notifications/threads/{id}/subscription"))).call()?;
		Ok(())
	}

	/// Watches `repo` (`owner/name`) for all activity, or stops watching it when `watching` is
	/// false.
	pub fn set_watching(&self, repo: &str, watching: bool) -> Result<(), Error> {
		let url = format!("{API_URL}/repos/{repo}/subscription");
		if watching {
			self.authorize(self.agent.put(url)).send_json(Subscription { subscribed: true })?;
		} else {
			self.authorize(self.agent.delete(url)).call()?;
		}
		Ok(())
	}

	/// Stars `repo` (`owner/name`), or unstars it when `starred` is false.
	pub fn set_starred(&self, repo: &str, starred: bool) -> Result<(), Error> {
		let url = format!("{API_URL}/user/starred/{repo}");
		if starred {
			self.authorize(self.agent.put(url)).send_empty()?;
		} else {
			self.authorize(self.agent.delete(url)).call()?;
		}
		Ok(())
	}

	/// Repository `repo` (`owner/name`), with its name as GitHub spells it.
	pub fn repository(&self, repo: &str) -> Result<Repository, Error> {
		self.get(&format!("/repos/{repo}"))
	}

	/// The signed-in user's repositories of `kind`, in `order`.
	pub fn repositories(&self, kind: RepositoryKind, order: RepositoryOrder) -> Result<Page<Repository>, Error> {
		self.fetch_page(&format!("/user/repos?{}&{}&per_page=100", kind.parameter(), order.parameters()), Kind::Plain)
	}

	/// Repositories the signed-in user has starred, in `order`.
	pub fn starred(&self, order: StarredOrder) -> Result<Page<Repository>, Error> {
		self.fetch_page(&format!("/user/starred?{}&per_page=100", order.parameters()), Kind::Plain)
	}

	/// Repositories the signed-in user watches, which GitHub calls subscriptions, most recently
	/// watched first.
	pub fn watched(&self) -> Result<Page<Repository>, Error> {
		self.newest_first(&format!("/user/subscriptions?per_page={PAGE_SIZE}"))
	}

	/// Anyone's public profile, by their login.
	pub fn user(&self, login: &str) -> Result<UserProfile, Error> {
		self.get(&format!("/users/{login}"))
	}

	/// `login`'s public repositories, most recently updated first.
	pub fn user_repositories(&self, login: &str) -> Result<Page<Repository>, Error> {
		self.fetch_page(&format!("/users/{login}/repos?sort=updated&direction=desc&per_page=100"), Kind::Plain)
	}

	/// What `login` has starred, most recently starred first.
	pub fn user_starred(&self, login: &str) -> Result<Page<Repository>, Error> {
		self.fetch_page(&format!("/users/{login}/starred?sort=created&direction=desc&per_page=100"), Kind::Plain)
	}

	/// Who follows the user `login`, most recent first.
	pub fn followers(&self, login: &str) -> Result<Page<User>, Error> {
		self.connection_page(Connection::Followers, login, None)
	}

	/// Who follows the organization `login`, newest accounts first. GitHub can't say when anyone
	/// followed an organization, and lists them by account age.
	pub fn organization_followers(&self, login: &str) -> Result<Page<User>, Error> {
		self.newest_first(&format!("/users/{login}/followers?per_page={PAGE_SIZE}"))
	}

	/// Who `login` follows: the people most recent first, then the organizations, which GitHub
	/// can't say when they were followed.
	pub fn following(&self, login: &str) -> Result<Page<User>, Error> {
		let mut page = self.connection_page(Connection::Following, login, None)?;
		// Following only organizations leaves the first page of people empty.
		if page.items.is_empty() {
			self.load_pages(&mut page, 1)?;
		}
		Ok(page)
	}

	/// The newest commits on `branch` of `repo` (`owner/name`), or on its default branch when
	/// that's `None`. A repository with no commits yet has none, which GitHub answers with a
	/// conflict rather than an empty list.
	pub fn commits(&self, repo: &str, branch: Option<&str>) -> Result<Page<Commit>, Error> {
		let on = branch.map(|branch| format!("&sha={}", query_value(branch))).unwrap_or_default();
		match self.fetch_page(&format!("/repos/{repo}/commits?per_page=100{on}"), Kind::Plain) {
			Err(Error::Http(ureq::Error::StatusCode(409))) => Ok(Page::empty()),
			result => result,
		}
	}

	/// The branches of `repo` (`owner/name`), by name, as far as the first [`BRANCH_PAGES`] pages
	/// go.
	pub fn branches(&self, repo: &str) -> Result<Vec<Branch>, Error> {
		let mut page = self.fetch_page(&format!("/repos/{repo}/branches?per_page=100"), Kind::Plain)?;
		self.load_pages(&mut page, BRANCH_PAGES - 1)?;
		Ok(page.items)
	}

	/// Commit `sha` in `repo` (`owner/name`), with the files it changed and how.
	pub fn commit(&self, repo: &str, sha: &str) -> Result<CommitDetails, Error> {
		self.get(&format!("/repos/{repo}/commits/{sha}"))
	}

	/// Issues and pull requests in `repo` (`owner/name`), newest first.
	pub fn issues(&self, repo: &str, state: IssueState) -> Result<Page<Issue>, Error> {
		let path =
			format!("/repos/{repo}/issues?state={}&sort=created&direction=desc&per_page=100", state.as_parameter());
		self.fetch_page(&path, Kind::Rendered)
	}

	/// Issues and pull requests assigned to the signed-in user, across every repository.
	pub fn assigned(&self, state: IssueState, order: SearchOrder) -> Result<Page<Issue>, Error> {
		self.search("assignee:@me", state, order)
	}

	/// Pull requests waiting for the signed-in user's review.
	pub fn review_requests(&self, state: IssueState, order: SearchOrder) -> Result<Page<Issue>, Error> {
		self.search("is:pr review-requested:@me", state, order)
	}

	pub fn issue(&self, repo: &str, number: u64) -> Result<Issue, Error> {
		self.get_rendered(&format!("/repos/{repo}/issues/{number}"))
	}

	/// Every comment on the issue, however many pages they take: a thread reads as a whole.
	pub fn comments(&self, repo: &str, number: u64) -> Result<Vec<Comment>, Error> {
		let mut page =
			self.fetch_page(&format!("/repos/{repo}/issues/{number}/comments?per_page=100"), Kind::Rendered)?;
		self.load_pages(&mut page, usize::MAX)?;
		Ok(page.items)
	}

	/// The page `cursor` points at.
	pub fn next_page<T: DeserializeOwned>(&self, cursor: &Cursor<T>) -> Result<Page<T>, Error> {
		self.fetch_cursor(cursor)
	}

	/// Adds up to `pages` more pages to `page`, stopping at the list's end, so a list can be
	/// fetched again as far as it had been loaded.
	pub fn load_pages<T: DeserializeOwned>(&self, page: &mut Page<T>, pages: usize) -> Result<(), Error> {
		for _ in 0..pages {
			let Some(cursor) = page.next.take() else {
				break;
			};
			let next = self.next_page(&cursor)?;
			page.items.extend(next.items);
			page.next = next.next;
		}
		Ok(())
	}

	pub fn close_issue(&self, repo: &str, number: u64, reason: CloseReason) -> Result<(), Error> {
		let (state_reason, duplicate_issue_id) = match reason {
			CloseReason::Completed => ("completed", None),
			CloseReason::NotPlanned => ("not_planned", None),
			CloseReason::Duplicate(id) => ("duplicate", Some(id)),
		};
		self.change_state(
			repo,
			number,
			StateChange { state: "closed", state_reason: Some(state_reason), duplicate_issue_id },
		)
	}

	/// Closes a pull request without merging it. Pull requests take no close reason.
	pub fn close_pull_request(&self, repo: &str, number: u64) -> Result<(), Error> {
		self.change_state(repo, number, StateChange { state: "closed", state_reason: None, duplicate_issue_id: None })
	}

	/// Reopens an issue or pull request.
	pub fn reopen(&self, repo: &str, number: u64) -> Result<(), Error> {
		self.change_state(repo, number, StateChange { state: "open", state_reason: None, duplicate_issue_id: None })
	}

	// Pull requests are issues too, so the issue endpoint opens and closes both.
	fn change_state(&self, repo: &str, number: u64, change: StateChange) -> Result<(), Error> {
		self.authorize(self.agent.patch(format!("{API_URL}/repos/{repo}/issues/{number}"))).send_json(change)?;
		Ok(())
	}

	pub fn add_comment(&self, repo: &str, number: u64, body: &str) -> Result<Comment, Error> {
		Ok(self
			.authorize(self.agent.post(format!("{API_URL}/repos/{repo}/issues/{number}/comments")))
			.header("Accept", HTML_ACCEPT)
			.send_json(NewComment { body })?
			.body_mut()
			.read_json()?)
	}

	// Spaces are the only character the qualifiers here need escaping for; `:`, `@` and `-`
	// all travel through a query string as they are.
	fn search(&self, query: &str, state: IssueState, order: SearchOrder) -> Result<Page<Issue>, Error> {
		let query = format!("{query} {}", state.as_qualifier());
		self.fetch_page(
			&format!(
				"/search/issues?advanced_search=true&{}&per_page=100&q={}",
				order.parameters(),
				query.trim().replace(' ', "+")
			),
			Kind::Search,
		)
	}

	/// A list at `path`, which GitHub can only give oldest first, newest first: it is read from its
	/// last page back, each page back to front. The first page says where the last one is.
	fn newest_first<T: DeserializeOwned>(&self, path: &str) -> Result<Page<T>, Error> {
		let (mut first, last) = self.fetch_page_links(path, Kind::Plain, false)?;
		if let Some(last) = last {
			let mut page = self.fetch_cursor(&Cursor::new(last, Kind::Plain, true))?;
			// The last page can hold only a few, which would open the list nearly empty.
			if page.items.len() < PAGE_SIZE / 2 {
				self.load_pages(&mut page, 1)?;
			}
			return Ok(page);
		}
		// One page holds them all.
		first.items.reverse();
		Ok(first)
	}

	/// The first page of a list at `path`, read front to back.
	fn fetch_page<T: DeserializeOwned>(&self, path: &str, kind: Kind) -> Result<Page<T>, Error> {
		Ok(self.fetch_page_links(path, kind, false)?.0)
	}

	fn fetch_cursor<T: DeserializeOwned>(&self, cursor: &Cursor<T>) -> Result<Page<T>, Error> {
		match &cursor.at {
			At::Rest { path, kind, backwards } => Ok(self.fetch_page_links(path, *kind, *backwards)?.0),
			At::Graph { connection, login, after } => self.connection_page(*connection, login, Some(after)),
			At::FollowedOrganizations { login } => self.followed_organizations(login),
		}
	}

	/// The organizations `login` follows, picked out of everyone they follow.
	fn followed_organizations<T: DeserializeOwned>(&self, login: &str) -> Result<Page<T>, Error> {
		let mut everyone: Page<serde_json::Value> =
			self.fetch_page(&format!("/users/{login}/following?per_page={PAGE_SIZE}"), Kind::Plain)?;
		self.load_pages(&mut everyone, usize::MAX)?;
		let items = everyone
			.items
			.into_iter()
			.filter(|account| account.get("type").and_then(serde_json::Value::as_str) == Some("Organization"))
			.map(serde_json::from_value)
			.collect::<Result<_, _>>()?;
		Ok(Page { items, next: None })
	}

	/// A page of `login`'s `connection`, from GitHub's GraphQL API, after the cursor `after`, or
	/// its first page without one. Its nodes come as `T`, with each user's login and name.
	fn connection_page<T: DeserializeOwned>(
		&self,
		connection: Connection,
		login: &str,
		after: Option<&str>,
	) -> Result<Page<T>, Error> {
		let field = connection.field();
		tracing::debug!(login, field, "fetching from GraphQL");
		let query = format!(
			"query($login: String!, $after: String) {{ user(login: $login) {{ list: {field}(first: {PAGE_SIZE}, \
			 after: $after) {{ nodes {{ login name type: __typename }} pageInfo {{ hasNextPage endCursor }} }} }} }}"
		);
		let response: GraphResponse<T> = self
			.authorize(self.agent.post(format!("{API_URL}/graphql")))
			.send_json(GraphRequest { query: &query, variables: ConnectionVariables { login, after } })?
			.body_mut()
			.read_json()?;
		let Some(list) = response.data.and_then(|data| data.user).map(|user| user.list) else {
			let message = response.errors.into_iter().next().map(|error| error.message);
			return Err(Error::Graph(message.unwrap_or_else(|| format!("GitHub has no user {login}"))));
		};
		let next = match list.page_info.end_cursor.filter(|_| list.page_info.has_next_page) {
			Some(after) => Some(Cursor::graph(connection, login.to_owned(), after)),
			// After the people they follow come the organizations.
			None if connection == Connection::Following => Some(Cursor::followed_organizations(login.to_owned())),
			None => None,
		};
		Ok(Page { items: list.nodes, next })
	}

	/// One page of a list at `path`, with where the next page is, as GitHub's `Link` header says,
	/// and where its last page is. Read `backwards`, the page comes back to front, and the next
	/// page is the one before it.
	fn fetch_page_links<T: DeserializeOwned>(
		&self,
		path: &str,
		kind: Kind,
		backwards: bool,
	) -> Result<(Page<T>, Option<String>), Error> {
		// The path alone: the token travels in a header, never in it.
		tracing::debug!(path, "fetching");
		let accept = if kind == Kind::Rendered { HTML_ACCEPT } else { "application/vnd.github+json" };
		// Statuses are read here rather than as errors, since a refusal's body says why.
		let request = || {
			self.authorize(self.agent.get(format!("{API_URL}{path}")))
				.header("Accept", accept)
				.config()
				.http_status_as_error(false)
				.build()
				.call()
		};
		let mut response = request()?;
		// GitHub can time out on a page deep in a very long list, and asks clients to try again
		// after a server error.
		if response.status().is_server_error() {
			tracing::debug!(path, "server error, trying once more");
			thread::sleep(SERVER_ERROR_WAIT);
			response = request()?;
		}
		let status = response.status().as_u16();
		if status == 403
			&& let Some(organization) =
				response.body_mut().read_to_string().ok().as_deref().and_then(restricting_organization)
		{
			return Err(Error::Restricted(organization));
		}
		if !response.status().is_success() {
			return Err(ureq::Error::StatusCode(status).into());
		}
		let links = response.headers().get("Link").and_then(|header| header.to_str().ok()).unwrap_or_default();
		// A link anywhere but the API is never followed, so the token never goes anywhere else.
		let path_of = |rel| link(links, rel).and_then(|url| url.strip_prefix(API_URL)).map(str::to_owned);
		let next = path_of(if backwards { "prev" } else { "next" }).map(|path| Cursor::new(path, kind, backwards));
		let last = path_of("last");
		let mut items: Vec<T> = if kind == Kind::Search {
			response.body_mut().read_json::<SearchResults<T>>()?.items
		} else {
			response.body_mut().read_json()?
		};
		if backwards {
			items.reverse();
		}
		Ok((Page { items, next }, last))
	}

	fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
		self.fetch(path, "application/vnd.github+json")
	}

	/// A GET that asks for bodies as HTML as well as markdown.
	fn get_rendered<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
		self.fetch(path, HTML_ACCEPT)
	}

	fn fetch<T: DeserializeOwned>(&self, path: &str, accept: &str) -> Result<T, Error> {
		// The path alone: the token travels in a header, never in it.
		tracing::debug!(path, "fetching");
		Ok(self
			.authorize(self.agent.get(format!("{API_URL}{path}")))
			.header("Accept", accept)
			.call()?
			.body_mut()
			.read_json()?)
	}

	fn authorize<B>(&self, request: RequestBuilder<B>) -> RequestBuilder<B> {
		request.header("Authorization", format!("Bearer {}", self.token)).header("X-GitHub-Api-Version", API_VERSION)
	}
}

#[cfg(test)]
mod tests {
	use super::{query_value, restricting_organization};

	#[test]
	fn a_query_value_keeps_only_unreserved_characters() {
		assert_eq!(query_value("main"), "main");
		assert_eq!(query_value("fix/a+b c#1"), "fix%2Fa%2Bb%20c%231");
		assert_eq!(query_value("ünï"), "%C3%BCn%C3%AF");
	}

	#[test]
	fn an_organizations_app_restrictions_are_told_from_other_refusals() {
		let restricted = r#"{"message":"Although you appear to have the correct authorization credentials, the `accessware` organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited.","status":"403"}"#;
		assert_eq!(restricting_organization(restricted).as_deref(), Some("accessware"));
		assert_eq!(restricting_organization(r#"{"message":"API rate limit exceeded"}"#), None);
		assert_eq!(restricting_organization("not json"), None);
	}
}
