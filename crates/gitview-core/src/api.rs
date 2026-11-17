use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ureq::RequestBuilder;

use crate::{
	Error,
	models::{
		CloseReason, Comment, Commit, CommitDetails, Email, Issue, IssueState, Notification, Profile, ProfileUpdate,
		Repository, SocialAccount, SubjectDetails, User,
	},
	page::{Cursor, Kind, Page, next_link},
};

const API_URL: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
// Asks GitHub to render the markdown for us, so bodies arrive as `body_html` with the links,
// mentions and task lists already in place.
const HTML_ACCEPT: &str = "application/vnd.github.full+json";
const USER_AGENT: &str = concat!("gitview/", env!("CARGO_PKG_VERSION"));

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

pub struct Client {
	agent: ureq::Agent,
	token: String,
}

impl Client {
	#[must_use]
	pub fn new(token: String) -> Self {
		let agent = ureq::Agent::config_builder().user_agent(USER_AGENT).build().into();
		Self { agent, token }
	}

	pub fn current_user(&self) -> Result<User, Error> {
		self.get("/user")
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

	pub fn repositories(&self) -> Result<Page<Repository>, Error> {
		self.fetch_page("/user/repos?sort=updated&per_page=100", Kind::Plain)
	}

	/// Repositories the signed-in user has starred, most recently starred first.
	pub fn starred(&self) -> Result<Page<Repository>, Error> {
		self.fetch_page("/user/starred?sort=created&direction=desc&per_page=100", Kind::Plain)
	}

	/// Repositories the signed-in user watches, which GitHub calls subscriptions.
	pub fn watched(&self) -> Result<Page<Repository>, Error> {
		self.fetch_page("/user/subscriptions?per_page=100", Kind::Plain)
	}

	/// The newest commits on the default branch of `repo` (`owner/name`). A repository with no
	/// commits yet has none, which GitHub answers with a conflict rather than an empty list.
	pub fn commits(&self, repo: &str) -> Result<Page<Commit>, Error> {
		match self.fetch_page(&format!("/repos/{repo}/commits?per_page=100"), Kind::Plain) {
			Err(Error::Http(ureq::Error::StatusCode(409))) => Ok(Page::empty()),
			result => result,
		}
	}

	/// Commit `sha` in `repo` (`owner/name`), with the files it changed and how.
	pub fn commit(&self, repo: &str, sha: &str) -> Result<CommitDetails, Error> {
		self.get(&format!("/repos/{repo}/commits/{sha}"))
	}

	/// Issues and pull requests in `repo` (`owner/name`), newest first.
	pub fn issues(&self, repo: &str, state: IssueState) -> Result<Page<Issue>, Error> {
		self.fetch_page(&format!("/repos/{repo}/issues?state={}&per_page=100", state.as_parameter()), Kind::Rendered)
	}

	/// Issues and pull requests assigned to the signed-in user, across every repository.
	pub fn assigned(&self, state: IssueState) -> Result<Page<Issue>, Error> {
		self.search("assignee:@me", state)
	}

	/// Pull requests waiting for the signed-in user's review.
	pub fn review_requests(&self, state: IssueState) -> Result<Page<Issue>, Error> {
		self.search("is:pr review-requested:@me", state)
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
		self.fetch_page(&cursor.path, cursor.kind)
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
	fn search(&self, query: &str, state: IssueState) -> Result<Page<Issue>, Error> {
		let query = format!("{query} {}", state.as_qualifier());
		self.fetch_page(
			&format!("/search/issues?advanced_search=true&per_page=100&q={}", query.trim().replace(' ', "+")),
			Kind::Search,
		)
	}

	/// One page of a list at `path`, with where the next page is, as GitHub's `Link` header says.
	fn fetch_page<T: DeserializeOwned>(&self, path: &str, kind: Kind) -> Result<Page<T>, Error> {
		// The path alone: the token travels in a header, never in it.
		tracing::debug!(path, "fetching");
		let accept = if kind == Kind::Rendered { HTML_ACCEPT } else { "application/vnd.github+json" };
		let mut response =
			self.authorize(self.agent.get(format!("{API_URL}{path}"))).header("Accept", accept).call()?;
		// A link anywhere but the API is never followed, so the token never goes anywhere else.
		let next = response
			.headers()
			.get("Link")
			.and_then(|header| header.to_str().ok())
			.and_then(next_link)
			.and_then(|url| url.strip_prefix(API_URL))
			.map(|path| Cursor::new(path.to_owned(), kind));
		let items = if kind == Kind::Search {
			response.body_mut().read_json::<SearchResults<T>>()?.items
		} else {
			response.body_mut().read_json()?
		};
		Ok(Page { items, next })
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
