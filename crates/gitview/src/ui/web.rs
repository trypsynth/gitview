//! The read pane: a web view, so issue bodies keep their links, lists and code blocks.

use wxdragon::{
	event::{WebViewEventData, WebViewEvents},
	prelude::*,
	utils::{BrowserLaunchFlags, launch_default_browser},
	widgets::WebView,
};

const MESSAGE_HANDLER: &str = "gitview";
const OPEN_LINK: &str = "open_link:";
const ESCAPE: &str = "escape";
// A link inside the pane opens in the real browser: the pane is for reading, and following a
// link in it would leave the user stranded with no way back. The browser keeps key presses to
// itself, so Escape is passed back from the page for the owner to move focus out.
const PAGE_SCRIPT: &str = "document.addEventListener('click', function(event) { \
	var target = event.target; \
	while (target && target.tagName !== 'A') { target = target.parentNode; } \
	if (target && target.href) { \
		event.preventDefault(); \
		window.gitview.postMessage('open_link:' + target.href); \
	} \
}); \
document.addEventListener('keydown', function(event) { \
	if (event.key === 'Escape') { \
		event.preventDefault(); \
		window.gitview.postMessage('escape'); \
	} \
});";
const STYLE: &str = "body { color-scheme: light dark; font-family: sans-serif; margin: 0; padding: 8px; } \
	h1 { font-size: 1.3em; margin: 0 0 4px; } \
	p.meta { margin: 0 0 12px; } \
	h2 { font-size: 1.1em; margin: 16px 0 4px; } \
	img { max-width: 100%; height: auto; } \
	pre { overflow-x: auto; }";

/// `on_escape` runs when Escape is pressed inside the page.
pub fn build(parent: &dyn WxWidget, on_escape: impl Fn() + 'static) -> WebView {
	let view = WebView::builder(parent).build();
	view.add_script_message_handler(MESSAGE_HANDLER);
	view.on_script_message_received(move |event: WebViewEventData| {
		let Some(message) = event.get_string() else {
			return;
		};
		if message == ESCAPE {
			on_escape();
		} else if let Some(url) = message.strip_prefix(OPEN_LINK) {
			launch_default_browser(url, BrowserLaunchFlags::Default);
		}
	});
	let view_for_load = view;
	view.on_loaded(move |_| {
		view_for_load.run_script(PAGE_SCRIPT);
	});
	view
}

/// Puts a page in the view. `heading` and `meta` are plain text, and an empty `meta` leaves its
/// line out; `body` is already HTML.
pub fn show(view: WebView, heading: &str, meta: &str, body: &str) {
	let heading = escape(heading);
	let meta = if meta.is_empty() { String::new() } else { format!("<p class=\"meta\">{}</p>", escape(meta)) };
	let page = format!(
		"<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{heading}</title><style>{STYLE}</style></head>\
		<body><h1>{heading}</h1>{meta}{body}</body></html>",
	);
	view.set_page(&page, "https://github.com/");
}

/// Wraps plain text as a paragraph, for the bodies GitHub did not render for us.
pub fn paragraph(text: &str) -> String {
	format!("<p>{}</p>", escape(text).replace('\n', "<br>"))
}

pub fn escape(text: &str) -> String {
	text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
