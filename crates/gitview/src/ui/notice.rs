//! Error and warning messages that also go in the log, so it holds what the user was told.

use std::fmt::Display;

use wx_utils::{show_error, show_warning};
use wxdragon::prelude::*;

pub fn error(parent: &dyn WxWidget, message: impl Display, title: &str) {
	tracing::error!("{title}: {message}");
	show_error(parent, message, title);
}

pub fn warning(parent: &dyn WxWidget, message: impl Display, title: &str) {
	tracing::warn!("{title}: {message}");
	show_warning(parent, message, title);
}
