//! A list of sections beside the items of the one chosen, as the main window and a repository's
//! dialog lay themselves out. Each section has a quiet list of its own, and only the chosen one is
//! shown.

use std::{
	cell::{Cell, RefCell},
	rc::{Rc, Weak},
};

use quiet_list::QuietList;
use wx_utils::dialog_padding;
use wxdragon::prelude::*;

/// What runs when the section on screen changes.
type Changed<S> = Rc<dyn Fn(S)>;

/// A section's list of items and the label that names it.
struct Entry<S> {
	section: S,
	title: String,
	label: StaticText,
	list: Rc<QuietList<String>>,
}

struct Inner<S> {
	sections: ListBox,
	sections_label: StaticText,
	/// Every section, in the order their lists were made.
	entries: Vec<Entry<S>>,
	/// The sections the Sections list shows, in its order.
	order: RefCell<Vec<S>>,
	current: Cell<S>,
	changed: RefCell<Option<Changed<S>>>,
}

/// Sections on the left and the chosen section's items on the right. Clones share the same
/// controls.
pub struct SectionedLists<S> {
	inner: Rc<Inner<S>>,
}

impl<S> Clone for SectionedLists<S> {
	fn clone(&self) -> Self {
		Self { inner: Rc::clone(&self.inner) }
	}
}

impl<S: Copy + Eq + 'static> SectionedLists<S> {
	/// Makes the controls on `parent`: the sections list under `label`, then a labelled list for
	/// each of `sections`, named by its title. The Sections list shows every section, in this
	/// order, until [`set_order`](Self::set_order) says otherwise.
	///
	/// # Panics
	///
	/// If `sections` is empty, since there would be nothing to show.
	pub fn new(parent: &impl WxWidget, label: &str, sections: &[(S, &str)]) -> Self {
		assert!(!sections.is_empty(), "a sectioned list needs a section");
		// Each label is made just before its control, so screen readers take it for that
		// control's name.
		let sections_label = StaticText::builder(parent).with_label(label).build();
		let sections_list = ListBox::builder(parent).build();
		let entries = sections
			.iter()
			.map(|&(section, title)| {
				let label = StaticText::builder(parent).with_label(title).build();
				Entry { section, title: title.to_owned(), label, list: Rc::new(QuietList::new(parent, title)) }
			})
			.collect();
		let first = sections[0].0;
		let inner = Rc::new(Inner {
			sections: sections_list,
			sections_label,
			entries,
			order: RefCell::new(Vec::new()),
			current: Cell::new(first),
			changed: RefCell::new(None),
		});
		let weak = Rc::downgrade(&inner);
		sections_list.on_selection_changed(move |event| {
			let Some(inner) = Weak::upgrade(&weak) else {
				return;
			};
			let picked = event
				.get_selection()
				.and_then(|index| usize::try_from(index).ok())
				.and_then(|index| inner.order.borrow().get(index).copied());
			if let Some(section) = picked {
				Self { inner }.choose(section);
			}
		});
		let lists = Self { inner };
		lists.set_order(&sections.iter().map(|&(section, _)| section).collect::<Vec<_>>());
		lists
	}

	/// The two columns, sections and items, for the parent's sizer. The items column is
	/// `items_weight` times as wide as the sections one.
	pub fn sizer(&self, items_weight: i32) -> BoxSizer {
		let inner = &self.inner;
		let padding = dialog_padding(&inner.sections);
		let sizer = BoxSizer::builder(Orientation::Horizontal).build();
		let sections_column = BoxSizer::builder(Orientation::Vertical).build();
		sections_column.add(&inner.sections_label, 0, SizerFlag::empty(), 0);
		sections_column.add(&inner.sections, 1, SizerFlag::Expand, 0);
		sizer.add_sizer(&sections_column, 1, SizerFlag::Expand | SizerFlag::All, padding);
		// Every section's list is here, but only the shown one takes up room.
		let items_column = BoxSizer::builder(Orientation::Vertical).build();
		for entry in &inner.entries {
			items_column.add(&entry.label, 0, SizerFlag::empty(), 0);
			items_column.add(&*entry.list, 1, SizerFlag::Expand, 0);
		}
		sizer.add_sizer(&items_column, items_weight, SizerFlag::Expand | SizerFlag::All, padding);
		sizer
	}

	/// Shows `hook` each key pressed in the Sections list before the list acts on it. It returns
	/// whether it handled the key, in which case the list leaves it alone.
	pub fn on_sections_key_down(&self, hook: impl Fn(&WindowEventData) -> bool + 'static) {
		self.inner.sections.on_key_down(move |event| {
			// Said either way: a key left unsaid still goes on to the list, which would move the
			// selection after the hook had moved the section.
			event.skip(!hook(&event));
		});
	}

	/// Runs `changed` with the section the user or [`set_order`](Self::set_order) moved to.
	pub fn on_section_changed(&self, changed: impl Fn(S) + 'static) {
		*self.inner.changed.borrow_mut() = Some(Rc::new(changed));
	}

	/// Shows `order` in the Sections list. The section on screen stays when it is still listed;
	/// otherwise the first listed takes its place.
	///
	/// # Panics
	///
	/// If `order` is empty, since the Sections list would have nothing to choose.
	pub fn set_order(&self, order: &[S]) {
		assert!(!order.is_empty(), "a sectioned list needs a section to show");
		let inner = &self.inner;
		inner.sections.clear();
		for section in order {
			inner.sections.append(&self.entry(*section).title);
		}
		*inner.order.borrow_mut() = order.to_vec();
		let current = inner.current.get();
		let index = order.iter().position(|section| *section == current).unwrap_or(0);
		inner.sections.set_selection(u32::try_from(index).unwrap_or_default(), true);
		if order[index] == current {
			self.show_only(current);
		} else {
			self.choose(order[index]);
		}
	}

	pub fn current(&self) -> S {
		self.inner.current.get()
	}

	/// `section`'s list. Its callbacks should hold it weakly, since the list holds them.
	///
	/// # Panics
	///
	/// If `section` wasn't one of those the controls were made with.
	pub fn list(&self, section: S) -> &Rc<QuietList<String>> {
		&self.entry(section).list
	}

	pub fn current_list(&self) -> &Rc<QuietList<String>> {
		self.list(self.current())
	}

	/// Every section with its list, for binding the same events on each.
	pub fn lists(&self) -> impl Iterator<Item = (S, &Rc<QuietList<String>>)> {
		self.inner.entries.iter().map(|entry| (entry.section, &entry.list))
	}

	pub fn focus_sections(&self) {
		self.inner.sections.set_focus();
	}

	fn entry(&self, section: S) -> &Entry<S> {
		self.inner.entries.iter().find(|entry| entry.section == section).expect("a section made with the controls")
	}

	/// Moves to `section` and tells whoever is listening.
	fn choose(&self, section: S) {
		self.inner.current.set(section);
		self.show_only(section);
		let changed = self.inner.changed.borrow().clone();
		if let Some(changed) = changed {
			changed(section);
		}
	}

	/// Shows `section`'s label and list and hides the others'. Focus in a list being hidden moves
	/// to the one shown, rather than staying on a control no one can see.
	fn show_only(&self, section: S) {
		let inner = &self.inner;
		inner.current.set(section);
		let had_focus = inner.entries.iter().any(|entry| entry.list.has_focus());
		for entry in &inner.entries {
			let shown = entry.section == section;
			entry.label.show(shown);
			entry.list.show(shown);
		}
		if let Some(parent) = inner.sections.get_parent() {
			parent.layout();
		}
		if had_focus {
			self.list(section).set_focus();
		}
	}
}
