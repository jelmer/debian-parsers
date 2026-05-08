#![deny(missing_docs)]
//! Formatting-preserving parser and editor for Debian watch files
//!
//! # Example
//!
//! ```rust,ignore
//! // For line-based formats (v1-4):
//! // Note: This example requires the "linebased" feature (enabled by default)
//! let wf = debian_watch::linebased::WatchFile::new(None);
//! assert_eq!(wf.version(), debian_watch::DEFAULT_VERSION);
//! assert_eq!("", wf.to_string());
//!
//! let wf = debian_watch::linebased::WatchFile::new(Some(4));
//! assert_eq!(wf.version(), 4);
//! assert_eq!("version=4\n", wf.to_string());
//!
//! let wf: debian_watch::linebased::WatchFile = r#"version=4
//! opts=foo=blah https://foo.com/bar .*/v?(\d\S+)\.tar\.gz
//! "#.parse().unwrap();
//! assert_eq!(wf.version(), 4);
//! assert_eq!(wf.entries().collect::<Vec<_>>().len(), 1);
//! let entry = wf.entries().next().unwrap();
//! assert_eq!(entry.opts(), maplit::hashmap! {
//!    "foo".to_string() => "blah".to_string(),
//! });
//! assert_eq!(&entry.url(), "https://foo.com/bar");
//! assert_eq!(entry.matching_pattern().as_deref(), Some(".*/v?(\\d\\S+)\\.tar\\.gz"));
//! ```

#[cfg(feature = "linebased")]
mod lex;
#[cfg(feature = "linebased")]
/// Line-based watch file format parser (versions 1-4)
pub mod linebased;

#[cfg(all(feature = "deb822", feature = "linebased"))]
mod convert;
#[cfg(feature = "deb822")]
pub mod deb822;
#[cfg(all(feature = "discover", any(feature = "linebased", feature = "deb822")))]
pub mod discover;
pub mod mangle;
#[cfg(feature = "pgp")]
pub mod pgp;
pub mod release;
pub mod search;
#[cfg(feature = "deb822")]
pub mod templates;

/// Any watch files without a version are assumed to be
/// version 1.
pub const DEFAULT_VERSION: u32 = 1;

#[cfg(any(feature = "linebased", feature = "deb822"))]
pub mod parse;
pub mod subst;
mod types;
/// Default user agent string used for HTTP requests
pub const DEFAULT_USER_AGENT: &str = concat!("debian-watch-rs/", env!("CARGO_PKG_VERSION"));

pub use release::Release;
pub use types::*;

/// Let's start with defining all kinds of tokens and
/// composite nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types, missing_docs, clippy::upper_case_acronyms)]
#[repr(u16)]
pub enum SyntaxKind {
    KEY = 0,
    VALUE,
    EQUALS,
    QUOTE,
    COMMA,
    CONTINUATION,
    NEWLINE,
    WHITESPACE, // whitespaces is explicit
    COMMENT,    // comments
    ERROR,      // as well as errors

    // composite nodes
    ROOT,             // The entire file
    VERSION,          // "version=x\n"
    ENTRY,            // "opts=foo=blah https://foo.com/bar .*/v?(\d\S+)\.tar\.gz\n"
    OPTS_LIST,        // "opts=foo=blah"
    OPTION,           // "foo=blah"
    OPTION_SEPARATOR, // "," (comma separator between options)
    URL,              // "https://foo.com/bar"
    MATCHING_PATTERN, // ".*/v?(\d\S+)\.tar\.gz"
    VERSION_POLICY,   // "debian"
    SCRIPT,           // "uupdate"
}

/// Convert our `SyntaxKind` into the rowan `SyntaxKind`.
impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

// Only export traits - specific implementations are in their modules
// Users access linebased types via debian_watch::linebased::WatchFile
// Users access deb822 types via debian_watch::deb822::WatchFile

#[cfg(all(feature = "deb822", feature = "linebased"))]
pub use crate::convert::{convert_to_v5, ConversionError};

#[cfg(all(test, feature = "linebased"))]
mod tests {
    use crate::linebased::WatchFile;

    #[test]
    fn test_create_watchfile() {
        let wf = WatchFile::new(None);
        assert_eq!(wf.version(), super::DEFAULT_VERSION);

        assert_eq!("", wf.to_string());

        let wf = WatchFile::new(Some(4));
        assert_eq!(wf.version(), 4);

        assert_eq!("version=4\n", wf.to_string());
    }

    #[test]
    fn test_set_version() {
        let mut wf = WatchFile::new(Some(4));
        assert_eq!(wf.version(), 4);

        wf.set_version(5);
        assert_eq!(wf.version(), 5);
        assert_eq!("version=5\n", wf.to_string());

        // Test setting version on a file without version
        let mut wf = WatchFile::new(None);
        assert_eq!(wf.version(), super::DEFAULT_VERSION);

        wf.set_version(4);
        assert_eq!(wf.version(), 4);
        assert_eq!("version=4\n", wf.to_string());
    }

    #[test]
    fn test_set_version_on_parsed() {
        // Test that parsed WatchFiles can be mutated
        let mut wf: WatchFile = "version=4\n".parse().unwrap();
        assert_eq!(wf.version(), 4);

        wf.set_version(5);
        assert_eq!(wf.version(), 5);
        assert_eq!("version=5\n", wf.to_string());

        // Test setting version on a parsed file without version
        let mut wf: WatchFile = "".parse().unwrap();
        assert_eq!(wf.version(), super::DEFAULT_VERSION);

        wf.set_version(4);
        assert_eq!(wf.version(), 4);
        assert_eq!("version=4\n", wf.to_string());
    }
}
