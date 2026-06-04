//! A library for parsing and manipulating debian/copyright files that
//! use the DEP-5 format.
//!
//! This library is intended to be used for manipulating debian/copyright
//!
//! # Examples
//!
//! ```rust
//!
//! use debian_copyright::lossless::Copyright;
//! use std::path::Path;
//!
//! let text = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
//! Upstream-Author: John Doe <john@example>
//! Upstream-Name: example
//! Source: https://example.com/example
//!
//! Files: *
//! License: GPL-3+
//! Copyright: 2019 John Doe
//!
//! Files: debian/*
//! License: GPL-3+
//! Copyright: 2019 Jane Packager
//!
//! License: GPL-3+
//!  This program is free software: you can redistribute it and/or modify
//!  it under the terms of the GNU General Public License as published by
//!  the Free Software Foundation, either version 3 of the License, or
//!  (at your option) any later version.
//! "#;
//!
//! let c = text.parse::<Copyright>().unwrap();
//! let license = c.find_license_for_file(Path::new("debian/foo")).unwrap();
//! assert_eq!(license.name(), Some("GPL-3+"));
//! ```

use crate::{License, CURRENT_FORMAT, KNOWN_FORMATS};
use deb822_edit::IndentPattern;
use deb822_edit::{Deb822, Paragraph, TextRange};
use std::path::Path;

/// Decode deb822 paragraph markers in a multi-line field value.
///
/// According to Debian policy, blank lines in multi-line field values are
/// represented as lines containing only "." (a single period). The deb822-edit
/// parser already strips the leading indentation whitespace from continuation lines,
/// so we only need to decode the period markers back to blank lines.
///
/// # Arguments
///
/// * `text` - The raw field value text from deb822-edit with indentation already stripped
///
/// # Returns
///
/// The decoded text with blank lines restored
fn decode_field_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line == "." {
                // Paragraph marker representing a blank line
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Encode blank lines in a field value to deb822 paragraph markers.
///
/// According to Debian policy, blank lines in multi-line field values must be
/// represented as lines containing only "." (a single period). The deb822-edit
/// library will reject values with actual blank lines, so we must encode them first.
///
/// # Arguments
///
/// * `text` - The decoded text with normal blank lines
///
/// # Returns
///
/// The encoded text with blank lines replaced by "."
pub fn encode_field_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                // Blank line must be encoded as period marker
                "."
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Field order for header paragraphs according to DEP-5 specification
const HEADER_FIELD_ORDER: &[&str] = &[
    "Format",
    "Upstream-Name",
    "Upstream-Contact",
    "Source",
    "Files-Excluded",
    "Files-Included",
    "Disclaimer",
    "Comment",
    "License",
    "Copyright",
];

/// Field order for Files paragraphs according to DEP-5 specification
const FILES_FIELD_ORDER: &[&str] = &["Files", "Copyright", "License", "Comment"];

/// Field order for standalone License paragraphs according to DEP-5 specification
const LICENSE_FIELD_ORDER: &[&str] = &["License", "Comment"];

/// Default separator for files in Files field
const FILES_SEPARATOR: &str = " ";

/// A copyright file
#[derive(Debug, Clone, PartialEq)]
pub struct Copyright(Deb822);

impl Copyright {
    /// Capture an independent snapshot of this copyright file.
    ///
    /// The returned value shares the underlying immutable green-node data
    /// with `self` at the time of the call, but lives in its own mutable
    /// tree: subsequent mutations to `self` do not propagate to the snapshot.
    /// Pair with [`Self::tree_eq`] to detect later mutations.
    pub fn snapshot(&self) -> Self {
        Copyright(self.0.snapshot())
    }

    /// Returns true iff the syntax trees of `self` and `other` are
    /// value-equal. An O(1) pointer-identity fast path makes this free for
    /// trees that still share state with a recent [`Self::snapshot`].
    pub fn tree_eq(&self, other: &Self) -> bool {
        self.0.tree_eq(&other.0)
    }

    /// Create a new copyright file, with the current format
    pub fn new() -> Self {
        let mut deb822 = Deb822::new();
        let mut header = deb822.add_paragraph();
        header.set("Format", CURRENT_FORMAT);
        Copyright(deb822)
    }

    /// Create a new empty copyright file
    ///
    /// The difference with `new` is that this does not add the `Format` field.
    pub fn empty() -> Self {
        Self(Deb822::new())
    }

    /// Return the underlying Deb822 object
    pub fn as_deb822(&self) -> &Deb822 {
        &self.0
    }

    /// Return the header paragraph
    pub fn header(&self) -> Option<Header> {
        self.0.paragraphs().next().map(Header)
    }

    /// Iterate over all files paragraphs
    pub fn iter_files(&self) -> impl Iterator<Item = FilesParagraph> {
        self.0
            .paragraphs()
            .filter(|x| x.contains_key("Files"))
            .map(FilesParagraph)
    }

    /// Iter over all license paragraphs
    pub fn iter_licenses(&self) -> impl Iterator<Item = LicenseParagraph> {
        self.0
            .paragraphs()
            .filter(|x| {
                !x.contains_key("Files") && !x.contains_key("Format") && x.contains_key("License")
            })
            .map(LicenseParagraph)
    }

    /// Return the header paragraph if it intersects with the given text range
    ///
    /// # Arguments
    /// * `range` - The text range to query
    ///
    /// # Returns
    /// The header paragraph if it exists and its text range overlaps with the provided range
    pub fn header_in_range(&self, range: TextRange) -> Option<Header> {
        self.header().filter(|h| {
            let para_range = h.as_deb822().text_range();
            para_range.start() < range.end() && para_range.end() > range.start()
        })
    }

    /// Iterate over files paragraphs that intersect with the given text range
    ///
    /// # Arguments
    /// * `range` - The text range to query
    ///
    /// # Returns
    /// An iterator over files paragraphs whose text ranges overlap with the provided range
    pub fn iter_files_in_range(
        &self,
        range: TextRange,
    ) -> impl Iterator<Item = FilesParagraph> + '_ {
        self.iter_files().filter(move |f| {
            let para_range = f.as_deb822().text_range();
            para_range.start() < range.end() && para_range.end() > range.start()
        })
    }

    /// Iterate over license paragraphs that intersect with the given text range
    ///
    /// # Arguments
    /// * `range` - The text range to query
    ///
    /// # Returns
    /// An iterator over license paragraphs whose text ranges overlap with the provided range
    pub fn iter_licenses_in_range(
        &self,
        range: TextRange,
    ) -> impl Iterator<Item = LicenseParagraph> + '_ {
        self.iter_licenses().filter(move |l| {
            let para_range = l.as_deb822().text_range();
            para_range.start() < range.end() && para_range.end() > range.start()
        })
    }

    /// Returns the Files paragraph for the given filename.
    ///
    /// Consistent with the specification, this returns the last paragraph
    /// that matches (which should be the most specific)
    pub fn find_files(&self, filename: &Path) -> Option<FilesParagraph> {
        self.iter_files().filter(|p| p.matches(filename)).last()
    }

    /// Find license by name
    ///
    /// This will return the first license paragraph that has the given name.
    pub fn find_license_by_name(&self, name: &str) -> Option<License> {
        self.iter_licenses()
            .find(|p| p.name().as_deref() == Some(name))
            .map(|x| x.into())
    }

    /// Returns the license for the given file.
    pub fn find_license_for_file(&self, filename: &Path) -> Option<License> {
        let files = self.find_files(filename)?;
        let license = files.license()?;
        if license.text().is_some() {
            return Some(license);
        }
        self.find_license_by_name(license.name()?)
    }

    /// Read copyright file from a string, allowing syntax errors
    pub fn from_str_relaxed(s: &str) -> Result<(Self, Vec<String>), Error> {
        if !s.starts_with("Format:") {
            return Err(Error::NotMachineReadable);
        }

        let (deb822, errors) = Deb822::from_str_relaxed(s);
        Ok((Self(deb822), errors))
    }

    /// Read copyright file from a file, allowing syntax errors
    pub fn from_file_relaxed<P: AsRef<Path>>(path: P) -> Result<(Self, Vec<String>), Error> {
        let text = std::fs::read_to_string(path)?;
        Self::from_str_relaxed(&text)
    }

    /// Read copyright file from a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path)?;
        use std::str::FromStr;
        Self::from_str(&text)
    }

    /// Add a new files paragraph
    ///
    /// Returns a mutable reference to the newly created FilesParagraph
    pub fn add_files(
        &mut self,
        files: &[&str],
        copyright: &[&str],
        license: &License,
    ) -> FilesParagraph {
        let mut para = self.0.add_paragraph();
        para.set_with_field_order("Files", &files.join(FILES_SEPARATOR), FILES_FIELD_ORDER);
        para.set_with_field_order("Copyright", &copyright.join("\n"), FILES_FIELD_ORDER);
        let license_text = match license {
            License::Name(name) => name.to_string(),
            License::Named(name, text) => format!("{}\n{}", name, encode_field_text(text)),
            License::Text(text) => encode_field_text(text),
        };
        para.set_with_forced_indent(
            "License",
            &license_text,
            &IndentPattern::Fixed(1),
            Some(FILES_FIELD_ORDER),
        );
        FilesParagraph(para)
    }

    /// Add a new license paragraph
    ///
    /// Returns a mutable reference to the newly created LicenseParagraph
    pub fn add_license(&mut self, license: &License) -> LicenseParagraph {
        let mut para = self.0.add_paragraph();
        let license_text = match license {
            License::Name(name) => name.to_string(),
            License::Named(name, text) => format!("{}\n{}", name, encode_field_text(text)),
            License::Text(text) => encode_field_text(text),
        };
        // Force 1-space indentation for License field according to DEP-5 spec
        para.set_with_indent_pattern(
            "License",
            &license_text,
            Some(&IndentPattern::Fixed(1)),
            Some(LICENSE_FIELD_ORDER),
        );
        LicenseParagraph(para)
    }

    /// Remove a license paragraph by its short name
    ///
    /// This removes the first standalone license paragraph that matches the given name.
    /// Returns true if a paragraph was removed, false otherwise.
    pub fn remove_license_by_name(&mut self, name: &str) -> bool {
        // Find the index of the license paragraph
        let mut index = None;
        for (i, para) in self.0.paragraphs().enumerate() {
            if !para.contains_key("Files")
                && !para.contains_key("Format")
                && para.contains_key("License")
            {
                let license_para = LicenseParagraph(para);
                if license_para.name().as_deref() == Some(name) {
                    index = Some(i);
                    break;
                }
            }
        }

        if let Some(i) = index {
            self.0.remove_paragraph(i);
            true
        } else {
            false
        }
    }

    /// Remove a files paragraph by matching file pattern
    ///
    /// This removes the first files paragraph where the Files field contains the given pattern.
    /// Returns true if a paragraph was removed, false otherwise.
    pub fn remove_files_by_pattern(&mut self, pattern: &str) -> bool {
        // Find the index of the files paragraph
        let mut index = None;
        for (i, para) in self.0.paragraphs().enumerate() {
            if para.contains_key("Files") {
                let files_para = FilesParagraph(para);
                if files_para.files().iter().any(|f| f == pattern) {
                    index = Some(i);
                    break;
                }
            }
        }

        if let Some(i) = index {
            self.0.remove_paragraph(i);
            true
        } else {
            false
        }
    }

    /// Wrap and sort the entire copyright file
    ///
    /// This will:
    /// - Sort paragraphs according to DEP-5 conventions (header first, Files paragraphs sorted by pattern, License paragraphs last)
    /// - Sort file patterns within Files paragraphs (wildcards first, debian/* last)
    /// - Sort fields within each paragraph according to their respective field orders
    /// - Wrap long lines according to the provided parameters
    ///
    /// # Arguments
    /// * `indentation` - The indentation to use for multi-line fields
    /// * `immediate_empty_line` - Whether to add an empty line at the start of multi-line fields
    /// * `max_line_length_one_liner` - The maximum line length for one-liner fields
    pub fn wrap_and_sort(
        &mut self,
        indentation: deb822_edit::Indentation,
        immediate_empty_line: bool,
        max_line_length_one_liner: Option<usize>,
    ) {
        // Sort paragraphs: header first, Files paragraphs by pattern, License paragraphs last
        let sort_paragraphs = |a: &Paragraph, b: &Paragraph| -> std::cmp::Ordering {
            let a_is_header = a.contains_key("Format");
            let b_is_header = b.contains_key("Format");
            let a_is_files = a.contains_key("Files");
            let b_is_files = b.contains_key("Files");

            // Header always comes first
            if a_is_header && !b_is_header {
                return std::cmp::Ordering::Less;
            }
            if !a_is_header && b_is_header {
                return std::cmp::Ordering::Greater;
            }

            // Files paragraphs come before license paragraphs
            if a_is_files && !b_is_files && !b_is_header {
                return std::cmp::Ordering::Less;
            }
            if !a_is_files && b_is_files && !a_is_header {
                return std::cmp::Ordering::Greater;
            }

            // Sort Files paragraphs by their first file pattern
            if a_is_files && b_is_files {
                let a_files = a.get("Files").unwrap_or_default();
                let b_files = b.get("Files").unwrap_or_default();

                let a_first = a_files.split_whitespace().next().unwrap_or("");
                let b_first = b_files.split_whitespace().next().unwrap_or("");

                let a_depth = crate::pattern_depth(a_first);
                let b_depth = crate::pattern_depth(b_first);

                let a_key = crate::pattern_sort_key(a_first, a_depth);
                let b_key = crate::pattern_sort_key(b_first, b_depth);

                return a_key.cmp(&b_key);
            }

            std::cmp::Ordering::Equal
        };

        // Wrap and sort each paragraph based on its type
        let wrap_and_sort_para = |para: &Paragraph| -> Paragraph {
            let is_header = para.contains_key("Format");
            let is_files = para.contains_key("Files");

            if is_header {
                let mut header = Header(para.clone());
                header.wrap_and_sort(indentation, immediate_empty_line, max_line_length_one_liner);
                header.0
            } else if is_files {
                let mut files = FilesParagraph(para.clone());
                files.wrap_and_sort(indentation, immediate_empty_line, max_line_length_one_liner);
                files.0
            } else {
                let mut license = LicenseParagraph(para.clone());
                license.wrap_and_sort(indentation, immediate_empty_line, max_line_length_one_liner);
                license.0
            }
        };

        self.0 = self
            .0
            .wrap_and_sort(Some(&sort_paragraphs), Some(&wrap_and_sort_para));
    }
}

/// Error parsing copyright files
#[derive(Debug)]
pub enum Error {
    /// Parse error
    ParseError(deb822_edit::ParseError),

    /// IO error
    IoError(std::io::Error),

    /// Invalid value (e.g., empty continuation lines)
    InvalidValue(String),

    /// The file is not machine readable
    NotMachineReadable,
}

impl From<deb822_edit::Error> for Error {
    fn from(e: deb822_edit::Error) -> Self {
        match e {
            deb822_edit::Error::ParseError(e) => Error::ParseError(e),
            deb822_edit::Error::IoError(e) => Error::IoError(e),
            deb822_edit::Error::InvalidValue(msg) => Error::InvalidValue(msg),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<deb822_edit::ParseError> for Error {
    fn from(e: deb822_edit::ParseError) -> Self {
        Error::ParseError(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self {
            Error::ParseError(e) => write!(f, "parse error: {}", e),
            Error::NotMachineReadable => write!(f, "not machine readable"),
            Error::IoError(e) => write!(f, "io error: {}", e),
            Error::InvalidValue(msg) => write!(f, "invalid value: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

impl Default for Copyright {
    fn default() -> Self {
        Copyright(Deb822::new())
    }
}

impl std::str::FromStr for Copyright {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with("Format:") {
            return Err(Error::NotMachineReadable);
        }
        Ok(Self(Deb822::from_str(s)?))
    }
}

impl std::fmt::Display for Copyright {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

/// A header paragraph
pub struct Header(Paragraph);

impl Header {
    /// Returns the format string for this file.
    pub fn format_string(&self) -> Option<String> {
        self.0
            .get("Format")
            .or_else(|| self.0.get("Format-Specification"))
    }

    /// Return the underlying Deb822 paragraph
    pub fn as_deb822(&self) -> &Paragraph {
        &self.0
    }

    /// Return the underlying Deb822 paragraph, mutably
    #[deprecated = "Use as_deb822 instead"]
    pub fn as_mut_deb822(&mut self) -> &mut Paragraph {
        &mut self.0
    }

    /// Upstream name
    pub fn upstream_name(&self) -> Option<String> {
        self.0.get("Upstream-Name")
    }

    /// Set an arbitrary field on the header, honouring the canonical
    /// DEP-5 field order so the new field lands in the right position.
    ///
    /// Use this when the typed setters (`set_upstream_name`,
    /// `set_source`, …) don't cover the field you need.
    pub fn set_field(&mut self, name: &str, value: &str) {
        self.0.set_with_field_order(name, value, HEADER_FIELD_ORDER);
    }

    /// Remove a field from the header. A no-op if the field isn't set.
    pub fn remove_field(&mut self, name: &str) {
        self.0.remove(name);
    }

    /// Set the upstream name
    pub fn set_upstream_name(&mut self, name: &str) {
        self.0
            .set_with_field_order("Upstream-Name", name, HEADER_FIELD_ORDER);
    }

    /// Upstream contact
    pub fn upstream_contact(&self) -> Option<String> {
        self.0.get("Upstream-Contact")
    }

    /// Set the upstream contact
    pub fn set_upstream_contact(&mut self, contact: &str) {
        self.0
            .set_with_field_order("Upstream-Contact", contact, HEADER_FIELD_ORDER);
    }

    /// Source
    pub fn source(&self) -> Option<String> {
        self.0.get("Source")
    }

    /// Set the source
    pub fn set_source(&mut self, source: &str) {
        self.0
            .set_with_field_order("Source", source, HEADER_FIELD_ORDER);
    }

    /// List of files excluded from the copyright information, as well as the source package
    pub fn files_excluded(&self) -> Option<Vec<String>> {
        self.0
            .get("Files-Excluded")
            .map(|x| x.split('\n').map(|x| x.to_string()).collect::<Vec<_>>())
    }

    /// Set excluded files
    pub fn set_files_excluded(&mut self, files: &[&str]) {
        self.0
            .set_with_field_order("Files-Excluded", &files.join("\n"), HEADER_FIELD_ORDER);
    }

    /// List of files re-included after exclusion by `Files-Excluded`
    pub fn files_included(&self) -> Option<Vec<String>> {
        self.0
            .get("Files-Included")
            .map(|x| x.split('\n').map(|x| x.to_string()).collect::<Vec<_>>())
    }

    /// Set included files
    pub fn set_files_included(&mut self, files: &[&str]) {
        self.0
            .set_with_field_order("Files-Included", &files.join("\n"), HEADER_FIELD_ORDER);
    }

    /// Check whether a file is included in the upstream source.
    ///
    /// A file is considered excluded if it matches any pattern in `Files-Excluded`
    /// and does not match any pattern in `Files-Included`. If there are no
    /// `Files-Excluded` entries, all files are considered included.
    pub fn is_file_included(&self, filename: &Path) -> bool {
        let excluded = self.files_excluded().unwrap_or_default();
        let fname = filename.to_str().unwrap();
        let is_excluded = excluded
            .iter()
            .any(|pattern| crate::GlobPattern::new(pattern).is_match(fname));
        if !is_excluded {
            return true;
        }
        let included = self.files_included().unwrap_or_default();
        included
            .iter()
            .any(|pattern| crate::GlobPattern::new(pattern).is_match(fname))
    }

    /// Fix the the header paragraph
    ///
    /// Currently this just renames `Format-Specification` to `Format` and replaces older format
    /// strings with the current format string.
    pub fn fix(&mut self) {
        if self.0.contains_key("Format-Specification") {
            self.0.rename("Format-Specification", "Format");
        }

        if let Some(mut format) = self.0.get("Format") {
            if !format.ends_with('/') {
                format.push('/');
            }

            if let Some(rest) = format.strip_prefix("http:") {
                format = format!("https:{}", rest);
            }

            if KNOWN_FORMATS.contains(&format.as_str()) {
                format = CURRENT_FORMAT.to_string();
            }

            self.0.set("Format", format.as_str());
        }
    }

    /// Wrap and sort the header paragraph
    ///
    /// # Arguments
    /// * `indentation` - The indentation to use
    /// * `immediate_empty_line` - Whether to add an empty line at the start of multi-line fields
    /// * `max_line_length_one_liner` - The maximum line length for one-liner fields
    pub fn wrap_and_sort(
        &mut self,
        indentation: deb822_edit::Indentation,
        immediate_empty_line: bool,
        max_line_length_one_liner: Option<usize>,
    ) {
        let sort_entries =
            |a: &deb822_edit::Entry, b: &deb822_edit::Entry| -> std::cmp::Ordering {
                let a_key = a.key().unwrap_or_default();
                let b_key = b.key().unwrap_or_default();
                let a_pos = HEADER_FIELD_ORDER.iter().position(|&k| k == a_key);
                let b_pos = HEADER_FIELD_ORDER.iter().position(|&k| k == b_key);
                match (a_pos, b_pos) {
                    (Some(a_idx), Some(b_idx)) => a_idx.cmp(&b_idx),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            };
        self.0 = self.0.wrap_and_sort(
            indentation,
            immediate_empty_line,
            max_line_length_one_liner,
            Some(&sort_entries),
            None,
        );
    }
}

/// A files paragraph
pub struct FilesParagraph(Paragraph);

impl FilesParagraph {
    /// Return the underlying Deb822 paragraph
    pub fn as_deb822(&self) -> &Paragraph {
        &self.0
    }

    /// Set an arbitrary field on the paragraph, honouring the canonical
    /// DEP-5 Files-paragraph field order. The `License` field is forced
    /// to 1-space indentation per DEP-5; other fields preserve existing
    /// or auto-detected indentation.
    pub fn set_field(&mut self, name: &str, value: &str) {
        if name.eq_ignore_ascii_case("License") {
            let indent_pattern = deb822_edit::IndentPattern::Fixed(1);
            self.0
                .set_with_forced_indent(name, value, &indent_pattern, Some(FILES_FIELD_ORDER));
        } else {
            self.0.set_with_field_order(name, value, FILES_FIELD_ORDER);
        }
    }

    /// Remove a field from the paragraph. A no-op if not present.
    pub fn remove_field(&mut self, name: &str) {
        self.0.remove(name);
    }

    /// List of file patterns in the paragraph
    pub fn files(&self) -> Vec<String> {
        self.0
            .get("Files")
            .unwrap()
            .split_whitespace()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
    }

    /// Set the file patterns in the paragraph
    pub fn set_files(&mut self, files: &[&str]) {
        self.0
            .set_with_field_order("Files", &files.join(FILES_SEPARATOR), FILES_FIELD_ORDER);
    }

    /// Add a file pattern to the paragraph
    ///
    /// If the pattern already exists, it will not be added again.
    pub fn add_file(&mut self, pattern: &str) {
        let mut files = self.files();
        if !files.contains(&pattern.to_string()) {
            files.push(pattern.to_string());
            self.0
                .set_with_field_order("Files", &files.join(FILES_SEPARATOR), FILES_FIELD_ORDER);
        }
    }

    /// Remove a file pattern from the paragraph
    ///
    /// Returns true if the pattern was found and removed, false otherwise.
    pub fn remove_file(&mut self, pattern: &str) -> bool {
        let mut files = self.files();
        if let Some(pos) = files.iter().position(|f| f == pattern) {
            files.remove(pos);
            self.0
                .set_with_field_order("Files", &files.join(FILES_SEPARATOR), FILES_FIELD_ORDER);
            true
        } else {
            false
        }
    }

    /// Check whether the paragraph matches the given filename
    pub fn matches(&self, filename: &std::path::Path) -> bool {
        self.files()
            .iter()
            .any(|f| crate::GlobPattern::new(f).is_match(filename.to_str().unwrap()))
    }

    /// Copyright holders in the paragraph
    pub fn copyright(&self) -> Vec<String> {
        self.0
            .get("Copyright")
            .unwrap_or_default()
            .split('\n')
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
    }

    /// Set the copyright
    pub fn set_copyright(&mut self, authors: &[&str]) {
        self.0
            .set_with_field_order("Copyright", &authors.join("\n"), FILES_FIELD_ORDER);
    }

    /// Comment associated with the files paragraph
    pub fn comment(&self) -> Option<String> {
        self.0.get("Comment")
    }

    /// Set the comment associated with the files paragraph
    pub fn set_comment(&mut self, comment: &str) {
        self.0
            .set_with_field_order("Comment", comment, FILES_FIELD_ORDER);
    }

    /// License in the paragraph
    pub fn license(&self) -> Option<License> {
        self.0.get_multiline("License").map(|x| {
            x.split_once('\n').map_or_else(
                || License::Name(x.to_string()),
                |(name, text)| {
                    let decoded_text = decode_field_text(text);
                    if name.is_empty() {
                        License::Text(decoded_text)
                    } else {
                        License::Named(name.to_string(), decoded_text)
                    }
                },
            )
        })
    }

    /// Set the license associated with the files paragraph
    pub fn set_license(&mut self, license: &License) {
        let text = match license {
            License::Name(name) => name.to_string(),
            License::Named(name, text) => format!("{}\n{}", name, encode_field_text(text)),
            License::Text(text) => encode_field_text(text),
        };
        // Force 1-space indentation for License field according to DEP-5 spec
        let indent_pattern = deb822_edit::IndentPattern::Fixed(1);
        self.0
            .set_with_forced_indent("License", &text, &indent_pattern, Some(FILES_FIELD_ORDER));
    }

    /// Wrap and sort the files paragraph
    ///
    /// # Arguments
    /// * `indentation` - The indentation to use
    /// * `immediate_empty_line` - Whether to add an empty line at the start of multi-line fields
    /// * `max_line_length_one_liner` - The maximum line length for one-liner fields
    pub fn wrap_and_sort(
        &mut self,
        indentation: deb822_edit::Indentation,
        immediate_empty_line: bool,
        max_line_length_one_liner: Option<usize>,
    ) {
        let sort_entries =
            |a: &deb822_edit::Entry, b: &deb822_edit::Entry| -> std::cmp::Ordering {
                let a_key = a.key().unwrap_or_default();
                let b_key = b.key().unwrap_or_default();
                let a_pos = FILES_FIELD_ORDER.iter().position(|&k| k == a_key);
                let b_pos = FILES_FIELD_ORDER.iter().position(|&k| k == b_key);
                match (a_pos, b_pos) {
                    (Some(a_idx), Some(b_idx)) => a_idx.cmp(&b_idx),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            };

        let format_value = |key: &str, value: &str| -> String {
            if key == "Files" {
                let mut patterns: Vec<_> = value.split_whitespace().collect();
                patterns.sort_by_key(|p| {
                    let depth = crate::pattern_depth(p);
                    crate::pattern_sort_key(p, depth)
                });
                patterns.join(FILES_SEPARATOR)
            } else {
                value.to_string()
            }
        };

        self.0 = self.0.wrap_and_sort(
            indentation,
            immediate_empty_line,
            max_line_length_one_liner,
            Some(&sort_entries),
            Some(&format_value),
        );
    }
}

/// A paragraph that contains a license
pub struct LicenseParagraph(Paragraph);

impl From<LicenseParagraph> for License {
    fn from(p: LicenseParagraph) -> Self {
        p.license()
    }
}

impl LicenseParagraph {
    /// Return the underlying Deb822 paragraph
    pub fn as_deb822(&self) -> &Paragraph {
        &self.0
    }

    /// Set an arbitrary field on the paragraph, honouring the canonical
    /// DEP-5 standalone-License-paragraph field order. The `License`
    /// field is forced to 1-space indentation per DEP-5; other fields
    /// preserve existing or auto-detected indentation.
    pub fn set_field(&mut self, name: &str, value: &str) {
        if name.eq_ignore_ascii_case("License") {
            let indent_pattern = deb822_edit::IndentPattern::Fixed(1);
            self.0
                .set_with_forced_indent(name, value, &indent_pattern, Some(LICENSE_FIELD_ORDER));
        } else {
            self.0
                .set_with_field_order(name, value, LICENSE_FIELD_ORDER);
        }
    }

    /// Remove a field from the paragraph. A no-op if not present.
    pub fn remove_field(&mut self, name: &str) {
        self.0.remove(name);
    }

    /// Comment associated with the license
    pub fn comment(&self) -> Option<String> {
        self.0.get("Comment")
    }

    /// Set the comment associated with the license
    pub fn set_comment(&mut self, comment: &str) {
        self.0
            .set_with_field_order("Comment", comment, LICENSE_FIELD_ORDER);
    }

    /// Name of the license
    pub fn name(&self) -> Option<String> {
        self.0
            .get_multiline("License")
            .and_then(|x| x.split_once('\n').map(|(name, _)| name.to_string()))
    }

    /// Text of the license
    pub fn text(&self) -> Option<String> {
        self.0
            .get_multiline("License")
            .and_then(|x| x.split_once('\n').map(|(_, text)| decode_field_text(text)))
    }

    /// Get the license as a License enum
    pub fn license(&self) -> License {
        let x = self.0.get_multiline("License").unwrap();
        x.split_once('\n').map_or_else(
            || License::Name(x.to_string()),
            |(name, text)| {
                let decoded_text = decode_field_text(text);
                if name.is_empty() {
                    License::Text(decoded_text)
                } else {
                    License::Named(name.to_string(), decoded_text)
                }
            },
        )
    }

    /// Set the license
    pub fn set_license(&mut self, license: &License) {
        let text = match license {
            License::Name(name) => name.to_string(),
            License::Named(name, text) => format!("{}\n{}", name, encode_field_text(text)),
            License::Text(text) => encode_field_text(text),
        };
        // Force 1-space indentation for License field according to DEP-5 spec
        let indent_pattern = deb822_edit::IndentPattern::Fixed(1);
        self.0
            .set_with_forced_indent("License", &text, &indent_pattern, Some(LICENSE_FIELD_ORDER));
    }

    /// Set just the license name (short name on the first line)
    ///
    /// If the license currently has text, it will be preserved.
    /// If the license has no text, this will set it to just a name.
    pub fn set_name(&mut self, name: &str) {
        let current = self.license();
        let new_license = match current {
            License::Named(_, text) | License::Text(text) => License::Named(name.to_string(), text),
            License::Name(_) => License::Name(name.to_string()),
        };
        self.set_license(&new_license);
    }

    /// Set just the license text (the full license text after the first line)
    ///
    /// If text is None, removes the license text while keeping the name.
    /// If the license currently has a name, it will be preserved.
    /// If the license has no name and text is Some, this will create a license with just text.
    pub fn set_text(&mut self, text: Option<&str>) {
        let current = self.license();
        let new_license = match (current, text) {
            (License::Named(name, _), Some(new_text)) | (License::Name(name), Some(new_text)) => {
                License::Named(name, new_text.to_string())
            }
            (License::Named(name, _), None) | (License::Name(name), None) => License::Name(name),
            (License::Text(_), Some(new_text)) => License::Text(new_text.to_string()),
            (License::Text(_), None) => {
                // Edge case: removing text from a text-only license. Set empty name.
                License::Name(String::new())
            }
        };
        self.set_license(&new_license);
    }

    /// Wrap and sort the license paragraph
    ///
    /// # Arguments
    /// * `indentation` - The indentation to use
    /// * `immediate_empty_line` - Whether to add an empty line at the start of multi-line fields
    /// * `max_line_length_one_liner` - The maximum line length for one-liner fields
    pub fn wrap_and_sort(
        &mut self,
        indentation: deb822_edit::Indentation,
        immediate_empty_line: bool,
        max_line_length_one_liner: Option<usize>,
    ) {
        let sort_entries =
            |a: &deb822_edit::Entry, b: &deb822_edit::Entry| -> std::cmp::Ordering {
                let a_key = a.key().unwrap_or_default();
                let b_key = b.key().unwrap_or_default();
                let a_pos = LICENSE_FIELD_ORDER.iter().position(|&k| k == a_key);
                let b_pos = LICENSE_FIELD_ORDER.iter().position(|&k| k == b_key);
                match (a_pos, b_pos) {
                    (Some(a_idx), Some(b_idx)) => a_idx.cmp(&b_idx),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            };
        self.0 = self.0.wrap_and_sort(
            indentation,
            immediate_empty_line,
            max_line_length_one_liner,
            Some(&sort_entries),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use deb822_edit::{TextRange, TextSize};

    #[test]
    fn test_not_machine_readable() {
        let s = r#"
This copyright file is not machine readable.
"#;
        let ret = s.parse::<super::Copyright>();
        assert!(ret.is_err());
        assert!(matches!(ret.unwrap_err(), super::Error::NotMachineReadable));
    }

    #[test]
    fn test_new() {
        let n = super::Copyright::new();
        assert_eq!(
            n.to_string().as_str(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"
        );
    }

    #[test]
    fn test_parse() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: foo
Upstream-Contact: Joe Bloggs <joe@example.com>
Source: https://example.com/foo

Files: *
Copyright:
  2020 Joe Bloggs <joe@example.com>
License: GPL-3+

Files: debian/*
Comment: Debian packaging is licensed under the GPL-3+.
Copyright: 2023 Jelmer Vernooij
License: GPL-3+

License: GPL-3+
 This program is free software: you can redistribute it and/or modify
 it under the terms of the GNU General Public License as published by
 the Free Software Foundation, either version 3 of the License, or
 (at your option) any later version.
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");

        assert_eq!(
            "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/",
            copyright.header().unwrap().format_string().unwrap()
        );
        assert_eq!("foo", copyright.header().unwrap().upstream_name().unwrap());
        assert_eq!(
            "Joe Bloggs <joe@example.com>",
            copyright.header().unwrap().upstream_contact().unwrap()
        );
        assert_eq!(
            "https://example.com/foo",
            copyright.header().unwrap().source().unwrap()
        );

        let files = copyright.iter_files().collect::<Vec<_>>();
        assert_eq!(2, files.len());
        assert_eq!("*", files[0].files().join(" "));
        assert_eq!("debian/*", files[1].files().join(" "));
        assert_eq!(
            "Debian packaging is licensed under the GPL-3+.",
            files[1].comment().unwrap()
        );
        assert_eq!(
            vec!["2023 Jelmer Vernooij".to_string()],
            files[1].copyright()
        );
        assert_eq!("GPL-3+", files[1].license().unwrap().name().unwrap());
        assert_eq!(files[1].license().unwrap().text(), None);

        let licenses = copyright.iter_licenses().collect::<Vec<_>>();
        assert_eq!(1, licenses.len());
        assert_eq!("GPL-3+", licenses[0].name().unwrap());
        assert_eq!(
            "This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.",
            licenses[0].text().unwrap()
        );

        let upstream_files = copyright.find_files(std::path::Path::new("foo.c")).unwrap();
        assert_eq!(vec!["*"], upstream_files.files());

        let debian_files = copyright
            .find_files(std::path::Path::new("debian/foo.c"))
            .unwrap();
        assert_eq!(vec!["debian/*"], debian_files.files());

        let gpl = copyright.find_license_by_name("GPL-3+");
        assert!(gpl.is_some());

        let gpl = copyright.find_license_for_file(std::path::Path::new("debian/foo.c"));
        assert_eq!(gpl.unwrap().name().unwrap(), "GPL-3+");
    }

    #[test]
    fn test_from_str_relaxed() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: foo
Source: https://example.com/foo

Files: *
Copyright: 2020 Joe Bloggs <joe@example.com>
License: GPL-3+
"#;
        let (copyright, errors) = super::Copyright::from_str_relaxed(s).unwrap();
        assert!(errors.is_empty());
        assert_eq!("foo", copyright.header().unwrap().upstream_name().unwrap());
    }

    #[test]
    fn test_from_file_relaxed() {
        let tmpfile = std::env::temp_dir().join("test_copyright.txt");
        std::fs::write(
            &tmpfile,
            r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: foo
Source: https://example.com/foo

Files: *
Copyright: 2020 Joe Bloggs <joe@example.com>
License: GPL-3+
"#,
        )
        .unwrap();
        let (copyright, errors) = super::Copyright::from_file_relaxed(&tmpfile).unwrap();
        assert!(errors.is_empty());
        assert_eq!("foo", copyright.header().unwrap().upstream_name().unwrap());
        std::fs::remove_file(&tmpfile).unwrap();
    }

    #[test]
    fn test_header_set_upstream_contact() {
        let copyright = super::Copyright::new();
        let mut header = copyright.header().unwrap();
        header.set_upstream_contact("Test Person <test@example.com>");
        assert_eq!(
            header.upstream_contact().unwrap(),
            "Test Person <test@example.com>"
        );
    }

    #[test]
    fn test_header_set_source() {
        let copyright = super::Copyright::new();
        let mut header = copyright.header().unwrap();
        header.set_source("https://example.com/source");
        assert_eq!(header.source().unwrap(), "https://example.com/source");
    }

    #[test]
    fn test_header_set_field_honours_field_order() {
        // Inserting Upstream-Name via set_field should land between Format
        // and Source, matching the canonical DEP-5 header field order.
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Source: https://example.com/foo
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut header = copyright.header().unwrap();
        header.set_field("Upstream-Name", "foo");

        assert_eq!(header.upstream_name().as_deref(), Some("foo"));
        let expected =
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
                        Upstream-Name: foo\n\
                        Source: https://example.com/foo\n";
        assert_eq!(copyright.to_string(), expected);
    }

    #[test]
    fn test_header_set_field_updates_existing() {
        let copyright = super::Copyright::new();
        let mut header = copyright.header().unwrap();
        header.set_field("Upstream-Name", "foo");
        header.set_field("Upstream-Name", "bar");
        assert_eq!(header.upstream_name().as_deref(), Some("bar"));
    }

    #[test]
    fn test_header_set_field_unknown_field() {
        // Unknown fields aren't in HEADER_FIELD_ORDER but should still be set.
        let copyright = super::Copyright::new();
        let mut header = copyright.header().unwrap();
        header.set_field("X-Custom", "value");
        assert_eq!(header.as_deb822().get("X-Custom").as_deref(), Some("value"));
    }

    #[test]
    fn test_header_remove_field() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: foo
Source: https://example.com/foo
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut header = copyright.header().unwrap();
        header.remove_field("Upstream-Name");
        assert_eq!(header.upstream_name(), None);

        let expected =
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
                        Source: https://example.com/foo\n";
        assert_eq!(copyright.to_string(), expected);
    }

    #[test]
    fn test_header_remove_field_missing_is_noop() {
        let copyright = super::Copyright::new();
        let mut header = copyright.header().unwrap();
        header.remove_field("Upstream-Name");
        assert_eq!(header.upstream_name(), None);
        assert_eq!(
            copyright.to_string(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"
        );
    }

    #[test]
    fn test_files_paragraph_set_field_honours_field_order() {
        // Inserting Comment via set_field should land after License,
        // matching the canonical DEP-5 Files paragraph field order.
        let mut copyright = super::Copyright::new();
        let files = vec!["*"];
        let copyrights = vec!["2020 Joe Bloggs"];
        let license = crate::License::Name("GPL-3+".to_string());
        let mut para = copyright.add_files(&files, &copyrights, &license);
        para.set_field("Comment", "Test comment");

        let expected =
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
                        Files: *\n\
                        Copyright: 2020 Joe Bloggs\n\
                        License: GPL-3+\n\
                        Comment: Test comment\n";
        assert_eq!(copyright.to_string(), expected);
    }

    #[test]
    fn test_files_paragraph_remove_field() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2020 Joe Bloggs
License: GPL-3+
Comment: drop me
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut files = copyright.iter_files().next().expect("no files paragraph");
        files.remove_field("Comment");
        assert_eq!(files.comment(), None);

        let expected =
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
                        Files: *\n\
                        Copyright: 2020 Joe Bloggs\n\
                        License: GPL-3+\n";
        assert_eq!(copyright.to_string(), expected);
    }

    #[test]
    fn test_files_paragraph_remove_field_missing_is_noop() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2020 Joe Bloggs
License: GPL-3+
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let before = copyright.to_string();
        let mut files = copyright.iter_files().next().expect("no files paragraph");
        files.remove_field("Comment");
        assert_eq!(copyright.to_string(), before);
    }

    #[test]
    fn test_files_paragraph_license_decodes_paragraph_markers() {
        // FilesParagraph::license() must decode `.` paragraph markers to
        // blank lines, matching LicenseParagraph::license() and the
        // expectation of set_license() (which encodes them back).
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2020 Joe Bloggs
License: GPL-3+
 First paragraph.
 .
 Second paragraph.
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let files = copyright.iter_files().next().expect("no files paragraph");
        let license = files.license().expect("no license");
        assert_eq!(
            license.text(),
            Some("First paragraph.\n\nSecond paragraph.")
        );
    }

    #[test]
    fn test_files_paragraph_license_round_trips() {
        // license() then set_license() must be a no-op for a multi-paragraph
        // license body.
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2020 Joe Bloggs
License: GPL-3+
 First paragraph.
 .
 Second paragraph.
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut files = copyright.iter_files().next().expect("no files paragraph");
        let license = files.license().expect("no license");
        files.set_license(&license);
        assert_eq!(copyright.to_string(), s);
    }

    #[test]
    fn test_license_paragraph_set_field_honours_field_order() {
        // Inserting Comment via set_field should land after License,
        // matching the canonical DEP-5 standalone-License field order.
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 license text
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut license = copyright
            .iter_licenses()
            .next()
            .expect("no license paragraph");
        license.set_field("Comment", "see https://gnu.org/");

        let expected =
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
                        License: GPL-3+\n license text\n\
                        Comment: see https://gnu.org/\n";
        assert_eq!(copyright.to_string(), expected);
    }

    #[test]
    fn test_license_paragraph_remove_field() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 license text
Comment: drop me
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut license = copyright
            .iter_licenses()
            .next()
            .expect("no license paragraph");
        license.remove_field("Comment");
        assert_eq!(license.comment(), None);

        let expected =
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
                        License: GPL-3+\n license text\n";
        assert_eq!(copyright.to_string(), expected);
    }

    #[test]
    fn test_license_paragraph_remove_field_missing_is_noop() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 license text
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let before = copyright.to_string();
        let mut license = copyright
            .iter_licenses()
            .next()
            .expect("no license paragraph");
        license.remove_field("Comment");
        assert_eq!(copyright.to_string(), before);
    }

    #[test]
    fn test_license_paragraph_set_comment() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 This is the license text.
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut license = copyright.iter_licenses().next().unwrap();
        license.set_comment("This is a test comment");
        assert_eq!(license.comment().unwrap(), "This is a test comment");
    }

    #[test]
    fn test_license_paragraph_set_license() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 Old license text.
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut license = copyright.iter_licenses().next().unwrap();

        let new_license = crate::License::Named(
            "MIT".to_string(),
            "Permission is hereby granted...".to_string(),
        );
        license.set_license(&new_license);

        assert_eq!(license.name().unwrap(), "MIT");
        assert_eq!(license.text().unwrap(), "Permission is hereby granted...");
    }

    #[test]
    fn test_license_from_license_paragraph_decodes_markers() {
        // `License::from(LicenseParagraph)` must agree with
        // LicenseParagraph::license(): both decode `.` paragraph markers.
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-2+
 First paragraph.
 .
 Second paragraph.
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let license_para = copyright.iter_licenses().next().unwrap();
        let via_method = license_para.license();
        let via_from: crate::License = license_para.into();
        assert_eq!(via_from, via_method);
        assert_eq!(
            via_from,
            crate::License::Named(
                "GPL-2+".to_string(),
                "First paragraph.\n\nSecond paragraph.".to_string(),
            )
        );
    }

    #[test]
    fn test_iter_licenses_excludes_header() {
        // Test that iter_licenses does not include the header paragraph even if it has a License field
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: foo
License: GPL-3+

Files: *
Copyright: 2020 Joe Bloggs
License: MIT

License: GPL-3+
 This is the GPL-3+ license text.
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let licenses: Vec<_> = copyright.iter_licenses().collect();

        // Should only have the standalone License paragraph, not the header
        assert_eq!(1, licenses.len());
        assert_eq!("GPL-3+", licenses[0].name().unwrap());
        assert_eq!(
            "This is the GPL-3+ license text.",
            licenses[0].text().unwrap()
        );
    }

    #[test]
    fn test_add_files() {
        let mut copyright = super::Copyright::new();
        let license = crate::License::Name("GPL-3+".to_string());
        copyright.add_files(
            &["src/*", "*.rs"],
            &["2024 John Doe", "2024 Jane Doe"],
            &license,
        );

        let files: Vec<_> = copyright.iter_files().collect();
        assert_eq!(1, files.len());
        assert_eq!(vec!["src/*", "*.rs"], files[0].files());
        assert_eq!(vec!["2024 John Doe", "2024 Jane Doe"], files[0].copyright());
        assert_eq!("GPL-3+", files[0].license().unwrap().name().unwrap());

        // Verify the generated format
        assert_eq!(
            copyright.to_string(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
             Files: src/* *.rs\n\
             Copyright: 2024 John Doe\n           2024 Jane Doe\n\
             License: GPL-3+\n"
        );
    }

    #[test]
    fn test_add_files_with_license_text() {
        let mut copyright = super::Copyright::new();
        let license = crate::License::Named(
            "MIT".to_string(),
            "Permission is hereby granted...".to_string(),
        );
        copyright.add_files(&["*"], &["2024 Test Author"], &license);

        let files: Vec<_> = copyright.iter_files().collect();
        assert_eq!(1, files.len());
        assert_eq!("MIT", files[0].license().unwrap().name().unwrap());
        assert_eq!(
            "Permission is hereby granted...",
            files[0].license().unwrap().text().unwrap()
        );

        // Verify the generated format
        assert_eq!(
            copyright.to_string(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
             Files: *\n\
             Copyright: 2024 Test Author\n\
             License: MIT\n Permission is hereby granted...\n"
        );
    }

    #[test]
    fn test_add_files_with_multi_paragraph_license_text() {
        // add_files must encode blank lines in the license body as `.`
        // paragraph markers, matching add_license and set_license.
        let mut copyright = super::Copyright::new();
        let license = crate::License::Named(
            "GPL-2+".to_string(),
            "First paragraph.\n\nSecond paragraph.".to_string(),
        );
        copyright.add_files(&["*"], &["2024 Test Author"], &license);

        assert_eq!(
            copyright.to_string(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
             Files: *\n\
             Copyright: 2024 Test Author\n\
             License: GPL-2+\n First paragraph.\n .\n Second paragraph.\n"
        );

        // And the round trip back through license() must decode again.
        let files: Vec<_> = copyright.iter_files().collect();
        assert_eq!(
            files[0].license().unwrap().text(),
            Some("First paragraph.\n\nSecond paragraph.")
        );
    }

    #[test]
    fn test_add_license() {
        let mut copyright = super::Copyright::new();
        let license = crate::License::Named(
            "GPL-3+".to_string(),
            "This is the GPL-3+ license text.".to_string(),
        );
        copyright.add_license(&license);

        let licenses: Vec<_> = copyright.iter_licenses().collect();
        assert_eq!(1, licenses.len());
        assert_eq!("GPL-3+", licenses[0].name().unwrap());
        assert_eq!(
            "This is the GPL-3+ license text.",
            licenses[0].text().unwrap()
        );

        // Verify the generated format
        assert_eq!(
            copyright.to_string(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
             License: GPL-3+\n This is the GPL-3+ license text.\n"
        );
    }

    #[test]
    fn test_add_multiple_paragraphs() {
        let mut copyright = super::Copyright::new();

        // Add a files paragraph
        let license1 = crate::License::Name("MIT".to_string());
        copyright.add_files(&["src/*"], &["2024 Author One"], &license1);

        // Add another files paragraph
        let license2 = crate::License::Name("GPL-3+".to_string());
        copyright.add_files(&["debian/*"], &["2024 Author Two"], &license2);

        // Add a license paragraph
        let license3 =
            crate::License::Named("GPL-3+".to_string(), "Full GPL-3+ text here.".to_string());
        copyright.add_license(&license3);

        // Verify all paragraphs were added
        assert_eq!(2, copyright.iter_files().count());
        assert_eq!(1, copyright.iter_licenses().count());

        let files: Vec<_> = copyright.iter_files().collect();
        assert_eq!(vec!["src/*"], files[0].files());
        assert_eq!(vec!["debian/*"], files[1].files());

        let licenses: Vec<_> = copyright.iter_licenses().collect();
        assert_eq!("GPL-3+", licenses[0].name().unwrap());
        assert_eq!("Full GPL-3+ text here.", licenses[0].text().unwrap());

        // Verify the generated format
        assert_eq!(
            copyright.to_string(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
             Files: src/*\n\
             Copyright: 2024 Author One\n\
             License: MIT\n\n\
             Files: debian/*\n\
             Copyright: 2024 Author Two\n\
             License: GPL-3+\n\n\
             License: GPL-3+\n Full GPL-3+ text here.\n"
        );
    }

    #[test]
    fn test_remove_license_by_name() {
        let mut copyright = super::Copyright::new();

        // Add multiple license paragraphs
        let license1 = crate::License::Named("MIT".to_string(), "MIT license text.".to_string());
        copyright.add_license(&license1);

        let license2 =
            crate::License::Named("GPL-3+".to_string(), "GPL-3+ license text.".to_string());
        copyright.add_license(&license2);

        let license3 =
            crate::License::Named("Apache-2.0".to_string(), "Apache license text.".to_string());
        copyright.add_license(&license3);

        // Verify we have 3 license paragraphs
        assert_eq!(3, copyright.iter_licenses().count());

        // Remove the GPL-3+ license
        let removed = copyright.remove_license_by_name("GPL-3+");
        assert!(removed);

        // Verify we now have 2 license paragraphs
        assert_eq!(2, copyright.iter_licenses().count());

        // Verify the remaining licenses
        let licenses: Vec<_> = copyright.iter_licenses().collect();
        assert_eq!("MIT", licenses[0].name().unwrap());
        assert_eq!("Apache-2.0", licenses[1].name().unwrap());

        // Try to remove a non-existent license
        let removed = copyright.remove_license_by_name("BSD-3-Clause");
        assert!(!removed);
        assert_eq!(2, copyright.iter_licenses().count());
    }

    #[test]
    fn test_remove_files_by_pattern() {
        let mut copyright = super::Copyright::new();

        // Add multiple files paragraphs
        let license1 = crate::License::Name("MIT".to_string());
        copyright.add_files(&["src/*"], &["2024 Author One"], &license1);

        let license2 = crate::License::Name("GPL-3+".to_string());
        copyright.add_files(&["debian/*"], &["2024 Author Two"], &license2);

        let license3 = crate::License::Name("Apache-2.0".to_string());
        copyright.add_files(&["docs/*"], &["2024 Author Three"], &license3);

        // Verify we have 3 files paragraphs
        assert_eq!(3, copyright.iter_files().count());

        // Remove the debian/* files paragraph
        let removed = copyright.remove_files_by_pattern("debian/*");
        assert!(removed);

        // Verify we now have 2 files paragraphs
        assert_eq!(2, copyright.iter_files().count());

        // Verify the remaining files paragraphs
        let files: Vec<_> = copyright.iter_files().collect();
        assert_eq!(vec!["src/*"], files[0].files());
        assert_eq!(vec!["docs/*"], files[1].files());

        // Try to remove a non-existent pattern
        let removed = copyright.remove_files_by_pattern("tests/*");
        assert!(!removed);
        assert_eq!(2, copyright.iter_files().count());
    }

    #[test]
    fn test_remove_files_by_pattern_with_multiple_patterns() {
        let mut copyright = super::Copyright::new();

        // Add a files paragraph with multiple patterns
        let license = crate::License::Name("MIT".to_string());
        copyright.add_files(&["src/*", "*.rs"], &["2024 Author"], &license);

        // Verify we have 1 files paragraph
        assert_eq!(1, copyright.iter_files().count());

        // Remove by matching one of the patterns
        let removed = copyright.remove_files_by_pattern("*.rs");
        assert!(removed);

        // Verify the paragraph was removed
        assert_eq!(0, copyright.iter_files().count());
    }

    #[test]
    fn test_license_paragraph_set_name() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 This is the GPL-3+ license text.
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut license = copyright.iter_licenses().next().unwrap();

        // Change just the name, preserving the text
        license.set_name("Apache-2.0");

        assert_eq!(license.name().unwrap(), "Apache-2.0");
        assert_eq!(license.text().unwrap(), "This is the GPL-3+ license text.");
    }

    #[test]
    fn test_license_paragraph_set_name_no_text() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut license = copyright.iter_licenses().next().unwrap();

        // Change just the name when there's no text
        license.set_name("MIT");

        assert_eq!(license.license(), crate::License::Name("MIT".to_string()));
        assert_eq!(license.text(), None);
    }

    #[test]
    fn test_license_paragraph_set_text() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 Old license text.
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut license = copyright.iter_licenses().next().unwrap();

        // Change just the text, preserving the name
        license.set_text(Some("New license text."));

        assert_eq!(license.name().unwrap(), "GPL-3+");
        assert_eq!(license.text().unwrap(), "New license text.");
    }

    #[test]
    fn test_license_paragraph_set_text_remove() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 Old license text.
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut license = copyright.iter_licenses().next().unwrap();

        // Remove the text, keeping just the name
        license.set_text(None);

        assert_eq!(
            license.license(),
            crate::License::Name("GPL-3+".to_string())
        );
        assert_eq!(license.text(), None);
    }

    #[test]
    fn test_license_paragraph_set_text_add() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut license = copyright.iter_licenses().next().unwrap();

        // Add text to a name-only license
        license.set_text(Some("This is the full GPL-3+ license text."));

        assert_eq!(license.name().unwrap(), "GPL-3+");
        assert_eq!(
            license.text().unwrap(),
            "This is the full GPL-3+ license text."
        );
    }

    #[test]
    fn test_files_paragraph_set_files() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test Author
License: MIT
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut files = copyright.iter_files().next().unwrap();

        // Set new file patterns
        files.set_files(&["src/*", "*.rs", "tests/*"]);

        // Verify the files were updated
        assert_eq!(vec!["src/*", "*.rs", "tests/*"], files.files());
    }

    #[test]
    fn test_files_paragraph_add_file() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Test Author
License: MIT
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut files = copyright.iter_files().next().unwrap();

        // Add a new file pattern
        files.add_file("*.rs");
        assert_eq!(vec!["src/*", "*.rs"], files.files());

        // Add another pattern
        files.add_file("tests/*");
        assert_eq!(vec!["src/*", "*.rs", "tests/*"], files.files());

        // Try to add a duplicate - should not be added
        files.add_file("*.rs");
        assert_eq!(vec!["src/*", "*.rs", "tests/*"], files.files());
    }

    #[test]
    fn test_files_paragraph_remove_file() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/* *.rs tests/*
Copyright: 2024 Test Author
License: MIT
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let mut files = copyright.iter_files().next().unwrap();

        // Remove a file pattern
        let removed = files.remove_file("*.rs");
        assert!(removed);
        assert_eq!(vec!["src/*", "tests/*"], files.files());

        // Remove another pattern
        let removed = files.remove_file("tests/*");
        assert!(removed);
        assert_eq!(vec!["src/*"], files.files());

        // Try to remove a non-existent pattern
        let removed = files.remove_file("debian/*");
        assert!(!removed);
        assert_eq!(vec!["src/*"], files.files());
    }

    #[test]
    fn test_field_order_with_comment() {
        // Test that fields follow DEP-5 order: Files, Copyright, License, Comment
        let mut copyright = super::Copyright::new();

        let files = vec!["*"];
        let copyrights = vec!["Unknown"];
        let license = crate::License::Name("GPL-2+".to_string());

        let mut para = copyright.add_files(&files, &copyrights, &license);
        para.set_comment("Test comment");

        let output = copyright.to_string();

        // Expected order: Format, blank line, Files, Copyright, License, Comment
        let expected =
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
                        Files: *\n\
                        Copyright: Unknown\n\
                        License: GPL-2+\n\
                        Comment: Test comment\n";

        assert_eq!(
            output, expected,
            "Fields should be in DEP-5 order (Files, Copyright, License, Comment), but got:\n{}",
            output
        );
    }

    #[test]
    fn test_license_text_decoding_paragraph_markers() {
        // Test that paragraph markers (.) are decoded to blank lines
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: MIT
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files.
 .
 THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND.
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let license_para = copyright
            .iter_licenses()
            .next()
            .expect("no license paragraph");
        let text = license_para.text().expect("no license text");

        // The period marker should be decoded to a blank line
        assert!(
            text.contains("\n\n"),
            "Expected blank line in decoded text, got: {:?}",
            text
        );
        assert!(
            !text.contains("\n.\n"),
            "Period marker should be decoded, not present in output"
        );

        // Verify exact content
        let expected = "Permission is hereby granted, free of charge, to any person obtaining a copy\nof this software and associated documentation files.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND.";
        assert_eq!(text, expected);
    }

    #[test]
    fn test_license_enum_decoding() {
        // Test that the license() method also decodes paragraph markers
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 This program is free software.
 .
 You can redistribute it.
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let license_para = copyright
            .iter_licenses()
            .next()
            .expect("no license paragraph");
        let license = license_para.license();

        match license {
            crate::License::Named(name, text) => {
                assert_eq!(name, "GPL-3+");
                assert!(text.contains("\n\n"), "Expected blank line in decoded text");
                assert!(!text.contains("\n.\n"), "Period marker should be decoded");
                assert_eq!(
                    text,
                    "This program is free software.\n\nYou can redistribute it."
                );
            }
            _ => panic!("Expected Named license"),
        }
    }

    #[test]
    fn test_encode_field_text() {
        // Test basic encoding of blank lines
        let input = "line 1\n\nline 3";
        let output = super::encode_field_text(input);
        assert_eq!(output, "line 1\n.\nline 3");
    }

    #[test]
    fn test_encode_decode_round_trip() {
        // Test that encoding and decoding are inverse operations
        let original = "First paragraph\n\nSecond paragraph\n\nThird paragraph";
        let encoded = super::encode_field_text(original);
        let decoded = super::decode_field_text(&encoded);
        assert_eq!(
            decoded, original,
            "Round-trip encoding/decoding should preserve text"
        );
    }

    #[test]
    fn test_set_license_with_blank_lines() {
        // Test that setting a license with blank lines encodes them properly
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 Original text
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut license_para = copyright
            .iter_licenses()
            .next()
            .expect("no license paragraph");

        // Set license text with blank lines
        let new_license = crate::License::Named(
            "GPL-3+".to_string(),
            "First paragraph.\n\nSecond paragraph.".to_string(),
        );
        license_para.set_license(&new_license);

        // Verify it was encoded properly in the raw deb822
        let raw_text = copyright.to_string();
        let expected_output = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nLicense: GPL-3+\n First paragraph.\n .\n Second paragraph.\n";
        assert_eq!(raw_text, expected_output);

        // Verify it decodes back correctly
        let retrieved = license_para.text().expect("no text");
        assert_eq!(retrieved, "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn test_set_text_with_blank_lines() {
        // Test that set_text also encodes blank lines
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: MIT
 Original text
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut license_para = copyright
            .iter_licenses()
            .next()
            .expect("no license paragraph");

        // Set text with blank lines
        license_para.set_text(Some("Line 1\n\nLine 2"));

        // Verify encoding
        let raw_text = copyright.to_string();
        let expected_output = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nLicense: MIT\n Line 1\n .\n Line 2\n";
        assert_eq!(raw_text, expected_output);

        // Verify decoding
        let retrieved = license_para.text().expect("no text");
        assert_eq!(retrieved, "Line 1\n\nLine 2");
    }

    #[test]
    fn test_set_license_uses_single_space_indent_for_new_multiline() {
        // Test that set_license() uses 1-space indentation when converting
        // a single-line license (no existing indentation) to multi-line
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: Apache-2.0
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut license_para = copyright
            .iter_licenses()
            .next()
            .expect("no license paragraph");

        // Set new multi-line license text
        let new_license = crate::License::Named(
            "Apache-2.0".to_string(),
            "Licensed under the Apache License, Version 2.0".to_string(),
        );
        license_para.set_license(&new_license);

        // Verify the new license uses 1-space indentation
        let result = copyright.to_string();
        let expected = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nLicense: Apache-2.0\n Licensed under the Apache License, Version 2.0\n";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_header_as_deb822() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: foo
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let header = copyright.header().unwrap();
        let para = header.as_deb822();
        assert_eq!(para.get("Upstream-Name"), Some("foo".to_string()));
    }

    #[test]
    fn test_files_paragraph_as_deb822() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let files = copyright.iter_files().next().unwrap();
        let para = files.as_deb822();
        assert_eq!(para.get("Files"), Some("*".to_string()));
    }

    #[test]
    fn test_license_paragraph_as_deb822() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: GPL-3+
 License text
"#;
        let copyright = s.parse::<super::Copyright>().unwrap();
        let license = copyright.iter_licenses().next().unwrap();
        let para = license.as_deb822();
        assert!(para.get("License").unwrap().starts_with("GPL-3+"));
    }

    #[test]
    fn test_header_in_range() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example

Files: *
Copyright: 2024 Author
License: MIT
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");

        // Get the header's text range
        let header = copyright.header().unwrap();
        let header_range = header.as_deb822().text_range();

        // Query with the exact header range should return the header
        let result = copyright.header_in_range(header_range);
        assert!(result.is_some());
        assert_eq!(
            "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/",
            result.unwrap().format_string().unwrap()
        );

        // Query with a range that overlaps with the header
        let overlapping_range =
            TextRange::new(TextSize::from(0), header_range.end() - TextSize::from(10));
        let result = copyright.header_in_range(overlapping_range);
        assert!(result.is_some());

        // Query with a range completely outside the header should return None
        let files = copyright.iter_files().next().unwrap();
        let files_range = files.as_deb822().text_range();
        let result = copyright.header_in_range(files_range);
        assert!(result.is_none());
    }

    #[test]
    fn test_iter_files_in_range() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Main Author
License: GPL-3+

Files: src/*
Copyright: 2024 Author
License: MIT

Files: debian/*
Copyright: 2024 Debian Maintainer
License: GPL-3+
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");

        // Get all files paragraphs
        let all_files: Vec<_> = copyright.iter_files().collect();
        assert_eq!(3, all_files.len());

        // Query with the range of the second Files paragraph
        let second_range = all_files[1].as_deb822().text_range();
        let result: Vec<_> = copyright.iter_files_in_range(second_range).collect();
        assert_eq!(1, result.len());
        assert_eq!(vec!["src/*"], result[0].files());

        // Query with a range that spans the first two Files paragraphs
        let span_range = TextRange::new(
            all_files[0].as_deb822().text_range().start(),
            all_files[1].as_deb822().text_range().end(),
        );
        let result: Vec<_> = copyright.iter_files_in_range(span_range).collect();
        assert_eq!(2, result.len());
        assert_eq!(vec!["*"], result[0].files());
        assert_eq!(vec!["src/*"], result[1].files());

        // Query with a range that doesn't overlap with any Files paragraphs
        let header_range = copyright.header().unwrap().as_deb822().text_range();
        let result: Vec<_> = copyright.iter_files_in_range(header_range).collect();
        assert_eq!(0, result.len());
    }

    #[test]
    fn test_iter_licenses_in_range() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Author
License: MIT

License: MIT
 MIT license text here.

License: GPL-3+
 GPL license text here.
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");

        // Get all license paragraphs
        let all_licenses: Vec<_> = copyright.iter_licenses().collect();
        assert_eq!(2, all_licenses.len());

        // Query with the range of the first License paragraph
        let first_range = all_licenses[0].as_deb822().text_range();
        let result: Vec<_> = copyright.iter_licenses_in_range(first_range).collect();
        assert_eq!(1, result.len());
        assert_eq!(Some("MIT".to_string()), result[0].name());

        // Query with a range that spans both License paragraphs
        let span_range = TextRange::new(
            all_licenses[0].as_deb822().text_range().start(),
            all_licenses[1].as_deb822().text_range().end(),
        );
        let result: Vec<_> = copyright.iter_licenses_in_range(span_range).collect();
        assert_eq!(2, result.len());
        assert_eq!(Some("MIT".to_string()), result[0].name());
        assert_eq!(Some("GPL-3+".to_string()), result[1].name());

        // Query with a range that doesn't overlap with any License paragraphs (Files range)
        let files = copyright.iter_files().next().unwrap();
        let files_range = files.as_deb822().text_range();
        let result: Vec<_> = copyright.iter_licenses_in_range(files_range).collect();
        assert_eq!(0, result.len());
    }

    #[test]
    fn test_header_wrap_and_sort() {
        // Test that Header::wrap_and_sort() properly orders fields
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Comment: Some comment
Source: https://example.com
Upstream-Contact: John Doe
Upstream-Name: example
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut header = copyright.header().unwrap();

        header.wrap_and_sort(deb822_edit::Indentation::Spaces(1), false, None);

        // Verify the exact output with fields in HEADER_FIELD_ORDER
        let expected = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: example\nUpstream-Contact: John Doe\nSource: https://example.com\nComment: Some comment\n";
        assert_eq!(expected, header.0.to_string());
    }

    #[test]
    fn test_files_paragraph_wrap_and_sort_field_order() {
        // Test that FilesParagraph::wrap_and_sort() properly orders fields
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Comment: Some comment
License: MIT
Copyright: 2024 Author
Files: *
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut files = copyright.iter_files().next().unwrap();

        files.wrap_and_sort(deb822_edit::Indentation::Spaces(1), false, None);

        // Verify the exact output with fields in FILES_FIELD_ORDER
        let expected = "Files: *\nCopyright: 2024 Author\nLicense: MIT\nComment: Some comment\n";
        assert_eq!(expected, files.0.to_string());
    }

    #[test]
    fn test_files_paragraph_wrap_and_sort_patterns() {
        // Test that FilesParagraph::wrap_and_sort() properly sorts file patterns
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: debian/* src/foo/* * src/*
Copyright: 2024 Author
License: MIT
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut files = copyright.iter_files().next().unwrap();

        files.wrap_and_sort(deb822_edit::Indentation::Spaces(1), false, None);

        // Verify exact file pattern order
        assert_eq!(vec!["*", "src/*", "src/foo/*", "debian/*"], files.files());

        // Verify exact output
        let expected = "Files: * src/* src/foo/* debian/*\nCopyright: 2024 Author\nLicense: MIT\n";
        assert_eq!(expected, files.0.to_string());
    }

    #[test]
    fn test_license_paragraph_wrap_and_sort() {
        // Test that LicenseParagraph::wrap_and_sort() properly orders fields
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Comment: This is a comment
License: GPL-3+
 GPL license text here.
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut license = copyright.iter_licenses().next().unwrap();

        license.wrap_and_sort(deb822_edit::Indentation::Spaces(1), false, None);

        // Verify the exact output with fields in LICENSE_FIELD_ORDER
        let expected = "License: GPL-3+\n GPL license text here.\nComment: This is a comment\n";
        assert_eq!(expected, license.0.to_string());
    }

    #[test]
    fn test_copyright_wrap_and_sort() {
        // Test that Copyright::wrap_and_sort() properly sorts paragraphs and file patterns
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example

Files: debian/*
Copyright: 2024 Debian Maintainer
License: GPL-3+

License: GPL-3+
 GPL license text here.

Files: src/foo/* src/*
Copyright: 2024 Author
License: MIT

Files: *
Copyright: 2024 Main Author
License: GPL-3+

License: MIT
 MIT license text here.
"#;
        let mut copyright = s.parse::<super::Copyright>().expect("failed to parse");

        // Apply wrap and sort
        copyright.wrap_and_sort(deb822_edit::Indentation::Spaces(1), false, None);

        // Verify exact output with correct paragraph and field ordering
        let expected = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example

Files: *
Copyright: 2024 Main Author
License: GPL-3+

Files: src/* src/foo/*
Copyright: 2024 Author
License: MIT

Files: debian/*
Copyright: 2024 Debian Maintainer
License: GPL-3+

License: GPL-3+
 GPL license text here.

License: MIT
 MIT license text here.
"#;
        assert_eq!(expected, copyright.to_string());

        // Also verify via iteration
        let files: Vec<_> = copyright.iter_files().collect();
        assert_eq!(3, files.len());
        assert_eq!(vec!["*"], files[0].files());
        assert_eq!(vec!["src/*", "src/foo/*"], files[1].files());
        assert_eq!(vec!["debian/*"], files[2].files());

        let licenses: Vec<_> = copyright.iter_licenses().collect();
        assert_eq!(2, licenses.len());
    }

    #[test]
    fn test_copyright_wrap_and_sort_file_patterns_within_paragraph() {
        // Test that file patterns within a Files paragraph are sorted correctly
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: debian/* src/foo/* * src/*
Copyright: 2024 Author
License: MIT
"#;
        let mut copyright = s.parse::<super::Copyright>().expect("failed to parse");

        copyright.wrap_and_sort(deb822_edit::Indentation::Spaces(1), false, None);

        // Verify exact output with sorted patterns
        let expected = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: * src/* src/foo/* debian/*
Copyright: 2024 Author
License: MIT
"#;
        assert_eq!(expected, copyright.to_string());

        // Also verify via iteration
        let files: Vec<_> = copyright.iter_files().collect();
        assert_eq!(1, files.len());
        assert_eq!(
            vec!["*", "src/*", "src/foo/*", "debian/*"],
            files[0].files()
        );
    }

    #[test]
    fn test_files_included_none() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        assert_eq!(copyright.header().unwrap().files_included(), None);
    }

    #[test]
    fn test_files_included_single() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example
Files-Included: src/important.c

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        assert_eq!(
            copyright.header().unwrap().files_included(),
            Some(vec!["src/important.c".to_string()])
        );
    }

    #[test]
    fn test_files_included_multiple() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example
Files-Included: src/important.c
 src/also-important.c
 docs/*

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        assert_eq!(
            copyright.header().unwrap().files_included(),
            Some(vec![
                "src/important.c".to_string(),
                "src/also-important.c".to_string(),
                "docs/*".to_string(),
            ])
        );
    }

    #[test]
    fn test_set_files_included() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut header = copyright.header().unwrap();
        header.set_files_included(&["src/important.c", "docs/*"]);
        assert_eq!(
            header.files_included(),
            Some(vec!["src/important.c".to_string(), "docs/*".to_string(),])
        );
    }

    #[test]
    fn test_files_excluded_none() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        assert_eq!(copyright.header().unwrap().files_excluded(), None);
    }

    #[test]
    fn test_files_excluded_multiple() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example
Files-Excluded: vendor/*
 .github/*

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        assert_eq!(
            copyright.header().unwrap().files_excluded(),
            Some(vec!["vendor/*".to_string(), ".github/*".to_string(),])
        );
    }

    #[test]
    fn test_set_files_excluded() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut header = copyright.header().unwrap();
        header.set_files_excluded(&["vendor/*", ".github/*"]);
        assert_eq!(
            header.files_excluded(),
            Some(vec!["vendor/*".to_string(), ".github/*".to_string(),])
        );
    }

    #[test]
    fn test_is_file_included_no_exclusions() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let header = copyright.header().unwrap();
        assert!(header.is_file_included(std::path::Path::new("src/foo.c")));
        assert!(header.is_file_included(std::path::Path::new("vendor/lib.c")));
    }

    #[test]
    fn test_is_file_included_with_exclusion() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example
Files-Excluded: vendor/*

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let header = copyright.header().unwrap();
        assert!(header.is_file_included(std::path::Path::new("src/foo.c")));
        assert!(!header.is_file_included(std::path::Path::new("vendor/lib.c")));
    }

    #[test]
    fn test_is_file_included_with_reinclude() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: example
Files-Excluded: vendor/*
Files-Included: vendor/important.c

Files: *
License: GPL-3+
Copyright: 2019 John Doe
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let header = copyright.header().unwrap();
        assert!(header.is_file_included(std::path::Path::new("src/foo.c")));
        assert!(!header.is_file_included(std::path::Path::new("vendor/lib.c")));
        assert!(header.is_file_included(std::path::Path::new("vendor/important.c")));
    }

    #[test]
    fn test_set_license_normalizes_unusual_indentation() {
        // Regression test: set_license() should NOT preserve unusual indentation
        // from the original paragraph, it should always use 1-space indentation
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

License: Apache-2.0
                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/
 .
   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");
        let mut license_para = copyright
            .iter_licenses()
            .next()
            .expect("no license paragraph");

        // Set new license text with normal formatting (no unusual indentation)
        let new_text = "Licensed under the Apache License, Version 2.0 (the \"License\");\nyou may not use this file except in compliance with the License.\nYou may obtain a copy of the License at\n\nhttp://www.apache.org/licenses/LICENSE-2.0";
        let new_license = crate::License::Named("Apache-2.0".to_string(), new_text.to_string());
        license_para.set_license(&new_license);

        // Verify the output uses 1-space indentation, NOT the 33-space from the original
        let result = copyright.to_string();

        // The bug is now fixed - output uses 1-space indentation regardless of the original formatting
        let expected = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nLicense: Apache-2.0\n Licensed under the Apache License, Version 2.0 (the \"License\");\n you may not use this file except in compliance with the License.\n You may obtain a copy of the License at\n .\n http://www.apache.org/licenses/LICENSE-2.0\n";

        assert_eq!(result, expected);
    }
}

/// Thread-safe parse result for Copyright files, suitable for use in Salsa databases.
///
/// This type wraps `deb822_edit::Parse<Deb822>` for use in Salsa databases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse(deb822_edit::Parse<Deb822>);

impl From<deb822_edit::Parse<Deb822>> for Parse {
    fn from(parse: deb822_edit::Parse<Deb822>) -> Self {
        Parse(parse)
    }
}

impl Parse {
    /// Parse copyright text, returning a Parse result
    pub fn parse(text: &str) -> Self {
        Parse(Deb822::parse(text))
    }

    /// Parse copyright text relaxed (allows syntax errors)
    pub fn parse_relaxed(text: &str) -> Self {
        let deb822_parse = Deb822::parse(text);
        Parse(deb822_parse)
    }

    /// Get the syntax errors
    pub fn errors(&self) -> &[String] {
        self.0.errors()
    }

    /// Check if there are any errors
    pub fn ok(&self) -> bool {
        self.0.ok()
    }

    /// Get the parsed tree, even if there are errors
    ///
    /// Returns the Copyright object regardless of parse errors, allowing
    /// error-resilient tooling to work with partial/invalid input.
    pub fn tree(&self) -> Copyright {
        Copyright(self.0.tree())
    }

    /// Convert to a Copyright object
    ///
    /// Returns an empty Copyright if there are any parse errors.
    /// Prefer [`tree()`](Self::tree) which returns the parsed tree
    /// regardless of errors, allowing error-tolerant tooling.
    #[deprecated(
        since = "0.1.48",
        note = "use tree() instead, which preserves content even with parse errors"
    )]
    pub fn to_copyright(&self) -> Copyright {
        if let Ok(deb822) = self.0.clone().to_result() {
            Copyright(deb822)
        } else {
            // If there are parse errors, create an empty copyright
            Copyright(Deb822::new())
        }
    }

    /// Convert to a Result, returning the Copyright if there are no errors
    pub fn to_result(self) -> Result<Copyright, Error> {
        self.0.to_result().map(Copyright).map_err(Error::ParseError)
    }
}

// Implement Send + Sync since deb822_edit::Parse implements them
unsafe impl Send for Parse {}
unsafe impl Sync for Parse {}
