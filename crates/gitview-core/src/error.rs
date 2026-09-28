#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("GitHub no longer accepts the saved sign-in")]
	Unauthorized,
	#[error("the sign-in code expired, try again")]
	CodeExpired,
	#[error("sign-in was denied on GitHub")]
	AccessDenied,
	#[error("sign-in was cancelled")]
	Cancelled,
	#[error("that link does not point at the GitHub API")]
	NotGitHub,
	#[error("sign-in failed: {0}")]
	Auth(String),
	#[error(transparent)]
	Http(ureq::Error),
}

impl From<ureq::Error> for Error {
	fn from(error: ureq::Error) -> Self {
		match error {
			ureq::Error::StatusCode(401) => Self::Unauthorized,
			other => Self::Http(other),
		}
	}
}
