use chrono::{DateTime, Local};
use gitview_core::models::Repository;

use super::web;

/// A repository as a list shows it: its name, what it's for when it says, and whether it's
/// private or a fork.
pub fn repository_label(repository: &Repository) -> String {
	let mut label = repository.full_name.clone();
	if let Some(description) = repository.description.as_deref().filter(|text| !text.is_empty()) {
		label.push_str(": ");
		label.push_str(description);
	}
	if repository.private {
		label.push_str(", private");
	}
	if repository.fork {
		label.push_str(", fork");
	}
	label
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
