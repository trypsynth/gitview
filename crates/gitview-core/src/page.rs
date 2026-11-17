//! Lists that GitHub hands out a page at a time.

use std::marker::PhantomData;

/// One page of a list, and where the next one is when there is one.
#[derive(Debug)]
pub struct Page<T> {
	pub items: Vec<T>,
	pub next: Option<Cursor<T>>,
}

impl<T> Page<T> {
	#[must_use]
	pub const fn empty() -> Self {
		Self { items: Vec::new(), next: None }
	}
}

/// Where the next page of a list of `T` is. Only [`Client::next_page`](crate::Client::next_page)
/// follows it, so a cursor for one kind of list can't load another.
#[derive(Debug)]
pub struct Cursor<T> {
	/// The page's path under the API, never a whole URL, so the token can't travel elsewhere.
	pub(crate) path: String,
	pub(crate) kind: Kind,
	items: PhantomData<fn() -> T>,
}

impl<T> Cursor<T> {
	pub(crate) const fn new(path: String, kind: Kind) -> Self {
		Self { path, kind, items: PhantomData }
	}
}

impl<T> Clone for Cursor<T> {
	fn clone(&self) -> Self {
		Self::new(self.path.clone(), self.kind)
	}
}

/// How a list's pages are asked for and read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	/// A plain JSON array.
	Plain,
	/// A JSON array whose bodies come rendered to HTML as well.
	Rendered,
	/// The search endpoint's results, which wrap the array.
	Search,
}

/// The next page's URL in a `Link` header, such as
/// `<https://api.github.com/user/starred?page=2>; rel="next", <...>; rel="last"`.
pub fn next_link(header: &str) -> Option<&str> {
	header.split(',').find_map(|link| {
		let (url, params) = link.split_once(';')?;
		params
			.split(';')
			.any(|param| param.trim() == "rel=\"next\"")
			.then(|| url.trim().trim_start_matches('<').trim_end_matches('>'))
	})
}

#[cfg(test)]
mod tests {
	use super::next_link;

	#[test]
	fn the_next_link_is_found_among_the_others() {
		let header = "<https://api.github.com/user/starred?per_page=100&page=2>; rel=\"next\", \
		              <https://api.github.com/user/starred?per_page=100&page=5>; rel=\"last\"";
		assert_eq!(next_link(header), Some("https://api.github.com/user/starred?per_page=100&page=2"));
		let last_page = "<https://api.github.com/user/starred?per_page=100&page=1>; rel=\"prev\", \
		                 <https://api.github.com/user/starred?per_page=100&page=1>; rel=\"first\"";
		assert_eq!(next_link(last_page), None);
		assert_eq!(next_link(""), None);
	}
}
