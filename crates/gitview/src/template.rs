//! The templates each kind of row is shown with, which the user can change in Settings, as in
//! Fedra. They're written in minijinja's syntax, and the README lists what each one can say.

use std::{cell::RefCell, collections::BTreeMap};

use minijinja::{AutoEscape, Environment, Error, Value};
use serde::{Deserialize, Serialize};

/// A kind of row, in the main window and the dialogs alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	Notification,
	Repository,
	Issue,
	Commit,
	Person,
	Post,
}

impl Kind {
	/// Every kind, in the order Settings lists them.
	pub const ALL: [Self; 6] =
		[Self::Notification, Self::Repository, Self::Issue, Self::Commit, Self::Person, Self::Post];

	/// What the settings file calls its template.
	const fn id(self) -> &'static str {
		match self {
			Self::Notification => "notification",
			Self::Repository => "repository",
			Self::Issue => "issue",
			Self::Commit => "commit",
			Self::Person => "person",
			Self::Post => "post",
		}
	}

	/// What Settings calls it.
	pub const fn title(self) -> &'static str {
		match self {
			Self::Notification => "Notifications",
			Self::Repository => "Repositories",
			Self::Issue => "Issues and pull requests",
			Self::Commit => "Commits",
			Self::Person => "People",
			Self::Post => "Posts in a thread",
		}
	}

	/// The template it has until the user changes it.
	pub const fn default_template(self) -> &'static str {
		match self {
			Self::Notification => "{{ repository }}: {{ title }}. {{ kind }}, {{ reason }}",
			Self::Repository => {
				"{{ name }}{% if description %}: {{ description }}{% endif %}{% if private %}, private{% endif %}{% if \
				 fork %}, fork{% endif %}{% if archived %}, archived{% endif %}"
			}
			// A repository's own dialog leaves out the repository, and the kind its section says.
			Self::Issue => {
				"{% if repository %}{{ repository }}: #{{ number }} {{ title }}, {{ kind }} by {% else %}#{{ number }} \
				 {{ title }}, by {% endif %}{{ author }}, {{ comments }}{% if ending %}, {{ ending }}{% endif %}"
			}
			// Checks that passed go unsaid, or every commit of a repository with CI would say so.
			Self::Commit => concat!(
				"{{ summary }}, by {{ author }}, {{ time }}, {{ sha }}",
				"{% if checks and checks != \"passed\" %}, checks {{ checks }}{% endif %}",
			),
			Self::Person => "{{ login }}{% if organization %}, organization{% endif %}",
			Self::Post => {
				"{% if opening %}{{ author }} opened this {{ kind }} on {{ time }}{% else %}{{ author }} commented on {{ \
				 time }}{% endif %}:\n{{ text }}"
			}
		}
	}
}

/// The templates the user has changed, by kind. One left as it was isn't kept, so it follows the
/// default if that improves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Templates(BTreeMap<String, String>);

impl Templates {
	/// `kind`'s template: the user's, or the default when they haven't changed it.
	pub fn get(&self, kind: Kind) -> &str {
		self.0.get(kind.id()).map_or_else(|| kind.default_template(), String::as_str)
	}

	/// Gives `kind` `template`. An empty one, or the default, goes back to the default.
	pub fn set(&mut self, kind: Kind, template: &str) {
		if template.trim().is_empty() || template == kind.default_template() {
			self.0.remove(kind.id());
		} else {
			self.0.insert(kind.id().to_owned(), template.to_owned());
		}
	}
}

thread_local! {
	/// The templates rows are shown with. Rows are only ever labelled on the UI thread.
	static CURRENT: RefCell<Templates> = RefCell::default();
	/// Compiling a template costs far more than rendering one, and a list renders every row each
	/// time it fills, so each template is compiled once, keyed by its text.
	static COMPILED: RefCell<Environment<'static>> = RefCell::new({
		let mut environment = Environment::new();
		// Rows are plain text, never HTML.
		environment.set_auto_escape_callback(|_| AutoEscape::None);
		environment
	});
}

/// Shows rows with `templates` from now on.
pub fn use_templates(templates: Templates) {
	CURRENT.set(templates);
}

/// A row of `kind`, with what `context` says about its item. A template that fails falls back to
/// the default, so a mistake can't empty a list.
pub fn render(kind: Kind, context: &Value) -> String {
	let template = CURRENT.with_borrow(|templates| templates.get(kind).to_owned());
	render_source(&template, context).or_else(|_| render_source(kind.default_template(), context)).unwrap_or_default()
}

/// What's wrong with `template`, when it can't be read as a template.
pub fn mistake(template: &str) -> Option<String> {
	Environment::new().template_from_str(template).err().map(|error| error.to_string())
}

fn render_source(source: &str, context: &Value) -> Result<String, Error> {
	COMPILED.with_borrow_mut(|environment| {
		if environment.get_template(source).is_err() {
			environment.add_template_owned(source.to_owned(), source.to_owned())?;
		}
		environment.get_template(source)?.render(context)
	})
}

#[cfg(test)]
mod tests {
	use minijinja::context;

	use super::{Kind, Templates, mistake, render, use_templates};

	#[test]
	fn the_defaults_read_as_rows_always_have() {
		use_templates(Templates::default());
		let repository =
			context! { name => "trypsynth/gitview", description => "A GitHub client", private => false, fork => true };
		assert_eq!(render(Kind::Repository, &repository), "trypsynth/gitview: A GitHub client, fork");
		let searched = context! {
			repository => "trypsynth/gitview", number => 4, title => "See commits", kind => "issue",
			author => "masonasons", comments => "2 comments", ending => "",
		};
		assert_eq!(
			render(Kind::Issue, &searched),
			"trypsynth/gitview: #4 See commits, issue by masonasons, 2 comments"
		);
		let in_dialog = context! {
			repository => "", number => 4, title => "See commits", kind => "issue", author => "masonasons",
			comments => "2 comments", ending => "completed",
		};
		assert_eq!(render(Kind::Issue, &in_dialog), "#4 See commits, by masonasons, 2 comments, completed");
		let post =
			context! { opening => false, author => "trypsynth", time => "Nov 17, 2026 at 1:00 PM", text => "Done." };
		assert_eq!(render(Kind::Post, &post), "trypsynth commented on Nov 17, 2026 at 1:00 PM:\nDone.");
	}

	#[test]
	fn a_commit_only_mentions_checks_that_have_not_passed() {
		use_templates(Templates::default());
		let commit = |checks: &str| {
			context! { summary => "Fix it", author => "trypsynth", time => "now", sha => "de94f2a", checks }
		};
		assert_eq!(render(Kind::Commit, &commit("")), "Fix it, by trypsynth, now, de94f2a");
		assert_eq!(render(Kind::Commit, &commit("passed")), "Fix it, by trypsynth, now, de94f2a");
		assert_eq!(render(Kind::Commit, &commit("failed")), "Fix it, by trypsynth, now, de94f2a, checks failed");
		assert_eq!(render(Kind::Commit, &commit("running")), "Fix it, by trypsynth, now, de94f2a, checks running");
	}

	#[test]
	fn a_changed_template_is_used_and_a_broken_one_falls_back() {
		let mut templates = Templates::default();
		templates.set(Kind::Person, "{{ name }} ({{ login }})");
		use_templates(templates.clone());
		let person = context! { login => "trypsynth", name => "Quin", organization => false };
		assert_eq!(render(Kind::Person, &person), "Quin (trypsynth)");
		templates.set(Kind::Person, "{{ login ");
		use_templates(templates);
		assert_eq!(render(Kind::Person, &person), "trypsynth");
		use_templates(Templates::default());
	}

	#[test]
	fn only_changed_templates_are_kept() {
		let mut templates = Templates::default();
		templates.set(Kind::Commit, Kind::Commit.default_template());
		templates.set(Kind::Person, "  ");
		assert_eq!(templates, Templates::default());
		assert!(mistake("{{ login ").is_some());
		assert!(mistake(Kind::Issue.default_template()).is_none());
	}
}
