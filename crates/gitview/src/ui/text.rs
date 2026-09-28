use super::web;

/// "1 comment", not "1 comments".
pub fn comment_count(comments: u64) -> String {
	if comments == 1 { "1 comment".to_string() } else { format!("{comments} comments") }
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
