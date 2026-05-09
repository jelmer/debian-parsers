# Rust parsers and editors for Debian packaging files

This repository contains parsers and editors for various Debian packaging
files, implement in Rust.

## Deb822-based parsers

The core two deb822 parsers are :

* ``deb822-lossless``: parser that preserves formatting, whitespace and comments as well as allowing syntax errors.
* ``deb822-fast``: a fast parser that does not preserve formatting, whitespace and comments.

Several crates that build on these two crates are:

* ``debian-control``: A parser and editor for Debian control files, apt lists.
* ``debian-copyright``: A parser and editor for Debian copyright files.
* ``dep3``: A parser and editor for Debian DEP-3 headers.
* [r-description](https://github.com/jelmer/r-description-rs): A parser and
editor for R DESCRIPTION files.
* ``apt-sources``: A parser and editor for APT source files (package repositories specification).
* `debian-watch`: A parser and editor for debian/watch files (format 5+ is de8b22-based)

## Other parsers

* `debian-cangelog`: A parser and editor for Debian changelog files
* `lintian-overrides`: A parser and editor for lintian override files
