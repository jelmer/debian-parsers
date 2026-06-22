#![deny(missing_docs)]
// Until we drop support for PyO3 0.22, allow use of deprecated functions.
#![allow(deprecated)]

//! Parser for Debian control files.
//!
//! This crate provides a parser for Debian control files.
//!
//! # Example
//!
//! ```rust
//! use debian_control::lossy::Control;
//! use debian_control::fields::Priority;
//! use std::fs::File;
//!
//! let mut control = Control::new();
//! let mut source = &mut control.source;
//! source.name = "hello".to_string();
//! source.section = Some("rust".to_string());
//!
//! let mut binary = control.add_binary("hello");
//! binary.architecture = Some("amd64".to_string());
//! binary.priority = Some(Priority::Optional);
//! binary.description = Some("Hello, world!".to_string());
//!
//! assert_eq!(control.to_string(), r#"Source: hello
//! Section: rust
//!
//! Package: hello
//! Architecture: amd64
//! Priority: optional
//! Description: Hello, world!
//! "#);
//! ```
//!
//! See the ``edit`` module for a parser that preserves all comments and formatting, and
//! as well as allowing inline errors.
pub mod lossy;
#[cfg(feature = "edit")]
pub use edit::control::{Binary, Control, Source};
pub mod fields;
pub use fields::*;
#[cfg(feature = "edit")]
pub mod edit;
#[cfg(feature = "edit")]
pub use edit::apt;
#[cfg(feature = "edit")]
pub use edit::changes;
#[cfg(feature = "edit")]
pub use edit::control;

#[cfg(feature = "lossless")]
#[deprecated(since = "0.1.49", note = "Use `edit` module instead")]
pub mod lossless {
    //! Deprecated: Use the `edit` module instead.
    //!
    //! This module is a compatibility alias for the `edit` module and will be removed in a future version.
    pub use crate::edit::*;
}
#[cfg(feature = "lossless")]
pub mod pgp;
pub mod relations;
pub mod vcs;

use std::borrow::Cow;

/// Error type for parsing an identity string.
#[derive(Debug, PartialEq)]
pub enum ParseIdentityError {
    /// No email address found.
    NoEmail,
}

impl std::fmt::Display for ParseIdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ParseIdentityError::NoEmail => write!(f, "No email found"),
        }
    }
}

impl std::error::Error for ParseIdentityError {}

/// Parse an identity string into a name and an email address.
///
/// The input string should be in the format `Name <email>`. If the email is missing, an error is
/// returned.
///
/// # Example
/// ```
/// use debian_control::parse_identity;
/// assert_eq!(parse_identity("Joe Example <joe@example.com>"), Ok(("Joe Example", "joe@example.com")));
/// ```
///
/// # Arguments
/// * `s` - The input string.
///
/// # Returns
/// A tuple with the name and the email address.
pub fn parse_identity(s: &str) -> Result<(&str, &str), ParseIdentityError> {
    // Support Name <email> and email, but ensure email contains an "@".
    if let Some((name, email)) = s.split_once('<') {
        if let Some(email) = email.strip_suffix('>') {
            Ok((name.trim(), email.trim()))
        } else {
            Err(ParseIdentityError::NoEmail)
        }
    } else if s.contains('@') {
        Ok(("", s.trim()))
    } else {
        Err(ParseIdentityError::NoEmail)
    }
}

/// A trait for looking up versions of packages.
pub trait VersionLookup {
    /// Look up the version of a package.
    fn lookup_version<'a>(
        &'a self,
        package: &'_ str,
    ) -> Option<std::borrow::Cow<'a, debversion::Version>>;
}

impl VersionLookup for std::collections::HashMap<String, debversion::Version> {
    fn lookup_version<'a>(&'a self, package: &str) -> Option<Cow<'a, debversion::Version>> {
        self.get(package).map(Cow::Borrowed)
    }
}

impl<F> VersionLookup for F
where
    F: Fn(&str) -> Option<debversion::Version>,
{
    fn lookup_version<'a>(&'a self, name: &str) -> Option<Cow<'a, debversion::Version>> {
        self(name).map(Cow::Owned)
    }
}

impl VersionLookup for (String, debversion::Version) {
    fn lookup_version<'a>(&'a self, name: &str) -> Option<Cow<'a, debversion::Version>> {
        if name == self.0 {
            Some(Cow::Borrowed(&self.1))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_identity() {
        assert_eq!(
            parse_identity("Joe Example <joe@example.com>"),
            Ok(("Joe Example", "joe@example.com"))
        );
        assert_eq!(
            parse_identity("joe@example.com"),
            Ok(("", "joe@example.com"))
        );
        assert_eq!(parse_identity("somebody"), Err(ParseIdentityError::NoEmail));
    }
}
