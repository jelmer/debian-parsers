Lossless parser for deb822 style files
======================================

# Example

```rust
use deb822_edit::Deb822;
use std::str::FromStr;

let input = r#"Package: deb822-edit
Maintainer: Jelmer Vernooĳ <jelmer@debian.org>
Section: rust

Package: deb822-edit
Architecture: any
Description: Lossless parser for deb822 style files.
  This parser can be used to parse files in the deb822 format, while preserving
  all whitespace and comments. It is based on the [rowan] library, which is a
  lossless parser library for Rust.
"#;

let deb822 = Deb822::from_str(input).unwrap();
assert_eq!(deb822.paragraphs().count(), 2);
```
