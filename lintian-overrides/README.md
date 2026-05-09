lintian-overrides
=================

A format-preserving parser for Debian lintian override files, built on
[rowan](https://crates.io/crates/rowan).

## Usage

```rust
use lintian_overrides::LintianOverrides;

let text = "package-name: some-tag some extra info\n";
let parsed = LintianOverrides::parse(text);
let overrides = parsed.ok().unwrap();

for line in overrides.lines() {
    if let Some(tag) = line.tag() {
        println!("Tag: {}", tag.text());
    }
    if let Some(info) = line.info() {
        println!("Info: {}", info);
    }
}

// Round-trips perfectly
assert_eq!(overrides.text(), text);
```
