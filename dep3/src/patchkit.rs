//! Parse the unified diff body of a `debian/patches/*` file into
//! [`patchkit`] structures.
//!
//! DEP-3 only specifies the header of a patch file; everything after it is an
//! ordinary (possibly multi-file) unified diff. This module splits a complete
//! patch file at [`crate::header_end`] and hands the remaining diff body to
//! [`patchkit::unified::parse_patches`], so callers get both the parsed DEP-3
//! header and the structured diff in one step.
//!
//! It is gated behind the `patchkit` feature.
//!
//! # Examples
//!
//! ```rust
//! let content = "Author: alice\n\
//!                Description: fix a thing\n\
//!                --- a/foo\n\
//!                +++ b/foo\n\
//!                @@ -1 +1 @@\n\
//!                -old\n\
//!                +new\n";
//! let (header, patches) = dep3::patchkit::parse_with_diff(content).unwrap();
//! assert_eq!(header.author(), Some("alice".to_string()));
//! assert_eq!(patches.len(), 1);
//! ```

use patchkit::unified::{parse_patches, PlainOrBinaryPatch};

/// An error returned while parsing a patch file into a header and a diff body.
#[derive(Debug)]
pub enum Error {
    /// The DEP-3 header could not be parsed as a deb822 paragraph.
    #[cfg(feature = "lossless")]
    Header(deb822_lossless::ParseError),
    /// The unified diff body could not be parsed.
    Diff(patchkit::unified::Error),
}

#[cfg(feature = "lossless")]
impl From<deb822_lossless::ParseError> for Error {
    fn from(e: deb822_lossless::ParseError) -> Self {
        Error::Header(e)
    }
}

impl From<patchkit::unified::Error> for Error {
    fn from(e: patchkit::unified::Error) -> Self {
        Error::Diff(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "lossless")]
            Error::Header(e) => write!(f, "header: {}", e),
            Error::Diff(e) => write!(f, "diff: {}", e),
        }
    }
}

impl std::error::Error for Error {}

/// Parse the unified diff body that follows a DEP-3 header.
///
/// `content` is the whole patch file. The header is split off at
/// [`crate::header_end`] and discarded; only the diff body is parsed. Returns
/// the patches it contains, which may be empty when the file is header-only.
pub fn parse_diff(content: &str) -> Result<Vec<PlainOrBinaryPatch>, patchkit::unified::Error> {
    let end = crate::header_end(content);
    let body = &content[end..];
    parse_patches(body.split_inclusive('\n').map(|l| l.as_bytes().to_vec())).collect()
}

/// Parse a complete `debian/patches/*` file into its DEP-3 header and the
/// patchkit-parsed diff body.
///
/// Requires both the `lossless` and `patchkit` features.
#[cfg(feature = "lossless")]
pub fn parse_with_diff(
    content: &str,
) -> Result<(crate::lossless::PatchHeader, Vec<PlainOrBinaryPatch>), Error> {
    let (header, _) = crate::lossless::PatchHeader::parse_relaxed(content)?;
    let patches = parse_diff(content)?;
    Ok((header, patches))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "Author: alice\n\
        Description: fix a thing\n\
        --- a/foo\n\
        +++ b/foo\n\
        @@ -1 +1 @@\n\
        -old\n\
        +new\n";

    #[test]
    fn parse_diff_returns_single_patch() {
        let patches = parse_diff(PATCH).unwrap();
        assert_eq!(patches.len(), 1);
        match &patches[0] {
            PlainOrBinaryPatch::Plain(p) => {
                assert_eq!(p.orig_name, b"a/foo");
                assert_eq!(p.mod_name, b"b/foo");
                assert_eq!(p.hunks.len(), 1);
            }
            PlainOrBinaryPatch::Binary(_) => panic!("expected a plain patch"),
        }
    }

    #[test]
    fn parse_diff_header_only_is_empty() {
        let patches = parse_diff("Author: alice\nDescription: bla\n").unwrap();
        assert!(patches.is_empty());
    }

    #[test]
    fn parse_diff_multiple_files() {
        let content = "Description: touch two files\n\
            --- a/foo\n\
            +++ b/foo\n\
            @@ -1 +1 @@\n\
            -a\n\
            +b\n\
            --- a/bar\n\
            +++ b/bar\n\
            @@ -1 +1 @@\n\
            -c\n\
            +d\n";
        let patches = parse_diff(content).unwrap();
        assert_eq!(patches.len(), 2);
    }

    #[cfg(feature = "lossless")]
    #[test]
    fn parse_with_diff_returns_header_and_patches() {
        let (header, patches) = parse_with_diff(PATCH).unwrap();
        assert_eq!(header.author(), Some("alice".to_string()));
        assert_eq!(header.description(), Some("fix a thing".to_string()));
        assert_eq!(patches.len(), 1);
    }
}
