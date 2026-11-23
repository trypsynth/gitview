use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct User {
	pub login: String,
	pub name: Option<String>,
	/// `User` or `Organization`, when GitHub says.
	#[serde(rename = "type", default)]
	pub kind: Option<String>,
}

impl User {
	#[must_use]
	pub fn is_organization(&self) -> bool {
		self.kind.as_deref() == Some("Organization")
	}
}

/// Anyone's public profile, as their page on GitHub shows it.
#[derive(Debug, Clone, Deserialize)]
pub struct UserProfile {
	pub login: String,
	pub name: Option<String>,
	/// `User` or `Organization`.
	#[serde(rename = "type")]
	pub kind: String,
	pub bio: Option<String>,
	pub company: Option<String>,
	pub location: Option<String>,
	/// The website link; GitHub calls it the blog. It may leave out `https://`.
	pub blog: Option<String>,
	pub html_url: String,
	pub followers: u64,
	pub following: u64,
	pub public_repos: u64,
	pub created_at: String,
}

impl UserProfile {
	#[must_use]
	pub fn is_organization(&self) -> bool {
		self.kind == "Organization"
	}
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
	pub html_url: String,
	pub description: Option<String>,
	pub private: bool,
	pub fork: bool,
	/// Whether its owner archived it, leaving it read-only.
	#[serde(default)]
	pub archived: bool,
	pub stargazers_count: u64,
	pub open_issues_count: u64,
}

/// A commit as a repository's history lists it.
#[derive(Debug, Clone, Deserialize)]
pub struct Commit {
	pub sha: String,
	pub html_url: String,
	pub commit: CommitData,
	/// The account the commit's author address belongs to. An address GitHub doesn't know leaves
	/// it out, or sends an empty one.
	pub author: Option<CommitAccount>,
}

impl Commit {
	/// The message's first line.
	#[must_use]
	pub fn summary(&self) -> &str {
		self.commit.message.lines().next().unwrap_or_default()
	}

	/// The message after its first line and the blank line under it, if there is any.
	#[must_use]
	pub fn description(&self) -> &str {
		self.commit.message.split_once('\n').map_or("", |(_, rest)| rest.trim())
	}

	/// The first seven characters of the hash, as GitHub shows it.
	#[must_use]
	pub fn short_sha(&self) -> &str {
		self.sha.get(..7).unwrap_or(&self.sha)
	}

	/// The author's GitHub login, or the name in the commit when the address has no account.
	#[must_use]
	pub fn author_name(&self) -> &str {
		self.author.as_ref().and_then(|account| account.login.as_deref()).unwrap_or(&self.commit.author.name)
	}
}

/// The GitHub account behind a commit. Its login is missing when GitHub sends an empty account,
/// as it does for some addresses it can't match, such as a bot's.
#[derive(Debug, Clone, Deserialize)]
pub struct CommitAccount {
	pub login: Option<String>,
}

/// The commit as git records it, apart from where it sits in the history.
#[derive(Debug, Clone, Deserialize)]
pub struct CommitData {
	pub message: String,
	pub author: GitAuthor,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitAuthor {
	pub name: String,
	/// When the change was written, which a rebase keeps.
	pub date: String,
}

/// One commit with what it changed.
#[derive(Debug, Clone, Deserialize)]
pub struct CommitDetails {
	#[serde(flatten)]
	pub commit: Commit,
	pub stats: CommitStats,
	pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitStats {
	pub additions: u64,
	pub deletions: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangedFile {
	pub filename: String,
	/// What happened to it: `added`, `removed`, `modified`, `renamed`, `copied`, `changed` or
	/// `unchanged`.
	pub status: String,
	pub additions: u64,
	pub deletions: u64,
	/// The name it had before a rename.
	pub previous_filename: Option<String>,
	/// The unified diff of the change, which binary files and very large changes leave out.
	pub patch: Option<String>,
}

/// Why an issue was closed, as GitHub records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
	Completed,
	NotPlanned,
	/// A duplicate of the issue with this id (not number).
	Duplicate(u64),
}

/// The orders GitHub can give the signed-in user's repositories in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryOrder {
	/// Most recently updated first.
	Updated,
	/// Most recently pushed to first.
	Pushed,
	/// Newest first.
	Created,
	/// By owner and name, A to Z.
	Name,
}

impl RepositoryOrder {
	pub(crate) const fn parameters(self) -> &'static str {
		match self {
			Self::Updated => "sort=updated&direction=desc",
			Self::Pushed => "sort=pushed&direction=desc",
			Self::Created => "sort=created&direction=desc",
			Self::Name => "sort=full_name&direction=asc",
		}
	}
}

/// Which of the signed-in user's repositories GitHub lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryKind {
	/// Every one they can reach: their own, ones they collaborate on, and their organizations'.
	All,
	/// Only the ones they own.
	Owned,
	Public,
	Private,
}

impl RepositoryKind {
	pub(crate) const fn parameter(self) -> &'static str {
		match self {
			Self::All => "type=all",
			Self::Owned => "type=owner",
			Self::Public => "type=public",
			Self::Private => "type=private",
		}
	}
}

/// The orders GitHub can give starred repositories in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarredOrder {
	/// Most recently starred first.
	Starred,
	/// Most recently updated first.
	Updated,
}

impl StarredOrder {
	pub(crate) const fn parameters(self) -> &'static str {
		match self {
			Self::Starred => "sort=created&direction=desc",
			Self::Updated => "sort=updated&direction=desc",
		}
	}
}

/// The orders GitHub's issue search can give results in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOrder {
	/// Most recently updated first.
	Updated,
	/// Most recently opened first.
	Created,
	/// Most comments first.
	Comments,
}

impl SearchOrder {
	pub(crate) const fn parameters(self) -> &'static str {
		match self {
			Self::Updated => "sort=updated&order=desc",
			Self::Created => "sort=created&order=desc",
			Self::Comments => "sort=comments&order=desc",
		}
	}
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
	/// GitHub's id for the issue, which marking another issue as its duplicate takes.
	pub id: u64,
	pub number: u64,
	pub title: String,
	pub state: String,
	/// Why a closed issue was closed: `completed`, `not_planned` or `duplicate`. Open issues and
	/// pull requests have none.
	pub state_reason: Option<String>,
	pub user: User,
	pub body: Option<String>,
	/// GitHub's own rendering of `body`, when the request asked for it.
	pub body_html: Option<String>,
	pub comments: u64,
	pub created_at: String,
	pub html_url: String,
	/// Only a pull request has this.
	pull_request: Option<PullRequestLinks>,
	/// Only the search endpoint sends this; the per-repository listings leave it out.
	repository_url: Option<String>,
}

impl Issue {
	#[must_use]
	pub const fn is_pull_request(&self) -> bool {
		self.pull_request.is_some()
	}

	/// Whether this is a pull request that was merged, rather than closed without merging.
	#[must_use]
	pub fn is_merged(&self) -> bool {
		self.pull_request.as_ref().is_some_and(|links| links.merged_at.is_some())
	}

	#[must_use]
	pub fn is_open(&self) -> bool {
		self.state == "open"
	}

	/// The `owner/name` the issue belongs to, for results that did not come from one repository.
	#[must_use]
	pub fn repository(&self) -> Option<&str> {
		self.repository_url.as_deref()?.strip_prefix("https://api.github.com/repos/")
	}
}

/// What an issue listing says about the pull request an issue is.
#[derive(Debug, Clone, Deserialize)]
struct PullRequestLinks {
	merged_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Comment {
	pub user: User,
	pub body: Option<String>,
	pub body_html: Option<String>,
	pub created_at: String,
	pub html_url: String,
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

	#[must_use]
	pub fn is_pull_request(&self) -> bool {
		self.subject.kind == "PullRequest"
	}

	/// The issue or pull request's page on GitHub. Other kinds only say where theirs is once
	/// fetched, in [`SubjectDetails::html_url`].
	#[must_use]
	pub fn issue_url(&self) -> Option<String> {
		let number = self.issue_number()?;
		let kind = if self.is_pull_request() { "pull" } else { "issues" };
		Some(format!("https://github.com/{}/{kind}/{number}", self.repository.full_name))
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
	pub html_url: Option<String>,
}

impl SubjectDetails {
	#[must_use]
	pub fn heading(&self) -> Option<&str> {
		self.title.as_deref().or(self.name.as_deref()).or(self.tag_name.as_deref())
	}
}

#[cfg(test)]
mod tests {
	use super::{Commit, CommitAccount, CommitData, GitAuthor};

	fn commit(message: &str, author: Option<&str>) -> Commit {
		Commit {
			sha: "1b4b5cf5d990986ea291b4eb45f46e71afbe72b2".to_owned(),
			html_url: String::new(),
			commit: CommitData {
				message: message.to_owned(),
				author: GitAuthor { name: "Quin Gillespie".to_owned(), date: String::new() },
			},
			author: author.map(|login| CommitAccount { login: Some(login.to_owned()) }),
		}
	}

	#[test]
	fn a_message_splits_into_summary_and_description() {
		let both = commit("Fix the list\n\nIt lost its place.\nNow it doesn't.\n", None);
		assert_eq!(both.summary(), "Fix the list");
		assert_eq!(both.description(), "It lost its place.\nNow it doesn't.");
		let summary_only = commit("Fix the list", None);
		assert_eq!(summary_only.summary(), "Fix the list");
		assert_eq!(summary_only.description(), "");
		assert_eq!(commit("", None).summary(), "");
	}

	fn issue(state: &str, state_reason: &str, pull_request: &str) -> super::Issue {
		serde_json::from_str(&format!(
			r#"{{"id": 1, "number": 4, "title": "t", "state": "{state}", "state_reason": {state_reason},
			"user": {{"login": "hexxbyte"}}, "body": null, "comments": 0, "created_at": "2026-09-29T02:29:08Z",
			"html_url": "https://github.com/trypsynth/gitview/issues/4", "pull_request": {pull_request}}}"#
		))
		.expect("an issue as GitHub lists it")
	}

	#[test]
	fn a_closed_issue_keeps_why_and_a_pull_request_whether_it_merged() {
		let closed = issue("closed", r#""not_planned""#, "null");
		assert_eq!(closed.state_reason.as_deref(), Some("not_planned"));
		assert!(!closed.is_pull_request());
		let merged = issue("closed", "null", r#"{"url": "u", "merged_at": "2026-11-16T22:58:41Z"}"#);
		assert!(merged.is_pull_request() && merged.is_merged());
		let unmerged = issue("closed", "null", r#"{"url": "u", "merged_at": null}"#);
		assert!(unmerged.is_pull_request() && !unmerged.is_merged());
	}

	#[test]
	fn an_empty_account_falls_back_to_the_name_in_the_commit() {
		let commit: Commit = serde_json::from_str(
			r#"{"sha": "8306b27", "html_url": "", "author": {},
			"commit": {"message": "Refactor", "author": {"name": "Sourcery AI", "date": ""}}}"#,
		)
		.expect("a commit whose author GitHub couldn't match");
		assert_eq!(commit.author_name(), "Sourcery AI");
	}

	#[test]
	fn a_commit_is_named_by_its_short_hash_and_its_author() {
		assert_eq!(commit("", None).short_sha(), "1b4b5cf");
		assert_eq!(commit("", Some("trypsynth")).author_name(), "trypsynth");
		assert_eq!(commit("", None).author_name(), "Quin Gillespie");
	}
}
