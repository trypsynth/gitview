use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ureq::RequestBuilder;

use crate::{
	Error,
	models::{
		CloseReason, Comment, Email, Issue, IssueState, Notification, Profile, ProfileUpdate, Repository,
		SocialAccount, SubjectDetails, User,
	},
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
struct SocialAccountUrls<'a> {
	account_urls: &'a [String],
}

#[derive(Deserialize)]
struct SearchResults {
	items: Vec<Issue>,
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
	pub fn notifications(&self, all: bool) -> Result<Vec<Notification>, Error> {
		self.get(&format!("/notifications?per_page=50&all={all}"))
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

	pub fn repositories(&self) -> Result<Vec<Repository>, Error> {
		self.get("/user/repos?sort=updated&per_page=100")
	}

	pub fn starred(&self) -> Result<Vec<Repository>, Error> {
		self.get("/user/starred?per_page=100")
	}

	/// Issues and pull requests in `repo` (`owner/name`), newest first.
	pub fn issues(&self, repo: &str, state: IssueState) -> Result<Vec<Issue>, Error> {
		self.get_rendered(&format!("/repos/{repo}/issues?state={}&per_page=100", state.as_parameter()))
	}

	/// Issues and pull requests assigned to the signed-in user, across every repository.
	pub fn assigned(&self, state: IssueState) -> Result<Vec<Issue>, Error> {
		self.search("assignee:@me", state)
	}

	/// Pull requests waiting for the signed-in user's review.
	pub fn review_requests(&self, state: IssueState) -> Result<Vec<Issue>, Error> {
		self.search("is:pr review-requested:@me", state)
	}

	pub fn issue(&self, repo: &str, number: u64) -> Result<Issue, Error> {
		self.get_rendered(&format!("/repos/{repo}/issues/{number}"))
	}

	pub fn comments(&self, repo: &str, number: u64) -> Result<Vec<Comment>, Error> {
		self.get_rendered(&format!("/repos/{repo}/issues/{number}/comments?per_page=100"))
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
	fn search(&self, query: &str, state: IssueState) -> Result<Vec<Issue>, Error> {
		let query = format!("{query} {}", state.as_qualifier());
		let results: SearchResults = self
			.get(&format!("/search/issues?advanced_search=true&per_page=100&q={}", query.trim().replace(' ', "+")))?;
		Ok(results.items)
	}

	fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
		self.fetch(path, "application/vnd.github+json")
	}

	/// A GET that asks for bodies as HTML as well as markdown.
	fn get_rendered<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
		self.fetch(path, HTML_ACCEPT)
	}

	fn fetch<T: DeserializeOwned>(&self, path: &str, accept: &str) -> Result<T, Error> {
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
