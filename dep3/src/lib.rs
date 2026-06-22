#![deny(missing_docs)]
//! A library for parsing and generating Debian patch headers.
//!
//! # Examples
//!
//! ```rust
//! use dep3::PatchHeader;
//! use std::str::FromStr;
//! let text = r#"From: John Doe <john.doe@example>
//! Date: Mon, 1 Jan 2000 00:00:00 +0000
//! Subject: [PATCH] fix a bug
//! Bug-Debian: https://bugs.debian.org/123456
//! Bug: https://bugzilla.example.com/bug.cgi?id=123456
//! Forwarded: not-needed
//! "#;
//!
//! let patch_header = PatchHeader::from_str(text).unwrap();
//! assert_eq!(patch_header.description, Some("[PATCH] fix a bug".to_string()));
//! assert_eq!(patch_header.vendor_bugs("Debian"), Some("https://bugs.debian.org/123456"));
//! ```
mod fields;
pub use fields::*;
#[cfg(feature = "edit")]
pub mod edit;

#[cfg(feature = "lossless")]
#[deprecated(since = "0.1.29", note = "Use `edit` module instead")]
pub mod lossless {
    //! Deprecated: Use the `edit` module instead.
    pub use crate::edit::*;
}
pub mod lossy;
#[cfg(feature = "patchkit")]
pub mod patchkit;

pub use lossy::PatchHeader;

/// Find the byte offset where the DEP-3 header in `content` ends, i.e.
/// the start of the first `---` / `diff ` / `Index:` line. Returns
/// `content.len()` if the file is header-only (no diff body).
///
/// This lets callers split a complete patch file into its header (a
/// deb822 paragraph) and its unified diff (which DEP-3 leaves
/// unspecified). Both [`lossless::PatchHeader::parse_relaxed`] and the
/// lossy [`PatchHeader`] use this internally; it's exposed for callers
/// that need to map source ranges back into the original file.
pub fn header_end(content: &str) -> usize {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.starts_with("---")
            || trimmed.starts_with("diff ")
            || trimmed.starts_with("Index:")
        {
            return offset;
        }
        offset += line.len();
    }
    content.len()
}
