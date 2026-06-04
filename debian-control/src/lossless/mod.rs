//! Lossless parser for various Debian control files
//!
//! This library provides a parser for various Debian control files, such as `control`, `changes`,
//! and apt `Release`, `Packages`, and `Sources` files. The parser is lossless, meaning that it
//! preserves all formatting as well as any possible errors in the files.
//!
//! # Example with positioned errors
//!
//! ```
//! use debian_control::lossless::{Control, PositionedParseError};
//!
//! let input = "Invalid: field\nBroken field without colon";
//! let parsed = Control::parse(input);
//!
//! // Access positioned errors for precise error reporting
//! for error in parsed.positioned_errors() {
//!     println!("Error at {:?}: {}", error.range, error.message);
//!     // Use error.range for IDE/language server integration
//! }
//! ```

pub mod apt;
pub mod buildinfo;
pub mod changes;
pub mod control;
pub mod relations;
pub use control::*;
pub use deb822_edit::{Parse, PositionedParseError};
pub use relations::*;
