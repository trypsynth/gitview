use std::fmt::Write;

use chrono::{DateTime, Local};
use gitview_core::models::{Check, CheckResult, Commit, Issue, Repository, User};
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
		archived => repository.archived,
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
		checks => commit.checks().unwrap_or_default(),
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

/// How many checks ended each way, the failures first, such as "1 failed, 3 passed".
pub fn checks_summary(checks: &[Check]) -> String {
	let mut counts: Vec<(CheckResult, u64)> = Vec::new();
	for check in checks {
		match counts.iter_mut().find(|(result, _)| *result == check.result) {
			Some((_, count)) => *count += 1,
			None => counts.push((check.result, 1)),
		}
	}
	counts.sort_by_key(|(result, _)| *result);
	counts.iter().map(|(result, count)| format!("{count} {}", result.label())).collect::<Vec<_>>().join(", ")
}

/// Checks as a page lists them: each one's name, linking to its page when it has one, how it
/// went, and how long it took once it finished.
pub fn checks_html(checks: &[Check]) -> String {
	let mut html = String::from("<ul>");
	for check in checks {
		let mut name = web::escape(&check.name);
		if let Some(url) = &check.url {
			name = format!("<a href=\"{}\">{name}</a>", web::escape(url));
		}
		let mut how = check.result.label().to_owned();
		let took = check.started.as_deref().zip(check.finished.as_deref());
		if let Some(took) = took.and_then(|(started, finished)| duration(started, finished)) {
			how = format!("{how}, {took}");
		}
		// Writing to a String can't fail.
		let _ = write!(html, "<li>{name}: {how}</li>");
	}
	html.push_str("</ul>");
	html
}

/// How long something took between two GitHub timestamps, such as "45 seconds" or "3 minutes",
/// when both parse.
pub fn duration(started: &str, finished: &str) -> Option<String> {
	let started = started.parse::<DateTime<Local>>().ok()?;
	let finished = finished.parse::<DateTime<Local>>().ok()?;
	let seconds = u64::try_from((finished - started).num_seconds()).ok()?;
	Some(if seconds < 60 {
		count(seconds, "second", "seconds")
	} else {
		count((seconds + 30) / 60, "minute", "minutes")
	})
}

/// A file's size as people say it, such as "309 bytes", "12.4 KB" or "4.9 MB".
pub fn file_size(bytes: u64) -> String {
	const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
	if bytes < 1024 {
		return count(bytes, "byte", "bytes");
	}
	// Shown to one place, so the rounding of a float is all the precision there is.
	#[allow(clippy::cast_precision_loss)]
	let mut size = bytes as f64 / 1024.0;
	let mut unit = UNITS[0];
	for next in &UNITS[1..] {
		if size < 1024.0 {
			break;
		}
		size /= 1024.0;
		unit = next;
	}
	format!("{size:.1} {unit}")
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
	use gitview_core::models::{Check, CheckResult};

	use super::{checks_summary, duration, local_time};

	fn check(result: CheckResult) -> Check {
		Check { name: String::new(), result, started: None, finished: None, url: None }
	}

	#[test]
	fn checks_are_counted_failures_first() {
		let checks = [CheckResult::Passed, CheckResult::Failed, CheckResult::Passed, CheckResult::Running].map(check);
		assert_eq!(checks_summary(&checks), "1 failed, 1 running, 2 passed");
		assert_eq!(checks_summary(&[]), "");
	}

	#[test]
	fn a_duration_reads_in_seconds_then_minutes() {
		assert_eq!(duration("2026-11-24T00:17:00Z", "2026-11-24T00:17:45Z").as_deref(), Some("45 seconds"));
		assert_eq!(duration("2026-11-24T00:17:00Z", "2026-11-24T00:20:20Z").as_deref(), Some("3 minutes"));
		assert_eq!(duration("2026-11-24T00:17:00Z", "2026-11-24T00:18:00Z").as_deref(), Some("1 minute"));
		assert_eq!(duration("not a time", "2026-11-24T00:18:00Z"), None);
	}

	#[test]
	fn timestamps_read_as_a_date_and_time() {
		// The hour depends on the machine's time zone, so only the shape is checked.
		let time = local_time("2026-06-15T12:00:00Z");
		assert!(time.starts_with("Jun 15, 2026 at ") && (time.ends_with(" AM") || time.ends_with(" PM")), "{time}");
		assert_eq!(local_time("not a time"), "not a time");
	}
}
