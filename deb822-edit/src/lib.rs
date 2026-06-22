#![deny(missing_docs)]
#![allow(clippy::type_complexity)]
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]
// Until we drop support for PyO3 0.22, allow use of deprecated functions.
#![allow(deprecated)]

mod common;
mod lex;
mod lossless;
mod parse;
pub use lex::SyntaxKind;
pub use lossless::{
    Deb822, Entry, Error, IndentPattern, Lang, Paragraph, ParseError, PositionedParseError,
};
pub use parse::Parse;
pub use rowan::{TextRange, TextSize};

/// The indentation to use when writing a deb822 file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indentation {
    /// Use the same indentation as the original line for the value.
    FieldNameLength,

    /// The number of spaces to use for indentation.
    Spaces(u32),
}

impl Default for Indentation {
    fn default() -> Self {
        Indentation::Spaces(4)
    }
}

#[cfg(feature = "deb822-fast")]
impl deb822_fast::convert::Deb822LikeParagraph for crate::lossless::Paragraph {
    fn get(&self, key: &str) -> Option<String> {
        crate::lossless::Paragraph::get(self, key).map(|v| v.to_string())
    }

    fn set(&mut self, key: &str, value: &str) {
        crate::lossless::Paragraph::set(self, key, value);
    }

    fn remove(&mut self, key: &str) {
        crate::lossless::Paragraph::remove(self, key);
    }
}
