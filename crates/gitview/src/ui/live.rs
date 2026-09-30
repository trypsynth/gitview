//! The label a window speaks through, to confirm what changes nothing on screen.

use wxdragon::prelude::*;

/// Makes a label on `parent` to announce through. Create it after the controls near it, so no
/// screen reader takes it for one of their labels.
///
/// The label stays shown: UI Automation drops notifications raised from a hidden window, so a
/// hidden one would announce to no one. At one pixel with no text there is nothing to see, and a
/// label is never in the tab order.
pub fn live_region(parent: &impl WxWidget) -> StaticText {
	let label = StaticText::builder(parent).with_label("").with_size(Size::new(1, 1)).build();
	// Set up before a screen reader first looks at the label, which it would otherwise remember
	// as unable to announce.
	let _ = live_region::set_live_region(&label);
	label
}
