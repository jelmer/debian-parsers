# debian version handling in rust

This simple crate provides a struct for parsing, validating, manipulating and
comparing Debian version strings.

It aims to follow the version specification as described in Debian policy
5.6.12 and consistent with the behaviour of ``dpkg``.

Example:

```rust
use debversion::Version;

let version: Version = "1.0-1".parse().unwrap();
assert_eq!(version.epoch, None);
assert_eq!(version.upstream_version, "1.0");
assert_eq!(version.debian_revision.as_deref(), Some("1"));

let version1: Version = "1.0-0".parse().unwrap();
let version2: Version = "1.0".parse().unwrap();
assert_eq!(version1, version2);

let version1: Version = "1.0-1".parse().unwrap();
let version2: Version = "1.0~alpha1-1".parse().unwrap();
assert!(version2 < version1);
```

## Features

### sqlx

The `sqlx` feature adds serialization support for the postgres
[debversion extension](https://pgxn.org/dist/debversion/) when using sqlx.

### python-debian

The `python-debian` feature provides conversion support between the debversion
Rust type and the ``Version`` class provided by ``python-debian``, when using
[pyo3](https://github.com/pyo3/pyo3).

### serde

The `serde` feature enables serialization to and from simple strings when
using serde.
