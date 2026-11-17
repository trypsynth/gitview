#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("GitHub no longer accepts the saved sign-in")]
	Unauthorized,
	#[error("the sign-in code expired, try again")]
	CodeExpired,
	#[error("sign-in was denied on GitHub")]
	AccessDenied,
	#[error("the sign-in does not allow editing your profile")]
	NeedsProfileAccess,
	#[error("sign-in was cancelled")]
	Cancelled,
	#[error("that link does not point at the GitHub API")]
	NotGitHub,
	#[error("sign-in failed: {0}")]
	Auth(String),
	#[error(transparent)]
	Http(ureq::Error),
	/// GitHub's GraphQL API answered, but with an error instead of what was asked for.
	#[error("{0}")]
	Graph(String),
}

impl Error {
	/// Whether GitHub said what was asked for doesn't exist, or isn't one the sign-in can see.
	#[must_use]
	pub const fn is_not_found(&self) -> bool {
		matches!(self, Self::Http(ureq::Error::StatusCode(404)))
	}
}

impl From<ureq::Error> for Error {
	fn from(error: ureq::Error) -> Self {
		match error {
			ureq::Error::StatusCode(401) => Self::Unauthorized,
			other => Self::Http(other),
		}
	}
}
