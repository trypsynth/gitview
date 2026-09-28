//! Plain text from rendered HTML, for reading line by line: a line per block, list items
//! bulleted or numbered, code blocks kept as they are, and table rows as tab-separated cells.
//!
//! A trimmed port of Paperback's `HtmlToText`, keeping the text and leaving out the document
//! structure (heading, link and table positions) Paperback records for navigation.

use std::fmt::Write;

use ego_tree::NodeRef;
use scraper::{Html, Node};

const SEPARATOR: &str = "----------------------------------------";
const SKIPPED_TAGS: &[&str] = &["head", "script", "style", "noscript", "template", "iframe", "object", "embed", "svg"];
const BLOCK_TAGS: &[&str] = &[
	"address",
	"article",
	"aside",
	"blockquote",
	"dd",
	"details",
	"div",
	"dl",
	"dt",
	"figcaption",
	"figure",
	"footer",
	"h1",
	"h2",
	"h3",
	"h4",
	"h5",
	"h6",
	"header",
	"hr",
	"li",
	"main",
	"nav",
	"ol",
	"p",
	"pre",
	"section",
	"summary",
	"table",
	"ul",
];

#[must_use]
pub fn html_to_text(html: &str) -> String {
	let fragment = Html::parse_fragment(html);
	let mut writer = Writer::default();
	writer.walk(fragment.tree.root());
	writer.finish_line();
	writer.lines.join("\n")
}

struct List {
	ordered: bool,
	next: i64,
}

#[derive(Default)]
struct Writer {
	lines: Vec<String>,
	line: String,
	/// Depth of `<pre>` and `<code>` elements, inside which whitespace is kept as written.
	preserve: usize,
	lists: Vec<List>,
	/// Leading spaces for the line being built, kept apart so trimming it leaves them alone.
	indent: usize,
}

impl Writer {
	fn walk(&mut self, node: NodeRef<'_, Node>) {
		let element = match node.value() {
			Node::Text(text) => {
				self.text(&text.text);
				return;
			}
			Node::Element(element) => element,
			_ => {
				self.walk_children(node);
				return;
			}
		};
		let tag = element.name();
		if SKIPPED_TAGS.contains(&tag) {
			return;
		}
		match tag {
			"table" => {
				self.table(node);
				return;
			}
			"br" => self.finish_line(),
			"hr" => {
				self.finish_line();
				self.lines.push(SEPARATOR.to_owned());
			}
			"img" => {
				if let Some(alt) = element.attr("alt").map(collapse_whitespace).filter(|alt| !alt.trim().is_empty()) {
					let _ = write!(self.line, "[Image: {}]", alt.trim());
				}
			}
			// GitHub renders task list items as disabled checkboxes.
			"input" if element.attr("type") == Some("checkbox") => {
				self.line.push_str(if element.attr("checked").is_some() { "[x] " } else { "[ ] " });
			}
			"pre" => {
				self.finish_line();
				self.preserve += 1;
			}
			"code" => self.preserve += 1,
			"ul" | "ol" => {
				self.finish_line();
				let next = element.attr("start").and_then(|start| start.parse().ok()).unwrap_or(1);
				self.lists.push(List { ordered: tag == "ol", next });
			}
			"li" => {
				self.finish_line();
				self.bullet();
			}
			_ => {}
		}
		self.walk_children(node);
		match tag {
			"pre" => {
				// The newline before `</pre>` would otherwise leave an empty line behind.
				if self.line.trim().is_empty() {
					self.line.clear();
				} else {
					self.finish_line();
				}
				self.preserve = self.preserve.saturating_sub(1);
			}
			"code" => self.preserve = self.preserve.saturating_sub(1),
			"ul" | "ol" => {
				self.lists.pop();
			}
			_ => {}
		}
		if BLOCK_TAGS.contains(&tag) {
			self.finish_line();
		}
	}

	fn walk_children(&mut self, node: NodeRef<'_, Node>) {
		for child in node.children() {
			self.walk(child);
		}
	}

	fn text(&mut self, text: &str) {
		let text = text.replace('\u{ad}', "");
		if self.preserve == 0 {
			self.line.push_str(&collapse_whitespace(&text));
			return;
		}
		let mut lines = text.split('\n').peekable();
		while let Some(line) = lines.next() {
			self.line.push_str(line);
			if lines.peek().is_some() {
				self.finish_line();
			}
		}
	}

	fn bullet(&mut self) {
		let depth = self.lists.len();
		self.indent = depth.saturating_sub(1) * 2;
		match self.lists.last_mut() {
			Some(list) if list.ordered => {
				let _ = write!(self.line, "{}. ", list.next);
				list.next += 1;
			}
			_ => {
				self.line.push_str(match depth {
					2 => "◦ ",
					3 => "* ",
					4 => "- ",
					_ => "• ",
				});
			}
		}
	}

	fn table(&mut self, table: NodeRef<'_, Node>) {
		self.finish_line();
		let mut rows = Vec::new();
		collect_rows(table, &mut rows);
		self.lines.extend(rows);
	}

	/// Ends the line being built. Outside code, whitespace is collapsed and a blank line
	/// dropped; inside, the line is kept as written, blank or not.
	fn finish_line(&mut self) {
		let mut line = std::mem::take(&mut self.line);
		let indent = std::mem::take(&mut self.indent);
		if self.preserve > 0 {
			while line.ends_with(['\n', '\r']) {
				line.pop();
			}
			self.lines.push(line);
			return;
		}
		let line = collapse_whitespace(&line);
		let line = line.trim();
		if !line.is_empty() {
			self.lines.push(format!("{:indent$}{line}", ""));
		}
	}
}

/// The rows of `node`'s table, through `thead`, `tbody` and `tfoot`, but not into a table
/// nested in a cell: that one's text belongs to its cell.
fn collect_rows(node: NodeRef<'_, Node>, rows: &mut Vec<String>) {
	for child in node.children() {
		let Node::Element(element) = child.value() else {
			continue;
		};
		match element.name() {
			"tr" => {
				let mut cells = Vec::new();
				collect_cells(child, &mut cells);
				rows.push(cells.join("\t"));
			}
			"table" => {}
			_ => collect_rows(child, rows),
		}
	}
}

fn collect_cells(node: NodeRef<'_, Node>, cells: &mut Vec<String>) {
	for child in node.children() {
		let Node::Element(element) = child.value() else {
			continue;
		};
		match element.name() {
			"td" | "th" => {
				let mut text = String::new();
				collect_text(child, &mut text);
				cells.push(collapse_whitespace(&text).trim().to_owned());
			}
			"table" => {}
			_ => collect_cells(child, cells),
		}
	}
}

/// Every text node under `node`, with a `<br>` read as a space so the words either side of
/// it stay apart.
fn collect_text(node: NodeRef<'_, Node>, text: &mut String) {
	match node.value() {
		Node::Text(content) => text.push_str(content),
		Node::Element(element) => {
			if element.name() == "br" {
				text.push(' ');
			}
			for child in node.children() {
				collect_text(child, text);
			}
		}
		_ => {}
	}
}

/// Runs of whitespace become one space. Spaces at either end are kept, as one, since text
/// nodes meet their neighbours there: the space in `see <a>this</a>` sits at the end of one.
fn collapse_whitespace(text: &str) -> String {
	let mut result = String::with_capacity(text.len());
	let mut space = false;
	for ch in text.chars() {
		if ch.is_whitespace() {
			space = true;
			continue;
		}
		if space {
			result.push(' ');
			space = false;
		}
		result.push(ch);
	}
	if space {
		result.push(' ');
	}
	result
}

#[cfg(test)]
mod tests {
	use super::html_to_text;

	#[test]
	fn paragraphs_and_breaks_become_lines() {
		assert_eq!(html_to_text("<p>One\n two</p><p>Three<br>four</p>"), "One two\nThree\nfour");
	}

	#[test]
	fn inline_elements_keep_their_spaces() {
		assert_eq!(html_to_text("<p>See <a href=\"x\">this</a> and <strong>that</strong>.</p>"), "See this and that.");
	}

	#[test]
	fn lists_are_bulleted_numbered_and_nested() {
		let html = "<ul><li>a<ul><li>b</li></ul></li></ul><ol start=\"3\"><li>c</li><li>d</li></ol>";
		assert_eq!(html_to_text(html), "• a\n  ◦ b\n3. c\n4. d");
	}

	#[test]
	fn task_lists_show_their_checkboxes() {
		let html = "<ul><li><input type=\"checkbox\" checked disabled> done</li><li><input type=\"checkbox\" disabled> todo</li></ul>";
		assert_eq!(html_to_text(html), "• [x] done\n• [ ] todo");
	}

	#[test]
	fn code_blocks_keep_their_layout() {
		let html = "<div class=\"highlight\"><pre><span>fn main() {</span>\n    <span>run();</span>\n\n}\n</pre></div>";
		assert_eq!(html_to_text(html), "fn main() {\n    run();\n\n}");
	}

	#[test]
	fn tables_become_tab_separated_rows() {
		let html = "<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>2 <br>3</td></tr></tbody></table>";
		assert_eq!(html_to_text(html), "A\tB\n1\t2 3");
	}

	#[test]
	fn images_are_described_by_their_alt_text() {
		assert_eq!(html_to_text("<p><img alt=\"a cat\" src=\"x\"> <img src=\"y\"></p>"), "[Image: a cat]");
	}
}
