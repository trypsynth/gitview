//! The read pane: a web view, so issue bodies keep their links, lists and code blocks.

use wxdragon::{
	event::{WebViewEventData, WebViewEvents},
	prelude::*,
	utils::{BrowserLaunchFlags, launch_default_browser},
	widgets::WebView,
};

const MESSAGE_HANDLER: &str = "gitview";
const OPEN_LINK: &str = "open_link:";
// A link inside the pane opens in the real browser: the pane is for reading, and following a
// link in it would leave the user stranded with no way back.
const LINK_SCRIPT: &str = "document.addEventListener('click', function(event) { \
	var target = event.target; \
	while (target && target.tagName !== 'A') { target = target.parentNode; } \
	if (target && target.href) { \
		event.preventDefault(); \
		window.gitview.postMessage('open_link:' + target.href); \
	} \
});";
const STYLE: &str = "body { color-scheme: light dark; font-family: sans-serif; margin: 0; padding: 8px; } \
	h1 { font-size: 1.3em; margin: 0 0 4px; } \
	p.meta { margin: 0 0 12px; } \
	h2 { font-size: 1.1em; margin: 16px 0 4px; } \
	img { max-width: 100%; height: auto; } \
	pre { overflow-x: auto; }";

pub fn build(parent: &dyn WxWidget) -> WebView {
	let view = WebView::builder(parent).build();
	view.add_script_message_handler(MESSAGE_HANDLER);
	view.on_script_message_received(move |event: WebViewEventData| {
		if let Some(url) = event.get_string().as_deref().and_then(|message| message.strip_prefix(OPEN_LINK)) {
			launch_default_browser(url, BrowserLaunchFlags::Default);
		}
	});
	let view_for_load = view;
	view.on_loaded(move |_| {
		view_for_load.run_script(LINK_SCRIPT);
	});
	view
}

/// Puts a page in the view. `heading` and `meta` are plain text; `body` is already HTML.
pub fn show(view: WebView, heading: &str, meta: &str, body: &str) {
	let heading = escape(heading);
	let page = format!(
		"<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{heading}</title><style>{STYLE}</style></head>\
		<body><h1>{heading}</h1><p class=\"meta\">{}</p>{body}</body></html>",
		escape(meta),
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
