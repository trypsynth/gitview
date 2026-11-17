use chrono::{DateTime, Local};
use gitview_core::models::{Commit, Issue, Repository, User};
use minijinja::context;

use super::web;
use crate::template::{self, Kind};

/// A repository as its template shows it.
pub fn repository_label(repository: &Repository) -> String {
	let context = context! {
		name => &repository.full_name,
		description => repository.description.as_deref().unwrap_or_default(),
		private => repository.private,
		fork => repository.fork,
		stars => repository.stargazers_count,
		open_issues => repository.open_issues_count,
	};
	template::render(Kind::Repository, &context)
}

/// An issue or pull request as its template shows it. In a repository's own dialog, where every
/// row shares the repository, it goes unsaid.
pub fn issue_label(issue: &Issue, in_repository: bool) -> String {
	let ending = if issue.is_open() {
		String::new()
	} else if issue.is_pull_request() {
		if issue.is_merged() { "merged".to_owned() } else { "closed".to_owned() }
	} else {
		issue.state_reason.as_deref().unwrap_or("closed").replace('_', " ")
	};
	let context = context! {
		repository => if in_repository { "" } else { issue.repository().unwrap_or("unknown") },
		number => issue.number,
		title => &issue.title,
		kind => if issue.is_pull_request() { "pull request" } else { "issue" },
		author => &issue.user.login,
		comments => comment_count(issue.comments),
		state => &issue.state,
		ending,
		time => local_time(&issue.created_at),
	};
	template::render(Kind::Issue, &context)
}

/// A commit as its template shows it.
pub fn commit_label(commit: &Commit) -> String {
	let context = context! {
		summary => commit.summary(),
		author => commit.author_name(),
		author_name => &commit.commit.author.name,
		time => local_time(&commit.commit.author.date),
		sha => commit.short_sha(),
	};
	template::render(Kind::Commit, &context)
}

/// A person, or an organization, as its template shows it.
pub fn person_label(user: &User) -> String {
	let context = context! {
		login => &user.login,
		name => user.name.as_deref().unwrap_or_default(),
		organization => user.is_organization(),
	};
	template::render(Kind::Person, &context)
}

/// "1 comment", not "1 comments".
pub fn comment_count(comments: u64) -> String {
	count(comments, "comment", "comments")
}

/// `number` with the word for one or for many after it, such as "1 file" or "3 files".
pub fn count(number: u64, one: &str, many: &str) -> String {
	format!("{number} {}", if number == 1 { one } else { many })
}

/// GitHub's own rendering of a body when the request asked for it, the raw markdown otherwise.
pub fn body_html(html: Option<&str>, markdown: Option<&str>) -> String {
	if let Some(html) = html.map(str::trim).filter(|html| !html.is_empty()) {
		return html.to_string();
	}
	markdown
		.map(str::trim)
		.filter(|body| !body.is_empty())
		.map_or_else(|| "<p>No description.</p>".to_string(), web::paragraph)
}

/// A GitHub timestamp in the user's own time zone, such as "Sep 28, 2026 at 3:04 PM".
pub fn local_time(timestamp: &str) -> String {
	timestamp
		.parse::<DateTime<Local>>()
		.map_or_else(|_| timestamp.to_string(), |time| time.format("%b %-d, %Y at %-I:%M %p").to_string())
}

#[cfg(test)]
mod tests {
	use super::local_time;

	#[test]
	fn timestamps_read_as_a_date_and_time() {
		// The hour depends on the machine's time zone, so only the shape is checked.
		let time = local_time("2026-06-15T12:00:00Z");
		assert!(time.starts_with("Jun 15, 2026 at ") && (time.ends_with(" AM") || time.ends_with(" PM")), "{time}");
		assert_eq!(local_time("not a time"), "not a time");
	}
}
