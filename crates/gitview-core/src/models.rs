use serde::{Deserialize, Serialize, de::IgnoredAny};

#[derive(Debug, Clone, Deserialize)]
pub struct User {
	pub login: String,
	pub name: Option<String>,
}

/// The signed-in user's public profile, as the profile settings page shows it.
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
	pub login: String,
	pub name: Option<String>,
	/// The address shown on the profile, if any.
	pub email: Option<String>,
	pub bio: Option<String>,
	/// The website link; GitHub calls it the blog.
	pub blog: Option<String>,
	pub company: Option<String>,
	pub location: Option<String>,
	pub hireable: Option<bool>,
}

/// The fields of a profile edit. GitHub treats an empty string as clearing a field.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileUpdate {
	pub name: String,
	/// Left out when unchanged: GitHub rejects a public address that is not verified.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub email: Option<String>,
	pub bio: String,
	pub blog: String,
	pub company: String,
	pub location: String,
	pub hireable: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Email {
	pub email: String,
	pub verified: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SocialAccount {
	pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repository {
	pub full_name: String,
	pub description: Option<String>,
	pub private: bool,
	pub fork: bool,
	pub stargazers_count: u64,
	pub open_issues_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueState {
	Open,
	Closed,
	All,
}

impl IssueState {
	/// The value the issue listing endpoints take as their `state` parameter.
	#[must_use]
	pub const fn as_parameter(self) -> &'static str {
		match self {
			Self::Open => "open",
			Self::Closed => "closed",
			Self::All => "all",
		}
	}

	/// The qualifier the search endpoint takes instead, empty when both states are wanted.
	#[must_use]
	pub const fn as_qualifier(self) -> &'static str {
		match self {
			Self::Open => "is:open",
			Self::Closed => "is:closed",
			Self::All => "",
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
pub struct Issue {
	pub number: u64,
	pub title: String,
	pub state: String,
	pub user: User,
	pub body: Option<String>,
	/// GitHub's own rendering of `body`, when the request asked for it.
	pub body_html: Option<String>,
	pub comments: u64,
	pub created_at: String,
	pull_request: Option<IgnoredAny>,
	/// Only the search endpoint sends this; the per-repository listings leave it out.
	repository_url: Option<String>,
}

impl Issue {
	#[must_use]
	pub const fn is_pull_request(&self) -> bool {
		self.pull_request.is_some()
	}

	/// The `owner/name` the issue belongs to, for results that did not come from one repository.
	#[must_use]
	pub fn repository(&self) -> Option<&str> {
		self.repository_url.as_deref()?.strip_prefix("https://api.github.com/repos/")
	}
}

#[derive(Debug, Clone, Deserialize)]
pub struct Comment {
	pub user: User,
	pub body: Option<String>,
	pub body_html: Option<String>,
	pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Notification {
	pub id: String,
	pub unread: bool,
	pub reason: String,
	pub subject: NotificationSubject,
	pub repository: NotificationRepository,
}

impl Notification {
	/// The issue or pull request number, when the notification is about one.
	#[must_use]
	pub fn issue_number(&self) -> Option<u64> {
		if !matches!(self.subject.kind.as_str(), "Issue" | "PullRequest") {
			return None;
		}
		self.subject.url.as_deref()?.rsplit('/').next()?.parse().ok()
	}
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationSubject {
	pub title: String,
	#[serde(rename = "type")]
	pub kind: String,
	pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationRepository {
	pub full_name: String,
}

/// What a notification points at when it is not an issue or a pull request, such as a release.
/// The fields differ per kind, so each one is optional.
#[derive(Debug, Clone, Deserialize)]
pub struct SubjectDetails {
	pub title: Option<String>,
	pub name: Option<String>,
	pub tag_name: Option<String>,
	pub body: Option<String>,
	pub body_html: Option<String>,
}

impl SubjectDetails {
	#[must_use]
	pub fn heading(&self) -> Option<&str> {
		self.title.as_deref().or(self.name.as_deref()).or(self.tag_name.as_deref())
	}
}
