Debian Changelog parser
=======================

This crate provides a parser for debian/changelog files, as described in the
Debian policy,
[section 4.4](https://www.debian.org/doc/debian-policy/ch-source.html#debian-changelog-debian-changelog).

The parser builds a CST. It is lossless - i.e. preserves formatting, and allows
editing and partial parsing.

Example:

```rust

use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open("/usr/share/doc/rustc/changelog.Debian.gz")?;
    let mut gz = flate2::read::GzDecoder::new(file);
    let mut contents = String::new();
    gz.read_to_string(&mut contents)?;
    let changelog: debian_changelog::ChangeLog = contents.parse()?;
    for entry in changelog.entries() {
        println!(
            "{}: {}",
            entry.package().unwrap(),
            entry.version().unwrap().to_string()
        );
    }
    Ok(())
}
```

Or to update an existing changelog file:

```rust

use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open("debian/changelog")?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut changelog: debian_changelog::ChangeLog = contents.parse()?;
    changelog.try_auto_add_change(
        &["* Make a change"],
        (
            "Jelmer Vernooĳ".to_string(),
            "jelmer@debian.org".to_string(),
        ),
        None,
        None,
    )?;
    std::fs::write("debian/changelog", changelog.to_string())?;
    Ok(())
}
```
