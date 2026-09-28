use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ureq::RequestBuilder;

use crate::{
	Error,
	models::{Comment, Issue, IssueState, Notification, Repository, SubjectDetails, User},
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
