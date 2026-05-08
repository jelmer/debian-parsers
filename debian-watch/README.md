Format-preserving parser and editor for Debian watch files
==========================================================

This crate supports reading, editing and writing Debian watch files,
while preserving the original contents byte-for-byte.

Example:

```rust
let wf = debian_watch::WatchFile::new(None);
assert_eq!(wf.version(), debian_watch::DEFAULT_VERSION);
assert_eq!("", wf.to_string());

let wf = debian_watch::WatchFile::new(Some(4));
assert_eq!(wf.version(), 4);
assert_eq!("version=4\n", wf.to_string());

let wf: debian_watch::WatchFile = r#"version=4
opts=foo=blah https://foo.com/bar .*/v?(\d\S+)\.tar\.gz
"#.parse().unwrap();

assert_eq!(wf.version(), 4);
assert_eq!(wf.entries().collect::<Vec<_>>().len(), 1);

let entry = wf.entries().next().unwrap();
assert_eq!(entry.opts(), maplit::hashmap! {
   "foo".to_string() => "blah".to_string(),
});
assert_eq!(&entry.url(), "https://foo.com/bar");
assert_eq!(entry.matching_pattern().as_deref(), Some(".*/v?(\\d\\S+)\\.tar\\.gz"));
```

It also supports partial parsing (with some error nodes), which could be useful
for e.g. IDEs.
