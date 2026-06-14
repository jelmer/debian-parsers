Rust parser for Debian DEP-3 patch headers
------------------------------------------

# Examples

```rust
use dep3::PatchHeader;
use std::str::FromStr;
let text = r#"From: John Doe <john.doe@example>
Date: Mon, 1 Jan 2000 00:00:00 +0000
Subject: [PATCH] fix a bug
Bug-Debian: https://bugs.debian.org/123456
Bug: https://bugzilla.example.com/bug.cgi?id=123456
Forwarded: not-needed
"#;

let patch_header = PatchHeader::from_str(text).unwrap();
assert_eq!(patch_header.description(), Some("[PATCH] fix a bug".to_string()));
assert_eq!(patch_header.vendor_bugs("Debian").collect::<Vec<_>>(), vec!["https://bugs.debian.org/123456".to_string()]);
```

## Features

* `lossless` (default): round-trip preserving parser built on `deb822-lossless`.
* `patchkit`: parse the unified diff body of a `debian/patches/*` file into
  [`patchkit`](https://crates.io/crates/patchkit) structures alongside its DEP-3
  header, via the `dep3::patchkit` module.

See <https://dep-team.pages.debian.net/deps/dep3/> for details
