#![deny(missing_docs)]
//! A library for parsing and manipulating debian/copyright files that
//! use the DEP-5 format.
//!
//! # Examples
//!
//! ```rust
//!
//! use debian_copyright::Copyright;
//! use std::path::Path;
//!
//! let text = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
//! Upstream-Author: John Doe <john@example>
//! Upstream-Name: example
//! Source: https://example.com/example
//!
//! Files: *
//! License: GPL-3+
//! Copyright: 2019 John Doe
//!
//! Files: debian/*
//! License: GPL-3+
//! Copyright: 2019 Jane Packager
//!
//! License: GPL-3+
//!  This program is free software: you can redistribute it and/or modify
//!  it under the terms of the GNU General Public License as published by
//!  the Free Software Foundation, either version 3 of the License, or
//!  (at your option) any later version.
//! "#;
//!
//! let c = text.parse::<Copyright>().unwrap();
//! let license = c.find_license_for_file(Path::new("debian/foo")).unwrap();
//! assert_eq!(license.name(), Some("GPL-3+"));
//! ```
//!
//! See the ``edit`` module (behind the ``edit`` feature) for a more forgiving parser that
//! allows partial parsing, parsing files with errors and unknown fields and editing while
//! preserving formatting.

#[cfg(feature = "edit")]
pub mod edit;

#[cfg(feature = "lossless")]
#[deprecated(since = "0.1.28", note = "Use `edit` module instead")]
pub mod lossless {
    //! Deprecated: Use the `edit` module instead.
    pub use crate::edit::*;
}
pub mod lossy;
pub use lossy::Copyright;

/// The current version of the DEP-5 format.
pub const CURRENT_FORMAT: &str =
    "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";

/// The known versions of the DEP-5 format.
pub const KNOWN_FORMATS: &[&str] = &[CURRENT_FORMAT];

pub mod expression;
/// DEP-5 glob pattern matching.
pub mod glob;
pub use expression::LicenseExpr;
pub use glob::GlobPattern;

/// Decode deb822 paragraph markers in a multi-line field value.
///
/// According to Debian policy, blank lines in multi-line field values are
/// represented as lines containing only "." (a single period). The deb822
/// parser already strips the leading indentation whitespace from continuation lines,
/// so we only need to decode the period markers back to blank lines.
///
/// # Arguments
///
/// * `text` - The raw field value text from deb822 parser with indentation already stripped
///
/// # Returns
///
/// The decoded text with blank lines restored
fn decode_field_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line == "." {
                // Paragraph marker representing a blank line
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Encode blank lines in a field value to deb822 paragraph markers.
///
/// According to Debian policy, blank lines in multi-line field values must be
/// represented as lines containing only "." (a single period).
///
/// # Arguments
///
/// * `text` - The decoded text with normal blank lines
///
/// # Returns
///
/// The encoded text with blank lines replaced by "."
fn encode_field_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                // Blank line must be encoded as period marker
                "."
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A license, which can be just a name, a text or a named license.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum License {
    /// A license with just a name.
    Name(String),

    /// A license with just a text.
    Text(String),

    /// A license with a name and a text.
    Named(String, String),
}

impl License {
    /// Returns the name of the license, if any.
    ///
    /// Note that this may be a license expression containing multiple licenses
    /// combined with `or`, `and`, or `with`. Use [`License::expr`] to parse
    /// the expression into a structured [`LicenseExpr`].
    pub fn name(&self) -> Option<&str> {
        match self {
            License::Name(name) => Some(name),
            License::Text(_) => None,
            License::Named(name, _) => Some(name),
        }
    }

    /// Returns the text of the license, if any.
    pub fn text(&self) -> Option<&str> {
        match self {
            License::Name(_) => None,
            License::Text(text) => Some(text),
            License::Named(_, text) => Some(text),
        }
    }

    /// Parse the license name as a structured expression.
    ///
    /// Returns `None` if the license has no name (i.e. is a `Text` variant).
    ///
    /// # Examples
    ///
    /// ```
    /// use debian_copyright::{License, LicenseExpr};
    ///
    /// let license = License::Name("GPL-2+ or MIT".to_string());
    /// assert_eq!(
    ///     license.expr(),
    ///     Some(LicenseExpr::Or(vec![
    ///         LicenseExpr::Name("GPL-2+".to_string()),
    ///         LicenseExpr::Name("MIT".to_string()),
    ///     ])),
    /// );
    /// ```
    pub fn expr(&self) -> Option<LicenseExpr> {
        self.name().map(LicenseExpr::parse)
    }
}

impl std::str::FromStr for License {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Some((name, rest)) = text.split_once('\n') {
            let decoded_text = decode_field_text(rest);
            if name.is_empty() {
                Ok(License::Text(decoded_text))
            } else {
                Ok(License::Named(name.to_string(), decoded_text))
            }
        } else {
            Ok(License::Name(text.to_string()))
        }
    }
}

impl std::fmt::Display for License {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            License::Name(name) => f.write_str(name),
            License::Text(text) => write!(f, "\n{}", encode_field_text(text)),
            License::Named(name, text) => write!(f, "{}\n{}", name, encode_field_text(text)),
        }
    }
}

/// Calculate the depth of a Files pattern by counting '/' characters.
pub fn pattern_depth(pattern: &str) -> usize {
    pattern.matches('/').count()
}

/// Check if a pattern is a debian/* pattern (should be sorted last by convention).
pub fn is_debian_pattern(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    trimmed.starts_with("debian/") || trimmed == "debian/*"
}

/// Calculate a sort key for a Files pattern.
///
/// Returns `(priority, depth)` where:
/// - priority 0: `*` (always first)
/// - priority 1: normal patterns (sorted by depth)
/// - priority 2: debian/* patterns (always last, then by depth)
///
/// This follows the Debian convention that the `*` wildcard should be first,
/// and `debian/*` patterns should be last in debian/copyright Files paragraphs.
pub fn pattern_sort_key(pattern: &str, depth: usize) -> (u8, usize) {
    let trimmed = pattern.trim();

    if trimmed == "*" {
        (0, 0)
    } else if is_debian_pattern(pattern) {
        (2, depth)
    } else {
        (1, depth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_field_text() {
        // Test basic decoding of period markers
        let input = "line 1\n.\nline 3";
        let output = decode_field_text(input);
        assert_eq!(output, "line 1\n\nline 3");
    }

    #[test]
    fn test_decode_field_text_no_markers() {
        // Test text without markers remains unchanged
        let input = "line 1\nline 2\nline 3";
        let output = decode_field_text(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_license_from_str_with_paragraph_markers() {
        // Test that License::from_str decodes paragraph markers
        let input = "GPL-3+\nThis is line 1\n.\nThis is line 3";
        let license: License = input.parse().unwrap();

        match license {
            License::Named(name, text) => {
                assert_eq!(name, "GPL-3+");
                assert_eq!(text, "This is line 1\n\nThis is line 3");
                assert!(!text.contains("\n.\n"));
            }
            _ => panic!("Expected Named license"),
        }
    }

    #[test]
    fn test_encode_field_text() {
        // Test basic encoding of blank lines
        let input = "line 1\n\nline 3";
        let output = encode_field_text(input);
        assert_eq!(output, "line 1\n.\nline 3");
    }

    #[test]
    fn test_encode_decode_round_trip() {
        // Test that encoding and decoding are inverse operations
        let original = "First paragraph\n\nSecond paragraph\n\nThird paragraph";
        let encoded = encode_field_text(original);
        let decoded = decode_field_text(&encoded);
        assert_eq!(
            decoded, original,
            "Round-trip encoding/decoding should preserve text"
        );
    }

    #[test]
    fn test_license_display_encodes_blank_lines() {
        // Test that License::Display encodes blank lines
        let license = License::Named("MIT".to_string(), "Line 1\n\nLine 2".to_string());
        let displayed = license.to_string();
        assert_eq!(displayed, "MIT\nLine 1\n.\nLine 2");
        assert!(displayed.contains("\n.\n"), "Should contain period marker");
        assert_eq!(
            displayed.matches("\n\n").count(),
            0,
            "Should not contain literal blank lines"
        );
    }

    #[test]
    fn test_pattern_depth() {
        assert_eq!(pattern_depth("*"), 0);
        assert_eq!(pattern_depth("src/*"), 1);
        assert_eq!(pattern_depth("src/foo/*"), 2);
        assert_eq!(pattern_depth("a/b/c/d/*"), 4);
        assert_eq!(pattern_depth("debian/*"), 1);
    }

    #[test]
    fn test_is_debian_pattern() {
        assert!(is_debian_pattern("debian/*"));
        assert!(is_debian_pattern("debian/patches/*"));
        assert!(is_debian_pattern(" debian/* "));
        assert!(!is_debian_pattern("*"));
        assert!(!is_debian_pattern("src/*"));
        assert!(!is_debian_pattern("src/debian/*"));
    }

    #[test]
    fn test_pattern_sort_key() {
        // Test wildcard pattern (priority 0)
        assert_eq!(pattern_sort_key("*", 0), (0, 0));
        assert_eq!(pattern_sort_key(" * ", 0), (0, 0));

        // Test normal patterns (priority 1)
        assert_eq!(pattern_sort_key("src/*", 1), (1, 1));
        assert_eq!(pattern_sort_key("src/foo/*", 2), (1, 2));
        assert_eq!(pattern_sort_key("tests/*", 1), (1, 1));

        // Test debian patterns (priority 2)
        assert_eq!(pattern_sort_key("debian/*", 1), (2, 1));
        assert_eq!(pattern_sort_key("debian/patches/*", 2), (2, 2));
    }

    #[test]
    fn test_pattern_sort_key_ordering() {
        // Wildcard comes first
        assert!(pattern_sort_key("*", 0) < pattern_sort_key("src/*", 1));
        assert!(pattern_sort_key("*", 0) < pattern_sort_key("debian/*", 1));

        // Normal patterns come before debian patterns
        assert!(pattern_sort_key("src/*", 1) < pattern_sort_key("debian/*", 1));
        assert!(pattern_sort_key("tests/*", 1) < pattern_sort_key("debian/*", 1));

        // Within same priority, shallower comes before deeper
        assert!(pattern_sort_key("src/*", 1) < pattern_sort_key("src/foo/*", 2));
        assert!(pattern_sort_key("debian/*", 1) < pattern_sort_key("debian/patches/*", 2));

        // Debian patterns come last even with same depth as normal patterns
        assert!(pattern_sort_key("src/*", 1) < pattern_sort_key("debian/*", 1));
    }
}
