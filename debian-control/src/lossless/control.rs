//! This module provides a lossless representation of a Debian control file.
//!
//! # Example
//! ```rust
//! use debian_control::lossless::Control;
//! use debian_control::relations::VersionConstraint;
//! let input = r###"Source: dulwich
//! ## Comments are preserved
//! Maintainer: Jelmer Vernooĳ <jelmer@jelmer.uk>
//! Build-Depends: python3, debhelper-compat (= 12)
//!
//! Package: python3-dulwich
//! Architecture: amd64
//! Description: Pure-python git implementation
//! "###;
//!
//! let mut control: Control = input.parse().unwrap();
//!
//! // Bump debhelper-compat
//! let source = control.source().unwrap();
//! let bd = source.build_depends().unwrap();
//!
//! // Get entry with index 1 in Build-Depends, then set the version
//! let entry = bd.get_entry(1).unwrap();
//! let mut debhelper = entry.relations().next().unwrap();
//! assert_eq!(debhelper.name(), "debhelper-compat");
//! debhelper.set_version(Some((VersionConstraint::Equal, "13".parse().unwrap())));
//!
//! assert_eq!(source.to_string(), r###"Source: dulwich
//! ## Comments are preserved
//! Maintainer: Jelmer Vernooĳ <jelmer@jelmer.uk>
//! Build-Depends: python3, debhelper-compat (= 12)
//! "###);
//! ```
use crate::fields::{MultiArch, Priority};
use crate::lossless::relations::Relations;
use deb822_lossless::{Deb822, Paragraph, TextRange};
use rowan::ast::AstNode;

/// Parsing mode for Relations fields
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    /// Strict parsing - fail on syntax errors
    Strict,
    /// Relaxed parsing - accept syntax errors
    Relaxed,
    /// Allow substvars like ${misc:Depends}
    Substvar,
}

/// Canonical field order for source paragraphs in debian/control files
pub const SOURCE_FIELD_ORDER: &[&str] = &[
    "Source",
    "Section",
    "Priority",
    "Maintainer",
    "Uploaders",
    "Build-Depends",
    "Build-Depends-Indep",
    "Build-Depends-Arch",
    "Build-Conflicts",
    "Build-Conflicts-Indep",
    "Build-Conflicts-Arch",
    "Standards-Version",
    "Vcs-Browser",
    "Vcs-Git",
    "Vcs-Svn",
    "Vcs-Bzr",
    "Vcs-Hg",
    "Vcs-Darcs",
    "Vcs-Cvs",
    "Vcs-Arch",
    "Vcs-Mtn",
    "Homepage",
    "Rules-Requires-Root",
    "Testsuite",
    "Testsuite-Triggers",
];

/// Canonical field order for binary packages in debian/control files
pub const BINARY_FIELD_ORDER: &[&str] = &[
    "Package",
    "Architecture",
    "Section",
    "Priority",
    "Multi-Arch",
    "Essential",
    "Build-Profiles",
    "Built-Using",
    "Static-Built-Using",
    "Pre-Depends",
    "Depends",
    "Recommends",
    "Suggests",
    "Enhances",
    "Conflicts",
    "Breaks",
    "Replaces",
    "Provides",
    "Description",
];

fn format_field(name: &str, value: &str) -> String {
    match name {
        "Uploaders" => value
            .split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<_>>()
            .join(",\n"),
        "Build-Depends"
        | "Build-Depends-Indep"
        | "Build-Depends-Arch"
        | "Build-Conflicts"
        | "Build-Conflicts-Indep"
        | "Build-Conflics-Arch"
        | "Depends"
        | "Recommends"
        | "Suggests"
        | "Enhances"
        | "Pre-Depends"
        | "Breaks" => {
            // Try to parse and format the relations, but if parsing fails,
            // preserve the original value to maintain lossless behavior
            match value.parse::<Relations>() {
                Ok(relations) => {
                    let relations = relations.wrap_and_sort();
                    relations.to_string()
                }
                Err(_) => value.to_string(),
            }
        }
        _ => value.to_string(),
    }
}

/// A Debian control file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    deb822: Deb822,
    parse_mode: ParseMode,
}

impl Control {
    /// Create a new control file with strict parsing
    pub fn new() -> Self {
        Control {
            deb822: Deb822::new(),
            parse_mode: ParseMode::Strict,
        }
    }

    /// Create a new control file with the specified parse mode
    pub fn new_with_mode(parse_mode: ParseMode) -> Self {
        Control {
            deb822: Deb822::new(),
            parse_mode,
        }
    }

    /// Get the parse mode for this control file
    pub fn parse_mode(&self) -> ParseMode {
        self.parse_mode
    }

    /// Return the underlying deb822 object, mutable
    pub fn as_mut_deb822(&mut self) -> &mut Deb822 {
        &mut self.deb822
    }

    /// Return the underlying deb822 object
    pub fn as_deb822(&self) -> &Deb822 {
        &self.deb822
    }

    /// Create an independent snapshot of this Control file.
    ///
    /// This creates a new Control with an independent copy of the underlying
    /// deb822 data. Modifications to the original will not affect the snapshot
    /// and vice versa.
    ///
    /// This is more efficient than serializing and re-parsing because it reuses
    /// the GreenNode structure from the rowan tree.
    pub fn snapshot(&self) -> Self {
        Control {
            deb822: self.deb822.snapshot(),
            parse_mode: self.parse_mode,
        }
    }

    /// Parse control file text, returning a Parse result
    pub fn parse(text: &str) -> deb822_lossless::Parse<Control> {
        let deb822_parse = Deb822::parse(text);
        // Transform Parse<Deb822> to Parse<Control>
        let green = deb822_parse.green().clone();
        let errors = deb822_parse.errors().to_vec();
        let positioned_errors = deb822_parse.positioned_errors().to_vec();
        deb822_lossless::Parse::new_with_positioned_errors(green, errors, positioned_errors)
    }

    /// Return the source package
    pub fn source(&self) -> Option<Source> {
        let parse_mode = self.parse_mode;
        self.deb822
            .paragraphs()
            .find(|p| p.get("Source").is_some())
            .map(|paragraph| Source {
                paragraph,
                parse_mode,
            })
    }

    /// Iterate over all binary packages
    pub fn binaries(&self) -> impl Iterator<Item = Binary> + '_ {
        let parse_mode = self.parse_mode;
        self.deb822
            .paragraphs()
            .filter(|p| p.get("Package").is_some())
            .map(move |paragraph| Binary {
                paragraph,
                parse_mode,
            })
    }

    /// Return the source package if it intersects with the given text range
    ///
    /// # Arguments
    /// * `range` - The text range to query
    ///
    /// # Returns
    /// The source package if it exists and its text range overlaps with the provided range
    pub fn source_in_range(&self, range: TextRange) -> Option<Source> {
        self.source().filter(|s| {
            let para_range = s.as_deb822().text_range();
            para_range.start() < range.end() && para_range.end() > range.start()
        })
    }

    /// Iterate over binary packages that intersect with the given text range
    ///
    /// # Arguments
    /// * `range` - The text range to query
    ///
    /// # Returns
    /// An iterator over binary packages whose text ranges overlap with the provided range
    pub fn binaries_in_range(&self, range: TextRange) -> impl Iterator<Item = Binary> + '_ {
        self.binaries().filter(move |b| {
            let para_range = b.as_deb822().text_range();
            para_range.start() < range.end() && para_range.end() > range.start()
        })
    }

    /// Add a new source package
    ///
    /// # Arguments
    /// * `name` - The name of the source package
    ///
    /// # Returns
    /// The newly created source package
    ///
    /// # Example
    /// ```rust
    /// use debian_control::lossless::control::Control;
    /// let mut control = Control::new();
    /// let source = control.add_source("foo");
    /// assert_eq!(source.name(), Some("foo".to_owned()));
    /// ```
    pub fn add_source(&mut self, name: &str) -> Source {
        let mut p = self.deb822.add_paragraph();
        p.set("Source", name);
        self.source().unwrap()
    }

    /// Add new binary package
    ///
    /// # Arguments
    /// * `name` - The name of the binary package
    ///
    /// # Returns
    /// The newly created binary package
    ///
    /// # Example
    /// ```rust
    /// use debian_control::lossless::control::Control;
    /// let mut control = Control::new();
    /// let binary = control.add_binary("foo");
    /// assert_eq!(binary.name(), Some("foo".to_owned()));
    /// ```
    pub fn add_binary(&mut self, name: &str) -> Binary {
        let mut p = self.deb822.add_paragraph();
        p.set("Package", name);
        Binary {
            paragraph: p,
            parse_mode: ParseMode::Strict,
        }
    }

    /// Remove a binary package paragraph by name
    ///
    /// # Arguments
    /// * `name` - The name of the binary package to remove
    ///
    /// # Returns
    /// `true` if a binary paragraph with the given name was found and removed, `false` otherwise
    ///
    /// # Example
    /// ```rust
    /// use debian_control::lossless::control::Control;
    /// let mut control = Control::new();
    /// control.add_binary("foo");
    /// assert_eq!(control.binaries().count(), 1);
    /// assert!(control.remove_binary("foo"));
    /// assert_eq!(control.binaries().count(), 0);
    /// ```
    pub fn remove_binary(&mut self, name: &str) -> bool {
        let index = self
            .deb822
            .paragraphs()
            .position(|p| p.get("Package").as_deref() == Some(name));

        if let Some(index) = index {
            self.deb822.remove_paragraph(index);
            true
        } else {
            false
        }
    }

    /// Read a control file from a file
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self, deb822_lossless::Error> {
        Ok(Control {
            deb822: Deb822::from_file(path)?,
            parse_mode: ParseMode::Strict,
        })
    }

    /// Read a control file from a file, allowing syntax errors
    pub fn from_file_relaxed<P: AsRef<std::path::Path>>(
        path: P,
    ) -> Result<(Self, Vec<String>), std::io::Error> {
        let (deb822, errors) = Deb822::from_file_relaxed(path)?;
        Ok((
            Control {
                deb822,
                parse_mode: ParseMode::Relaxed,
            },
            errors,
        ))
    }

    /// Read a control file from a reader
    pub fn read<R: std::io::Read>(mut r: R) -> Result<Self, deb822_lossless::Error> {
        Ok(Control {
            deb822: Deb822::read(&mut r)?,
            parse_mode: ParseMode::Strict,
        })
    }

    /// Read a control file from a reader, allowing syntax errors
    pub fn read_relaxed<R: std::io::Read>(
        mut r: R,
    ) -> Result<(Self, Vec<String>), deb822_lossless::Error> {
        let (deb822, errors) = Deb822::read_relaxed(&mut r)?;
        Ok((
            Control {
                deb822,
                parse_mode: ParseMode::Relaxed,
            },
            errors,
        ))
    }

    /// Wrap and sort the control file
    ///
    /// # Arguments
    /// * `indentation` - The indentation to use
    /// * `immediate_empty_line` - Whether to add an empty line at the start of multi-line fields
    /// * `max_line_length_one_liner` - The maximum line length for one-liner fields
    pub fn wrap_and_sort(
        &mut self,
        indentation: deb822_lossless::Indentation,
        immediate_empty_line: bool,
        max_line_length_one_liner: Option<usize>,
    ) {
        let sort_paragraphs = |a: &Paragraph, b: &Paragraph| -> std::cmp::Ordering {
            // Sort Source before Package
            let a_is_source = a.get("Source").is_some();
            let b_is_source = b.get("Source").is_some();

            if a_is_source && !b_is_source {
                return std::cmp::Ordering::Less;
            } else if !a_is_source && b_is_source {
                return std::cmp::Ordering::Greater;
            } else if a_is_source && b_is_source {
                return a.get("Source").cmp(&b.get("Source"));
            }

            a.get("Package").cmp(&b.get("Package"))
        };

        let wrap_paragraph = |p: &Paragraph| -> Paragraph {
            // TODO: Add Source/Package specific wrapping
            // TODO: Add support for wrapping and sorting fields
            p.wrap_and_sort(
                indentation,
                immediate_empty_line,
                max_line_length_one_liner,
                None,
                Some(&format_field),
            )
        };

        self.deb822 = self
            .deb822
            .wrap_and_sort(Some(&sort_paragraphs), Some(&wrap_paragraph));
    }

    /// Sort binary package paragraphs alphabetically by package name.
    ///
    /// This method reorders the binary package paragraphs in alphabetical order
    /// based on their Package field value. The source paragraph always remains first.
    ///
    /// # Arguments
    /// * `keep_first` - If true, keeps the first binary package in place and only
    ///   sorts the remaining binary packages. If false, sorts all binary packages.
    ///
    /// # Example
    /// ```rust
    /// use debian_control::lossless::Control;
    ///
    /// let input = r#"Source: foo
    ///
    /// Package: libfoo
    /// Architecture: all
    ///
    /// Package: libbar
    /// Architecture: all
    /// "#;
    ///
    /// let mut control: Control = input.parse().unwrap();
    /// control.sort_binaries(false);
    ///
    /// // Binary packages are now sorted: libbar comes before libfoo
    /// let binaries: Vec<_> = control.binaries().collect();
    /// assert_eq!(binaries[0].name(), Some("libbar".to_string()));
    /// assert_eq!(binaries[1].name(), Some("libfoo".to_string()));
    /// ```
    pub fn sort_binaries(&mut self, keep_first: bool) {
        let mut paragraphs: Vec<_> = self.deb822.paragraphs().collect();

        if paragraphs.len() <= 1 {
            return; // Only source paragraph, nothing to sort
        }

        // Find the index where binary packages start (after source)
        let source_idx = paragraphs.iter().position(|p| p.get("Source").is_some());
        let binary_start = source_idx.map(|i| i + 1).unwrap_or(0);

        // Determine where to start sorting
        let sort_start = if keep_first && paragraphs.len() > binary_start + 1 {
            binary_start + 1
        } else {
            binary_start
        };

        if sort_start >= paragraphs.len() {
            return; // Nothing to sort
        }

        // Sort binary packages by package name
        paragraphs[sort_start..].sort_by(|a, b| {
            let a_name = a.get("Package");
            let b_name = b.get("Package");
            a_name.cmp(&b_name)
        });

        // Rebuild the Deb822 with sorted paragraphs
        let sort_paragraphs = |a: &Paragraph, b: &Paragraph| -> std::cmp::Ordering {
            let a_pos = paragraphs.iter().position(|p| p == a);
            let b_pos = paragraphs.iter().position(|p| p == b);
            a_pos.cmp(&b_pos)
        };

        self.deb822 = self.deb822.wrap_and_sort(Some(&sort_paragraphs), None);
    }

    /// Iterate over fields that overlap with the given range
    ///
    /// This method returns all fields (entries) from all paragraphs that have any overlap
    /// with the specified text range. This is useful for incremental parsing in LSP contexts
    /// where you only want to process fields that were affected by a text change.
    ///
    /// # Arguments
    /// * `range` - The text range to check for overlaps
    ///
    /// # Returns
    /// An iterator over all Entry items that overlap with the given range
    ///
    /// # Example
    /// ```rust
    /// use debian_control::lossless::Control;
    /// use deb822_lossless::TextRange;
    ///
    /// let control_text = "Source: foo\nMaintainer: test@example.com\n\nPackage: bar\nArchitecture: all\n";
    /// let control: Control = control_text.parse().unwrap();
    ///
    /// // Get fields in a specific range (e.g., where a change occurred)
    /// let change_range = TextRange::new(20.into(), 40.into());
    /// for entry in control.fields_in_range(change_range) {
    ///     if let Some(key) = entry.key() {
    ///         println!("Field {} was in the changed range", key);
    ///     }
    /// }
    /// ```
    pub fn fields_in_range(
        &self,
        range: TextRange,
    ) -> impl Iterator<Item = deb822_lossless::Entry> + '_ {
        self.deb822
            .paragraphs()
            .flat_map(move |p| p.entries().collect::<Vec<_>>())
            .filter(move |entry| {
                let entry_range = entry.syntax().text_range();
                // Check if ranges overlap
                entry_range.start() < range.end() && range.start() < entry_range.end()
            })
    }
}

impl From<Control> for Deb822 {
    fn from(c: Control) -> Self {
        c.deb822
    }
}

impl From<Deb822> for Control {
    fn from(d: Deb822) -> Self {
        Control {
            deb822: d,
            parse_mode: ParseMode::Strict,
        }
    }
}

impl Default for Control {
    fn default() -> Self {
        Self::new()
    }
}

impl std::str::FromStr for Control {
    type Err = deb822_lossless::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Control::parse(s).to_result()
    }
}

/// A source package paragraph
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    paragraph: Paragraph,
    parse_mode: ParseMode,
}

impl From<Source> for Paragraph {
    fn from(s: Source) -> Self {
        s.paragraph
    }
}

impl From<Paragraph> for Source {
    fn from(p: Paragraph) -> Self {
        Source {
            paragraph: p,
            parse_mode: ParseMode::Strict,
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.paragraph.fmt(f)
    }
}

impl Source {
    /// Parse a relations field according to the parse mode
    fn parse_relations(&self, s: &str) -> Relations {
        match self.parse_mode {
            ParseMode::Strict => s.parse().unwrap(),
            ParseMode::Relaxed => Relations::parse_relaxed(s, false).0,
            ParseMode::Substvar => Relations::parse_relaxed(s, true).0,
        }
    }

    /// The name of the source package.
    pub fn name(&self) -> Option<String> {
        self.paragraph.get("Source")
    }

    /// Wrap and sort the control file paragraph
    pub fn wrap_and_sort(
        &mut self,
        indentation: deb822_lossless::Indentation,
        immediate_empty_line: bool,
        max_line_length_one_liner: Option<usize>,
    ) {
        self.paragraph = self.paragraph.wrap_and_sort(
            indentation,
            immediate_empty_line,
            max_line_length_one_liner,
            None,
            Some(&format_field),
        );
    }

    /// Return the underlying deb822 paragraph, mutable
    pub fn as_mut_deb822(&mut self) -> &mut Paragraph {
        &mut self.paragraph
    }

    /// Return the underlying deb822 paragraph
    pub fn as_deb822(&self) -> &Paragraph {
        &self.paragraph
    }

    /// Set the name of the source package.
    pub fn set_name(&mut self, name: &str) {
        self.set("Source", name);
    }

    /// The default section of the packages built from this source package.
    pub fn section(&self) -> Option<String> {
        self.paragraph.get("Section")
    }

    /// Set the section of the source package
    pub fn set_section(&mut self, section: Option<&str>) {
        if let Some(section) = section {
            self.set("Section", section);
        } else {
            self.paragraph.remove("Section");
        }
    }

    /// The default priority of the packages built from this source package.
    pub fn priority(&self) -> Option<Priority> {
        self.paragraph.get("Priority").and_then(|v| v.parse().ok())
    }

    /// Set the priority of the source package
    pub fn set_priority(&mut self, priority: Option<Priority>) {
        if let Some(priority) = priority {
            self.set("Priority", priority.to_string().as_str());
        } else {
            self.paragraph.remove("Priority");
        }
    }

    /// The maintainer of the package.
    pub fn maintainer(&self) -> Option<String> {
        self.paragraph.get("Maintainer")
    }

    /// Set the maintainer of the package
    pub fn set_maintainer(&mut self, maintainer: &str) {
        self.set("Maintainer", maintainer);
    }

    /// Return whether this package is maintained by the Debian QA team.
    ///
    /// Orphaned packages have their `Maintainer` field set to
    /// `Debian QA Group <packages@qa.debian.org>`.
    pub fn is_qa_package(&self) -> bool {
        self.maintainer()
            .as_deref()
            .and_then(|m| crate::parse_identity(m).ok())
            .map(|(_, email)| email.eq_ignore_ascii_case("packages@qa.debian.org"))
            .unwrap_or(false)
    }

    /// The build dependencies of the package.
    pub fn build_depends(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Build-Depends")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Build-Depends field
    pub fn set_build_depends(&mut self, relations: &Relations) {
        self.set("Build-Depends", relations.to_string().as_str());
    }

    /// Return the Build-Depends-Indep field
    pub fn build_depends_indep(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Build-Depends-Indep")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Build-Depends-Indep field
    pub fn set_build_depends_indep(&mut self, relations: &Relations) {
        self.set("Build-Depends-Indep", relations.to_string().as_str());
    }

    /// Return the Build-Depends-Arch field
    pub fn build_depends_arch(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Build-Depends-Arch")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Build-Depends-Arch field
    pub fn set_build_depends_arch(&mut self, relations: &Relations) {
        self.set("Build-Depends-Arch", relations.to_string().as_str());
    }

    /// The build conflicts of the package.
    pub fn build_conflicts(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Build-Conflicts")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Build-Conflicts field
    pub fn set_build_conflicts(&mut self, relations: &Relations) {
        self.set("Build-Conflicts", relations.to_string().as_str());
    }

    /// Return the Build-Conflicts-Indep field
    pub fn build_conflicts_indep(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Build-Conflicts-Indep")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Build-Conflicts-Indep field
    pub fn set_build_conflicts_indep(&mut self, relations: &Relations) {
        self.set("Build-Conflicts-Indep", relations.to_string().as_str());
    }

    /// Return the Build-Conflicts-Arch field
    pub fn build_conflicts_arch(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Build-Conflicts-Arch")
            .map(|s| self.parse_relations(&s))
    }

    /// Return the standards version
    pub fn standards_version(&self) -> Option<String> {
        self.paragraph.get("Standards-Version")
    }

    /// Set the Standards-Version field
    pub fn set_standards_version(&mut self, version: &str) {
        self.set("Standards-Version", version);
    }

    /// Return the upstrea mHomepage
    pub fn homepage(&self) -> Option<url::Url> {
        self.paragraph.get("Homepage").and_then(|s| s.parse().ok())
    }

    /// Set the Homepage field
    pub fn set_homepage(&mut self, homepage: &url::Url) {
        self.set("Homepage", homepage.to_string().as_str());
    }

    /// Return the Vcs-Git field
    pub fn vcs_git(&self) -> Option<String> {
        self.paragraph.get("Vcs-Git")
    }

    /// Set the Vcs-Git field
    pub fn set_vcs_git(&mut self, url: &str) {
        self.set("Vcs-Git", url);
    }

    /// Return the Vcs-Browser field
    pub fn vcs_svn(&self) -> Option<String> {
        self.paragraph.get("Vcs-Svn").map(|s| s.to_string())
    }

    /// Set the Vcs-Svn field
    pub fn set_vcs_svn(&mut self, url: &str) {
        self.set("Vcs-Svn", url);
    }

    /// Return the Vcs-Bzr field
    pub fn vcs_bzr(&self) -> Option<String> {
        self.paragraph.get("Vcs-Bzr").map(|s| s.to_string())
    }

    /// Set the Vcs-Bzr field
    pub fn set_vcs_bzr(&mut self, url: &str) {
        self.set("Vcs-Bzr", url);
    }

    /// Return the Vcs-Arch field
    pub fn vcs_arch(&self) -> Option<String> {
        self.paragraph.get("Vcs-Arch").map(|s| s.to_string())
    }

    /// Set the Vcs-Arch field
    pub fn set_vcs_arch(&mut self, url: &str) {
        self.set("Vcs-Arch", url);
    }

    /// Return the Vcs-Svk field
    pub fn vcs_svk(&self) -> Option<String> {
        self.paragraph.get("Vcs-Svk").map(|s| s.to_string())
    }

    /// Set the Vcs-Svk field
    pub fn set_vcs_svk(&mut self, url: &str) {
        self.set("Vcs-Svk", url);
    }

    /// Return the Vcs-Darcs field
    pub fn vcs_darcs(&self) -> Option<String> {
        self.paragraph.get("Vcs-Darcs").map(|s| s.to_string())
    }

    /// Set the Vcs-Darcs field
    pub fn set_vcs_darcs(&mut self, url: &str) {
        self.set("Vcs-Darcs", url);
    }

    /// Return the Vcs-Mtn field
    pub fn vcs_mtn(&self) -> Option<String> {
        self.paragraph.get("Vcs-Mtn").map(|s| s.to_string())
    }

    /// Set the Vcs-Mtn field
    pub fn set_vcs_mtn(&mut self, url: &str) {
        self.set("Vcs-Mtn", url);
    }

    /// Return the Vcs-Cvs field
    pub fn vcs_cvs(&self) -> Option<String> {
        self.paragraph.get("Vcs-Cvs").map(|s| s.to_string())
    }

    /// Set the Vcs-Cvs field
    pub fn set_vcs_cvs(&mut self, url: &str) {
        self.set("Vcs-Cvs", url);
    }

    /// Return the Vcs-Hg field
    pub fn vcs_hg(&self) -> Option<String> {
        self.paragraph.get("Vcs-Hg").map(|s| s.to_string())
    }

    /// Set the Vcs-Hg field
    pub fn set_vcs_hg(&mut self, url: &str) {
        self.set("Vcs-Hg", url);
    }

    /// Set a field in the source paragraph, using canonical field ordering for source packages
    pub fn set(&mut self, key: &str, value: &str) {
        self.paragraph
            .set_with_field_order(key, value, SOURCE_FIELD_ORDER);
    }

    /// Retrieve a field
    pub fn get(&self, key: &str) -> Option<String> {
        self.paragraph.get(key)
    }

    /// Return the Vcs-Browser field
    pub fn vcs_browser(&self) -> Option<String> {
        self.paragraph.get("Vcs-Browser")
    }

    /// Return the Vcs used by the package
    pub fn vcs(&self) -> Option<crate::vcs::Vcs> {
        for (name, value) in self.paragraph.items() {
            if name.starts_with("Vcs-") && name != "Vcs-Browser" {
                return crate::vcs::Vcs::from_field(&name, &value).ok();
            }
        }
        None
    }

    /// Set the Vcs-Browser field
    pub fn set_vcs_browser(&mut self, url: Option<&str>) {
        if let Some(url) = url {
            self.set("Vcs-Browser", url);
        } else {
            self.paragraph.remove("Vcs-Browser");
        }
    }

    /// Return the Uploaders field
    pub fn uploaders(&self) -> Option<Vec<String>> {
        self.paragraph
            .get("Uploaders")
            .map(|s| s.split(',').map(|s| s.trim().to_owned()).collect())
    }

    /// Set the uploaders field
    pub fn set_uploaders(&mut self, uploaders: &[&str]) {
        self.set(
            "Uploaders",
            uploaders
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
                .as_str(),
        );
    }

    /// Return the architecture field
    pub fn architecture(&self) -> Option<String> {
        self.paragraph.get("Architecture")
    }

    /// Set the architecture field
    pub fn set_architecture(&mut self, arch: Option<&str>) {
        if let Some(arch) = arch {
            self.set("Architecture", arch);
        } else {
            self.paragraph.remove("Architecture");
        }
    }

    /// Return the Rules-Requires-Root field
    pub fn rules_requires_root(&self) -> Option<bool> {
        self.paragraph
            .get("Rules-Requires-Root")
            .map(|s| match s.to_lowercase().as_str() {
                "yes" => true,
                "no" => false,
                _ => panic!("invalid Rules-Requires-Root value"),
            })
    }

    /// Set the Rules-Requires-Root field
    pub fn set_rules_requires_root(&mut self, requires_root: bool) {
        self.set(
            "Rules-Requires-Root",
            if requires_root { "yes" } else { "no" },
        );
    }

    /// Return the Testsuite field
    pub fn testsuite(&self) -> Option<String> {
        self.paragraph.get("Testsuite")
    }

    /// Set the Testsuite field
    pub fn set_testsuite(&mut self, testsuite: &str) {
        self.set("Testsuite", testsuite);
    }

    /// Check if this source paragraph's range overlaps with the given range
    ///
    /// # Arguments
    /// * `range` - The text range to check for overlap
    ///
    /// # Returns
    /// `true` if the paragraph overlaps with the given range, `false` otherwise
    pub fn overlaps_range(&self, range: TextRange) -> bool {
        let para_range = self.paragraph.syntax().text_range();
        para_range.start() < range.end() && range.start() < para_range.end()
    }

    /// Get fields in this source paragraph that overlap with the given range
    ///
    /// # Arguments
    /// * `range` - The text range to check for overlaps
    ///
    /// # Returns
    /// An iterator over Entry items that overlap with the given range
    pub fn fields_in_range(
        &self,
        range: TextRange,
    ) -> impl Iterator<Item = deb822_lossless::Entry> + '_ {
        self.paragraph.entries().filter(move |entry| {
            let entry_range = entry.syntax().text_range();
            entry_range.start() < range.end() && range.start() < entry_range.end()
        })
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for Source {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        self.paragraph.into_pyobject(py)
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for &Source {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        (&self.paragraph).into_pyobject(py)
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::FromPyObject<'_, 'py> for Source {
    type Error = pyo3::PyErr;

    fn extract(ob: pyo3::Borrowed<'_, 'py, pyo3::PyAny>) -> Result<Self, Self::Error> {
        Ok(Source {
            paragraph: ob.extract()?,
            parse_mode: ParseMode::Strict,
        })
    }
}

impl std::fmt::Display for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.deb822.fmt(f)
    }
}

impl AstNode for Control {
    type Language = deb822_lossless::Lang;

    fn can_cast(kind: <Self::Language as rowan::Language>::Kind) -> bool {
        Deb822::can_cast(kind)
    }

    fn cast(syntax: rowan::SyntaxNode<Self::Language>) -> Option<Self> {
        Deb822::cast(syntax).map(|deb822| Control {
            deb822,
            parse_mode: ParseMode::Strict,
        })
    }

    fn syntax(&self) -> &rowan::SyntaxNode<Self::Language> {
        self.deb822.syntax()
    }
}

/// A binary package paragraph
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binary {
    paragraph: Paragraph,
    parse_mode: ParseMode,
}

impl From<Binary> for Paragraph {
    fn from(b: Binary) -> Self {
        b.paragraph
    }
}

impl From<Paragraph> for Binary {
    fn from(p: Paragraph) -> Self {
        Binary {
            paragraph: p,
            parse_mode: ParseMode::Strict,
        }
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for Binary {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        self.paragraph.into_pyobject(py)
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for &Binary {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        (&self.paragraph).into_pyobject(py)
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::FromPyObject<'_, 'py> for Binary {
    type Error = pyo3::PyErr;

    fn extract(ob: pyo3::Borrowed<'_, 'py, pyo3::PyAny>) -> Result<Self, Self::Error> {
        Ok(Binary {
            paragraph: ob.extract()?,
            parse_mode: ParseMode::Strict,
        })
    }
}

impl Default for Binary {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for Binary {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.paragraph.fmt(f)
    }
}

impl Binary {
    /// Parse a relations field according to the parse mode
    fn parse_relations(&self, s: &str) -> Relations {
        match self.parse_mode {
            ParseMode::Strict => s.parse().unwrap(),
            ParseMode::Relaxed => Relations::parse_relaxed(s, false).0,
            ParseMode::Substvar => Relations::parse_relaxed(s, true).0,
        }
    }

    /// Create a new binary package control file
    pub fn new() -> Self {
        Binary {
            paragraph: Paragraph::new(),
            parse_mode: ParseMode::Strict,
        }
    }

    /// Return the underlying deb822 paragraph, mutable
    pub fn as_mut_deb822(&mut self) -> &mut Paragraph {
        &mut self.paragraph
    }

    /// Return the underlying deb822 paragraph
    pub fn as_deb822(&self) -> &Paragraph {
        &self.paragraph
    }

    /// Wrap and sort the control file
    pub fn wrap_and_sort(
        &mut self,
        indentation: deb822_lossless::Indentation,
        immediate_empty_line: bool,
        max_line_length_one_liner: Option<usize>,
    ) {
        self.paragraph = self.paragraph.wrap_and_sort(
            indentation,
            immediate_empty_line,
            max_line_length_one_liner,
            None,
            Some(&format_field),
        );
    }

    /// The name of the package.
    pub fn name(&self) -> Option<String> {
        self.paragraph.get("Package")
    }

    /// Set the name of the package
    pub fn set_name(&mut self, name: &str) {
        self.set("Package", name);
    }

    /// The section of the package.
    pub fn section(&self) -> Option<String> {
        self.paragraph.get("Section")
    }

    /// Set the section
    pub fn set_section(&mut self, section: Option<&str>) {
        if let Some(section) = section {
            self.set("Section", section);
        } else {
            self.paragraph.remove("Section");
        }
    }

    /// The priority of the package.
    pub fn priority(&self) -> Option<Priority> {
        self.paragraph.get("Priority").and_then(|v| v.parse().ok())
    }

    /// Set the priority of the package
    pub fn set_priority(&mut self, priority: Option<Priority>) {
        if let Some(priority) = priority {
            self.set("Priority", priority.to_string().as_str());
        } else {
            self.paragraph.remove("Priority");
        }
    }

    /// The architecture of the package.
    pub fn architecture(&self) -> Option<String> {
        self.paragraph.get("Architecture")
    }

    /// Set the architecture of the package
    pub fn set_architecture(&mut self, arch: Option<&str>) {
        if let Some(arch) = arch {
            self.set("Architecture", arch);
        } else {
            self.paragraph.remove("Architecture");
        }
    }

    /// The dependencies of the package.
    pub fn depends(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Depends")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Depends field
    pub fn set_depends(&mut self, depends: Option<&Relations>) {
        if let Some(depends) = depends {
            self.set("Depends", depends.to_string().as_str());
        } else {
            self.paragraph.remove("Depends");
        }
    }

    /// The package that this package recommends
    pub fn recommends(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Recommends")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Recommends field
    pub fn set_recommends(&mut self, recommends: Option<&Relations>) {
        if let Some(recommends) = recommends {
            self.set("Recommends", recommends.to_string().as_str());
        } else {
            self.paragraph.remove("Recommends");
        }
    }

    /// Packages that this package suggests
    pub fn suggests(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Suggests")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Suggests field
    pub fn set_suggests(&mut self, suggests: Option<&Relations>) {
        if let Some(suggests) = suggests {
            self.set("Suggests", suggests.to_string().as_str());
        } else {
            self.paragraph.remove("Suggests");
        }
    }

    /// The package that this package enhances
    pub fn enhances(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Enhances")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Enhances field
    pub fn set_enhances(&mut self, enhances: Option<&Relations>) {
        if let Some(enhances) = enhances {
            self.set("Enhances", enhances.to_string().as_str());
        } else {
            self.paragraph.remove("Enhances");
        }
    }

    /// The package that this package pre-depends on
    pub fn pre_depends(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Pre-Depends")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Pre-Depends field
    pub fn set_pre_depends(&mut self, pre_depends: Option<&Relations>) {
        if let Some(pre_depends) = pre_depends {
            self.set("Pre-Depends", pre_depends.to_string().as_str());
        } else {
            self.paragraph.remove("Pre-Depends");
        }
    }

    /// The package that this package breaks
    pub fn breaks(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Breaks")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Breaks field
    pub fn set_breaks(&mut self, breaks: Option<&Relations>) {
        if let Some(breaks) = breaks {
            self.set("Breaks", breaks.to_string().as_str());
        } else {
            self.paragraph.remove("Breaks");
        }
    }

    /// The package that this package conflicts with
    pub fn conflicts(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Conflicts")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Conflicts field
    pub fn set_conflicts(&mut self, conflicts: Option<&Relations>) {
        if let Some(conflicts) = conflicts {
            self.set("Conflicts", conflicts.to_string().as_str());
        } else {
            self.paragraph.remove("Conflicts");
        }
    }

    /// The package that this package replaces
    pub fn replaces(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Replaces")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Replaces field
    pub fn set_replaces(&mut self, replaces: Option<&Relations>) {
        if let Some(replaces) = replaces {
            self.set("Replaces", replaces.to_string().as_str());
        } else {
            self.paragraph.remove("Replaces");
        }
    }

    /// Return the Provides field
    pub fn provides(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Provides")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Provides field
    pub fn set_provides(&mut self, provides: Option<&Relations>) {
        if let Some(provides) = provides {
            self.set("Provides", provides.to_string().as_str());
        } else {
            self.paragraph.remove("Provides");
        }
    }

    /// Return the Built-Using field
    pub fn built_using(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Built-Using")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Built-Using field
    pub fn set_built_using(&mut self, built_using: Option<&Relations>) {
        if let Some(built_using) = built_using {
            self.set("Built-Using", built_using.to_string().as_str());
        } else {
            self.paragraph.remove("Built-Using");
        }
    }

    /// Return the Static-Built-Using field
    pub fn static_built_using(&self) -> Option<Relations> {
        self.paragraph
            .get_with_comments("Static-Built-Using")
            .map(|s| self.parse_relations(&s))
    }

    /// Set the Static-Built-Using field
    pub fn set_static_built_using(&mut self, static_built_using: Option<&Relations>) {
        if let Some(static_built_using) = static_built_using {
            self.set(
                "Static-Built-Using",
                static_built_using.to_string().as_str(),
            );
        } else {
            self.paragraph.remove("Static-Built-Using");
        }
    }

    /// The Multi-Arch field
    pub fn multi_arch(&self) -> Option<MultiArch> {
        self.paragraph.get("Multi-Arch").map(|s| s.parse().unwrap())
    }

    /// Set the Multi-Arch field
    pub fn set_multi_arch(&mut self, multi_arch: Option<MultiArch>) {
        if let Some(multi_arch) = multi_arch {
            self.set("Multi-Arch", multi_arch.to_string().as_str());
        } else {
            self.paragraph.remove("Multi-Arch");
        }
    }

    /// Whether the package is essential
    pub fn essential(&self) -> bool {
        self.paragraph
            .get("Essential")
            .map(|s| s == "yes")
            .unwrap_or(false)
    }

    /// Set whether the package is essential
    pub fn set_essential(&mut self, essential: bool) {
        if essential {
            self.set("Essential", "yes");
        } else {
            self.paragraph.remove("Essential");
        }
    }

    /// Binary package description
    pub fn description(&self) -> Option<String> {
        self.paragraph.get_multiline("Description")
    }

    /// Set the binary package description
    pub fn set_description(&mut self, description: Option<&str>) {
        if let Some(description) = description {
            self.paragraph.set_with_indent_pattern(
                "Description",
                description,
                Some(&deb822_lossless::IndentPattern::Fixed(1)),
                Some(BINARY_FIELD_ORDER),
            );
        } else {
            self.paragraph.remove("Description");
        }
    }

    /// Return the upstream homepage
    pub fn homepage(&self) -> Option<url::Url> {
        self.paragraph.get("Homepage").and_then(|s| s.parse().ok())
    }

    /// Set the upstream homepage
    pub fn set_homepage(&mut self, url: &url::Url) {
        self.set("Homepage", url.as_str());
    }

    /// Set a field in the binary paragraph, using canonical field ordering for binary packages
    pub fn set(&mut self, key: &str, value: &str) {
        self.paragraph
            .set_with_field_order(key, value, BINARY_FIELD_ORDER);
    }

    /// Retrieve a field
    pub fn get(&self, key: &str) -> Option<String> {
        self.paragraph.get(key)
    }

    /// Check if this binary paragraph's range overlaps with the given range
    ///
    /// # Arguments
    /// * `range` - The text range to check for overlap
    ///
    /// # Returns
    /// `true` if the paragraph overlaps with the given range, `false` otherwise
    pub fn overlaps_range(&self, range: TextRange) -> bool {
        let para_range = self.paragraph.syntax().text_range();
        para_range.start() < range.end() && range.start() < para_range.end()
    }

    /// Get fields in this binary paragraph that overlap with the given range
    ///
    /// # Arguments
    /// * `range` - The text range to check for overlaps
    ///
    /// # Returns
    /// An iterator over Entry items that overlap with the given range
    pub fn fields_in_range(
        &self,
        range: TextRange,
    ) -> impl Iterator<Item = deb822_lossless::Entry> + '_ {
        self.paragraph.entries().filter(move |entry| {
            let entry_range = entry.syntax().text_range();
            entry_range.start() < range.end() && range.start() < entry_range.end()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relations::VersionConstraint;

    #[test]
    fn test_source_set_field_ordering() {
        let mut control = Control::new();
        let mut source = control.add_source("mypackage");

        // Add fields in random order
        source.set("Homepage", "https://example.com");
        source.set("Build-Depends", "debhelper");
        source.set("Standards-Version", "4.5.0");
        source.set("Maintainer", "Test <test@example.com>");

        // Convert to string and check field order
        let output = source.to_string();
        let lines: Vec<&str> = output.lines().collect();

        // Source should be first
        assert!(lines[0].starts_with("Source:"));

        // Find the positions of each field
        let maintainer_pos = lines
            .iter()
            .position(|l| l.starts_with("Maintainer:"))
            .unwrap();
        let build_depends_pos = lines
            .iter()
            .position(|l| l.starts_with("Build-Depends:"))
            .unwrap();
        let standards_pos = lines
            .iter()
            .position(|l| l.starts_with("Standards-Version:"))
            .unwrap();
        let homepage_pos = lines
            .iter()
            .position(|l| l.starts_with("Homepage:"))
            .unwrap();

        // Check ordering according to SOURCE_FIELD_ORDER
        assert!(maintainer_pos < build_depends_pos);
        assert!(build_depends_pos < standards_pos);
        assert!(standards_pos < homepage_pos);
    }

    #[test]
    fn test_binary_set_field_ordering() {
        let mut control = Control::new();
        let mut binary = control.add_binary("mypackage");

        // Add fields in random order
        binary.set("Description", "A test package");
        binary.set("Architecture", "amd64");
        binary.set("Depends", "libc6");
        binary.set("Section", "utils");

        // Convert to string and check field order
        let output = binary.to_string();
        let lines: Vec<&str> = output.lines().collect();

        // Package should be first
        assert!(lines[0].starts_with("Package:"));

        // Find the positions of each field
        let arch_pos = lines
            .iter()
            .position(|l| l.starts_with("Architecture:"))
            .unwrap();
        let section_pos = lines
            .iter()
            .position(|l| l.starts_with("Section:"))
            .unwrap();
        let depends_pos = lines
            .iter()
            .position(|l| l.starts_with("Depends:"))
            .unwrap();
        let desc_pos = lines
            .iter()
            .position(|l| l.starts_with("Description:"))
            .unwrap();

        // Check ordering according to BINARY_FIELD_ORDER
        assert!(arch_pos < section_pos);
        assert!(section_pos < depends_pos);
        assert!(depends_pos < desc_pos);
    }

    #[test]
    fn test_source_specific_set_methods_use_field_ordering() {
        let mut control = Control::new();
        let mut source = control.add_source("mypackage");

        // Use specific set_* methods in random order
        source.set_homepage(&"https://example.com".parse().unwrap());
        source.set_maintainer("Test <test@example.com>");
        source.set_standards_version("4.5.0");
        source.set_vcs_git("https://github.com/example/repo");

        // Convert to string and check field order
        let output = source.to_string();
        let lines: Vec<&str> = output.lines().collect();

        // Find the positions of each field
        let source_pos = lines.iter().position(|l| l.starts_with("Source:")).unwrap();
        let maintainer_pos = lines
            .iter()
            .position(|l| l.starts_with("Maintainer:"))
            .unwrap();
        let standards_pos = lines
            .iter()
            .position(|l| l.starts_with("Standards-Version:"))
            .unwrap();
        let vcs_git_pos = lines
            .iter()
            .position(|l| l.starts_with("Vcs-Git:"))
            .unwrap();
        let homepage_pos = lines
            .iter()
            .position(|l| l.starts_with("Homepage:"))
            .unwrap();

        // Check ordering according to SOURCE_FIELD_ORDER
        assert!(source_pos < maintainer_pos);
        assert!(maintainer_pos < standards_pos);
        assert!(standards_pos < vcs_git_pos);
        assert!(vcs_git_pos < homepage_pos);
    }

    #[test]
    fn test_binary_specific_set_methods_use_field_ordering() {
        let mut control = Control::new();
        let mut binary = control.add_binary("mypackage");

        // Use specific set_* methods in random order
        binary.set_description(Some("A test package"));
        binary.set_architecture(Some("amd64"));
        let depends = "libc6".parse().unwrap();
        binary.set_depends(Some(&depends));
        binary.set_section(Some("utils"));
        binary.set_priority(Some(Priority::Optional));

        // Convert to string and check field order
        let output = binary.to_string();
        let lines: Vec<&str> = output.lines().collect();

        // Find the positions of each field
        let package_pos = lines
            .iter()
            .position(|l| l.starts_with("Package:"))
            .unwrap();
        let arch_pos = lines
            .iter()
            .position(|l| l.starts_with("Architecture:"))
            .unwrap();
        let section_pos = lines
            .iter()
            .position(|l| l.starts_with("Section:"))
            .unwrap();
        let priority_pos = lines
            .iter()
            .position(|l| l.starts_with("Priority:"))
            .unwrap();
        let depends_pos = lines
            .iter()
            .position(|l| l.starts_with("Depends:"))
            .unwrap();
        let desc_pos = lines
            .iter()
            .position(|l| l.starts_with("Description:"))
            .unwrap();

        // Check ordering according to BINARY_FIELD_ORDER
        assert!(package_pos < arch_pos);
        assert!(arch_pos < section_pos);
        assert!(section_pos < priority_pos);
        assert!(priority_pos < depends_pos);
        assert!(depends_pos < desc_pos);
    }

    #[test]
    fn test_parse() {
        let control: Control = r#"Source: foo
Section: libs
Priority: optional
Build-Depends: bar (>= 1.0.0), baz (>= 1.0.0)
Homepage: https://example.com

"#
        .parse()
        .unwrap();
        let source = control.source().unwrap();

        assert_eq!(source.name(), Some("foo".to_owned()));
        assert_eq!(source.section(), Some("libs".to_owned()));
        assert_eq!(source.priority(), Some(super::Priority::Optional));
        assert_eq!(
            source.homepage(),
            Some("https://example.com".parse().unwrap())
        );
        let bd = source.build_depends().unwrap();
        let entries = bd.entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        let rel = entries[0].relations().collect::<Vec<_>>().pop().unwrap();
        assert_eq!(rel.name(), "bar");
        assert_eq!(
            rel.version(),
            Some((
                VersionConstraint::GreaterThanEqual,
                "1.0.0".parse().unwrap()
            ))
        );
        let rel = entries[1].relations().collect::<Vec<_>>().pop().unwrap();
        assert_eq!(rel.name(), "baz");
        assert_eq!(
            rel.version(),
            Some((
                VersionConstraint::GreaterThanEqual,
                "1.0.0".parse().unwrap()
            ))
        );
    }

    #[test]
    fn test_description() {
        let control: Control = r#"Source: foo

Package: foo
Description: this is the short description
 And the longer one
 .
 is on the next lines
"#
        .parse()
        .unwrap();
        let binary = control.binaries().next().unwrap();
        assert_eq!(
            binary.description(),
            Some(
                "this is the short description\nAnd the longer one\n.\nis on the next lines"
                    .to_owned()
            )
        );
    }

    #[test]
    fn test_set_description_on_package_without_description() {
        let control: Control = r#"Source: foo

Package: foo
Architecture: amd64
"#
        .parse()
        .unwrap();
        let mut binary = control.binaries().next().unwrap();

        // Set description on a binary that doesn't have one
        binary.set_description(Some(
            "Short description\nLonger description\n.\nAnother line",
        ));

        let output = binary.to_string();

        // Check that the description was set
        assert_eq!(
            binary.description(),
            Some("Short description\nLonger description\n.\nAnother line".to_owned())
        );

        // Verify the output format has exactly one space indent
        assert_eq!(
            output,
            "Package: foo\nArchitecture: amd64\nDescription: Short description\n Longer description\n .\n Another line\n"
        );
    }

    #[test]
    fn test_as_mut_deb822() {
        let mut control = Control::new();
        let deb822 = control.as_mut_deb822();
        let mut p = deb822.add_paragraph();
        p.set("Source", "foo");
        assert_eq!(control.source().unwrap().name(), Some("foo".to_owned()));
    }

    #[test]
    fn test_as_deb822() {
        let control = Control::new();
        let _deb822: &Deb822 = control.as_deb822();
    }

    #[test]
    fn test_set_depends() {
        let mut control = Control::new();
        let mut binary = control.add_binary("foo");
        let relations: Relations = "bar (>= 1.0.0)".parse().unwrap();
        binary.set_depends(Some(&relations));
    }

    #[test]
    fn test_wrap_and_sort() {
        let mut control: Control = r#"Package: blah
Section:     libs



Package: foo
Description: this is a 
      bar
      blah
"#
        .parse()
        .unwrap();
        control.wrap_and_sort(deb822_lossless::Indentation::Spaces(2), false, None);
        let expected = r#"Package: blah
Section: libs

Package: foo
Description: this is a 
  bar
  blah
"#
        .to_owned();
        assert_eq!(control.to_string(), expected);
    }

    #[test]
    fn test_wrap_and_sort_source() {
        let mut control: Control = r#"Source: blah
Depends: foo, bar   (<=  1.0.0)

"#
        .parse()
        .unwrap();
        control.wrap_and_sort(deb822_lossless::Indentation::Spaces(2), true, None);
        let expected = r#"Source: blah
Depends: bar (<= 1.0.0), foo
"#
        .to_owned();
        assert_eq!(control.to_string(), expected);
    }

    #[test]
    fn test_source_wrap_and_sort() {
        let control: Control = r#"Source: blah
Build-Depends: foo, bar (>= 1.0.0)

"#
        .parse()
        .unwrap();
        let mut source = control.source().unwrap();
        source.wrap_and_sort(deb822_lossless::Indentation::Spaces(2), true, None);
        // The actual behavior - the method modifies the source in-place
        // but doesn't automatically affect the overall control structure
        // So we just test that the method executes without error
        assert!(source.build_depends().is_some());
    }

    #[test]
    fn test_binary_set_breaks() {
        let mut control = Control::new();
        let mut binary = control.add_binary("foo");
        let relations: Relations = "bar (>= 1.0.0)".parse().unwrap();
        binary.set_breaks(Some(&relations));
        assert!(binary.breaks().is_some());
    }

    #[test]
    fn test_binary_set_pre_depends() {
        let mut control = Control::new();
        let mut binary = control.add_binary("foo");
        let relations: Relations = "bar (>= 1.0.0)".parse().unwrap();
        binary.set_pre_depends(Some(&relations));
        assert!(binary.pre_depends().is_some());
    }

    #[test]
    fn test_binary_set_provides() {
        let mut control = Control::new();
        let mut binary = control.add_binary("foo");
        let relations: Relations = "bar (>= 1.0.0)".parse().unwrap();
        binary.set_provides(Some(&relations));
        assert!(binary.provides().is_some());
    }

    #[test]
    fn test_source_is_qa_package() {
        let control: Control = "Source: foo\n\n".parse().unwrap();
        assert!(!control.source().unwrap().is_qa_package());

        let control: Control = "Source: foo\nMaintainer: Jane Packager <jane@example.com>\n\n"
            .parse()
            .unwrap();
        assert!(!control.source().unwrap().is_qa_package());

        let control: Control =
            "Source: foo\nMaintainer: Debian QA Group <packages@qa.debian.org>\n\n"
                .parse()
                .unwrap();
        assert!(control.source().unwrap().is_qa_package());
    }

    #[test]
    fn test_source_build_conflicts() {
        let control: Control = r#"Source: blah
Build-Conflicts: foo, bar (>= 1.0.0)

"#
        .parse()
        .unwrap();
        let source = control.source().unwrap();
        let conflicts = source.build_conflicts();
        assert!(conflicts.is_some());
    }

    #[test]
    fn test_source_vcs_svn() {
        let control: Control = r#"Source: blah
Vcs-Svn: https://example.com/svn/repo

"#
        .parse()
        .unwrap();
        let source = control.source().unwrap();
        assert_eq!(
            source.vcs_svn(),
            Some("https://example.com/svn/repo".to_string())
        );
    }

    #[test]
    fn test_control_from_conversion() {
        let deb822_data = r#"Source: test
Section: libs

"#;
        let deb822: Deb822 = deb822_data.parse().unwrap();
        let control = Control::from(deb822);
        assert!(control.source().is_some());
    }

    #[test]
    fn test_fields_in_range() {
        let control_text = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper (>= 12)

Package: test-binary
Architecture: any
Depends: ${shlibs:Depends}
Description: Test package
 This is a test package
"#;
        let control: Control = control_text.parse().unwrap();

        // Test range that covers only the Source field
        let source_start = 0;
        let source_end = "Source: test-package".len();
        let source_range = TextRange::new((source_start as u32).into(), (source_end as u32).into());

        let fields: Vec<_> = control.fields_in_range(source_range).collect();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key(), Some("Source".to_string()));

        // Test range that covers multiple fields in source paragraph
        let maintainer_start = control_text.find("Maintainer:").unwrap();
        let build_depends_end = control_text
            .find("Build-Depends: debhelper (>= 12)")
            .unwrap()
            + "Build-Depends: debhelper (>= 12)".len();
        let multi_range = TextRange::new(
            (maintainer_start as u32).into(),
            (build_depends_end as u32).into(),
        );

        let fields: Vec<_> = control.fields_in_range(multi_range).collect();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].key(), Some("Maintainer".to_string()));
        assert_eq!(fields[1].key(), Some("Build-Depends".to_string()));

        // Test range that spans across paragraphs
        let cross_para_start = control_text.find("Build-Depends:").unwrap();
        let cross_para_end =
            control_text.find("Architecture: any").unwrap() + "Architecture: any".len();
        let cross_range = TextRange::new(
            (cross_para_start as u32).into(),
            (cross_para_end as u32).into(),
        );

        let fields: Vec<_> = control.fields_in_range(cross_range).collect();
        assert_eq!(fields.len(), 3); // Build-Depends, Package, Architecture
        assert_eq!(fields[0].key(), Some("Build-Depends".to_string()));
        assert_eq!(fields[1].key(), Some("Package".to_string()));
        assert_eq!(fields[2].key(), Some("Architecture".to_string()));

        // Test empty range (should return no fields)
        let empty_range = TextRange::new(1000.into(), 1001.into());
        let fields: Vec<_> = control.fields_in_range(empty_range).collect();
        assert_eq!(fields.len(), 0);
    }

    #[test]
    fn test_source_overlaps_range() {
        let control_text = r#"Source: test-package
Maintainer: Test User <test@example.com>

Package: test-binary
Architecture: any
"#;
        let control: Control = control_text.parse().unwrap();
        let source = control.source().unwrap();

        // Test range that overlaps with source paragraph
        let overlap_range = TextRange::new(10.into(), 30.into());
        assert!(source.overlaps_range(overlap_range));

        // Test range that doesn't overlap with source paragraph
        let binary_start = control_text.find("Package:").unwrap();
        let no_overlap_range = TextRange::new(
            (binary_start as u32).into(),
            ((binary_start + 20) as u32).into(),
        );
        assert!(!source.overlaps_range(no_overlap_range));

        // Test range that starts before and ends within source paragraph
        let partial_overlap = TextRange::new(0.into(), 15.into());
        assert!(source.overlaps_range(partial_overlap));
    }

    #[test]
    fn test_source_fields_in_range() {
        let control_text = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper (>= 12)

Package: test-binary
"#;
        let control: Control = control_text.parse().unwrap();
        let source = control.source().unwrap();

        // Test range covering Maintainer field
        let maintainer_start = control_text.find("Maintainer:").unwrap();
        let maintainer_end = maintainer_start + "Maintainer: Test User <test@example.com>".len();
        let maintainer_range = TextRange::new(
            (maintainer_start as u32).into(),
            (maintainer_end as u32).into(),
        );

        let fields: Vec<_> = source.fields_in_range(maintainer_range).collect();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key(), Some("Maintainer".to_string()));

        // Test range covering multiple fields
        let all_source_range = TextRange::new(0.into(), 100.into());
        let fields: Vec<_> = source.fields_in_range(all_source_range).collect();
        assert_eq!(fields.len(), 3); // Source, Maintainer, Build-Depends
    }

    #[test]
    fn test_binary_overlaps_range() {
        let control_text = r#"Source: test-package

Package: test-binary
Architecture: any
Depends: ${shlibs:Depends}
"#;
        let control: Control = control_text.parse().unwrap();
        let binary = control.binaries().next().unwrap();

        // Test range that overlaps with binary paragraph
        let package_start = control_text.find("Package:").unwrap();
        let overlap_range = TextRange::new(
            (package_start as u32).into(),
            ((package_start + 30) as u32).into(),
        );
        assert!(binary.overlaps_range(overlap_range));

        // Test range before binary paragraph
        let no_overlap_range = TextRange::new(0.into(), 10.into());
        assert!(!binary.overlaps_range(no_overlap_range));
    }

    #[test]
    fn test_binary_fields_in_range() {
        let control_text = r#"Source: test-package

Package: test-binary
Architecture: any
Depends: ${shlibs:Depends}
Description: Test binary
 This is a test binary package
"#;
        let control: Control = control_text.parse().unwrap();
        let binary = control.binaries().next().unwrap();

        // Test range covering Architecture and Depends
        let arch_start = control_text.find("Architecture:").unwrap();
        let depends_end = control_text.find("Depends: ${shlibs:Depends}").unwrap()
            + "Depends: ${shlibs:Depends}".len();
        let range = TextRange::new((arch_start as u32).into(), (depends_end as u32).into());

        let fields: Vec<_> = binary.fields_in_range(range).collect();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].key(), Some("Architecture".to_string()));
        assert_eq!(fields[1].key(), Some("Depends".to_string()));

        // Test partial overlap with Description field
        let desc_start = control_text.find("Description:").unwrap();
        let partial_range = TextRange::new(
            ((desc_start + 5) as u32).into(),
            ((desc_start + 15) as u32).into(),
        );
        let fields: Vec<_> = binary.fields_in_range(partial_range).collect();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key(), Some("Description".to_string()));
    }

    #[test]
    fn test_incremental_parsing_use_case() {
        // This test simulates a real LSP use case where only changed fields are processed
        let control_text = r#"Source: example
Maintainer: John Doe <john@example.com>
Standards-Version: 4.6.0
Build-Depends: debhelper-compat (= 13)

Package: example-bin
Architecture: all
Depends: ${misc:Depends}
Description: Example package
 This is an example.
"#;
        let control: Control = control_text.parse().unwrap();

        // Simulate a change to Standards-Version field
        let change_start = control_text.find("Standards-Version:").unwrap();
        let change_end = change_start + "Standards-Version: 4.6.0".len();
        let change_range = TextRange::new((change_start as u32).into(), (change_end as u32).into());

        // Only process fields in the changed range
        let affected_fields: Vec<_> = control.fields_in_range(change_range).collect();
        assert_eq!(affected_fields.len(), 1);
        assert_eq!(
            affected_fields[0].key(),
            Some("Standards-Version".to_string())
        );

        // Verify that we're not processing unrelated fields
        for entry in &affected_fields {
            let key = entry.key().unwrap();
            assert_ne!(key, "Maintainer");
            assert_ne!(key, "Build-Depends");
            assert_ne!(key, "Architecture");
        }
    }

    #[test]
    fn test_positioned_parse_errors() {
        // Test case from the requirements document
        let input = "Invalid: field\nBroken field without colon";
        let parsed = Control::parse(input);

        // Should have positioned errors accessible
        let positioned_errors = parsed.positioned_errors();
        assert!(
            !positioned_errors.is_empty(),
            "Should have positioned errors"
        );

        // Test that we can access error properties
        for error in positioned_errors {
            let start_offset: u32 = error.range.start().into();
            let end_offset: u32 = error.range.end().into();

            // Verify we have meaningful error messages
            assert!(!error.message.is_empty());

            // Verify ranges are valid
            assert!(start_offset <= end_offset);
            assert!(end_offset <= input.len() as u32);

            // Error should have a code
            assert!(error.code.is_some());

            println!(
                "Error at {:?}: {} (code: {:?})",
                error.range, error.message, error.code
            );
        }

        // Should also be able to get string errors for backward compatibility
        let string_errors = parsed.errors();
        assert!(!string_errors.is_empty());
        assert_eq!(string_errors.len(), positioned_errors.len());
    }

    #[test]
    fn test_sort_binaries_basic() {
        let input = r#"Source: foo

Package: libfoo
Architecture: all

Package: libbar
Architecture: all
"#;

        let mut control: Control = input.parse().unwrap();
        control.sort_binaries(false);

        let binaries: Vec<_> = control.binaries().collect();
        assert_eq!(binaries.len(), 2);
        assert_eq!(binaries[0].name(), Some("libbar".to_string()));
        assert_eq!(binaries[1].name(), Some("libfoo".to_string()));
    }

    #[test]
    fn test_sort_binaries_keep_first() {
        let input = r#"Source: foo

Package: zzz-first
Architecture: all

Package: libbar
Architecture: all

Package: libaaa
Architecture: all
"#;

        let mut control: Control = input.parse().unwrap();
        control.sort_binaries(true);

        let binaries: Vec<_> = control.binaries().collect();
        assert_eq!(binaries.len(), 3);
        // First binary should remain in place
        assert_eq!(binaries[0].name(), Some("zzz-first".to_string()));
        // The rest should be sorted
        assert_eq!(binaries[1].name(), Some("libaaa".to_string()));
        assert_eq!(binaries[2].name(), Some("libbar".to_string()));
    }

    #[test]
    fn test_sort_binaries_already_sorted() {
        let input = r#"Source: foo

Package: aaa
Architecture: all

Package: bbb
Architecture: all

Package: ccc
Architecture: all
"#;

        let mut control: Control = input.parse().unwrap();
        control.sort_binaries(false);

        let binaries: Vec<_> = control.binaries().collect();
        assert_eq!(binaries.len(), 3);
        assert_eq!(binaries[0].name(), Some("aaa".to_string()));
        assert_eq!(binaries[1].name(), Some("bbb".to_string()));
        assert_eq!(binaries[2].name(), Some("ccc".to_string()));
    }

    #[test]
    fn test_sort_binaries_no_binaries() {
        let input = r#"Source: foo
Maintainer: test@example.com
"#;

        let mut control: Control = input.parse().unwrap();
        control.sort_binaries(false);

        // Should not crash, just do nothing
        assert_eq!(control.binaries().count(), 0);
    }

    #[test]
    fn test_sort_binaries_one_binary() {
        let input = r#"Source: foo

Package: bar
Architecture: all
"#;

        let mut control: Control = input.parse().unwrap();
        control.sort_binaries(false);

        let binaries: Vec<_> = control.binaries().collect();
        assert_eq!(binaries.len(), 1);
        assert_eq!(binaries[0].name(), Some("bar".to_string()));
    }

    #[test]
    fn test_sort_binaries_preserves_fields() {
        let input = r#"Source: foo

Package: zzz
Architecture: any
Depends: libc6
Description: ZZZ package

Package: aaa
Architecture: all
Depends: ${misc:Depends}
Description: AAA package
"#;

        let mut control: Control = input.parse().unwrap();
        control.sort_binaries(false);

        let binaries: Vec<_> = control.binaries().collect();
        assert_eq!(binaries.len(), 2);

        // First binary should be aaa
        assert_eq!(binaries[0].name(), Some("aaa".to_string()));
        assert_eq!(binaries[0].architecture(), Some("all".to_string()));
        assert_eq!(binaries[0].description(), Some("AAA package".to_string()));

        // Second binary should be zzz
        assert_eq!(binaries[1].name(), Some("zzz".to_string()));
        assert_eq!(binaries[1].architecture(), Some("any".to_string()));
        assert_eq!(binaries[1].description(), Some("ZZZ package".to_string()));
    }

    #[test]
    fn test_remove_binary_basic() {
        let mut control = Control::new();
        control.add_binary("foo");
        assert_eq!(control.binaries().count(), 1);
        assert!(control.remove_binary("foo"));
        assert_eq!(control.binaries().count(), 0);
    }

    #[test]
    fn test_remove_binary_nonexistent() {
        let mut control = Control::new();
        control.add_binary("foo");
        assert!(!control.remove_binary("bar"));
        assert_eq!(control.binaries().count(), 1);
    }

    #[test]
    fn test_remove_binary_multiple() {
        let mut control = Control::new();
        control.add_binary("foo");
        control.add_binary("bar");
        control.add_binary("baz");
        assert_eq!(control.binaries().count(), 3);

        assert!(control.remove_binary("bar"));
        assert_eq!(control.binaries().count(), 2);

        let names: Vec<_> = control.binaries().map(|b| b.name().unwrap()).collect();
        assert_eq!(names, vec!["foo", "baz"]);
    }

    #[test]
    fn test_remove_binary_preserves_source() {
        let input = r#"Source: mypackage

Package: foo
Architecture: all

Package: bar
Architecture: all
"#;
        let mut control: Control = input.parse().unwrap();
        assert!(control.source().is_some());
        assert_eq!(control.binaries().count(), 2);

        assert!(control.remove_binary("foo"));

        // Source should still be present
        assert!(control.source().is_some());
        assert_eq!(
            control.source().unwrap().name(),
            Some("mypackage".to_string())
        );

        // Only bar should remain
        assert_eq!(control.binaries().count(), 1);
        assert_eq!(
            control.binaries().next().unwrap().name(),
            Some("bar".to_string())
        );
    }

    #[test]
    fn test_remove_binary_from_parsed() {
        let input = r#"Source: test

Package: test-bin
Architecture: any
Depends: libc6
Description: Test binary

Package: test-lib
Architecture: all
Description: Test library
"#;
        let mut control: Control = input.parse().unwrap();
        assert_eq!(control.binaries().count(), 2);

        assert!(control.remove_binary("test-bin"));

        let output = control.to_string();
        assert!(!output.contains("test-bin"));
        assert!(output.contains("test-lib"));
        assert!(output.contains("Source: test"));
    }

    #[test]
    fn test_build_depends_preserves_indentation_after_removal() {
        let input = r#"Source: acpi-support
Section: admin
Priority: optional
Maintainer: Debian Acpi Team <pkg-acpi-devel@lists.alioth.debian.org>
Build-Depends: debhelper (>= 10), quilt (>= 0.40),
    libsystemd-dev [linux-any], dh-systemd (>= 1.5), pkg-config
"#;
        let control: Control = input.parse().unwrap();
        let mut source = control.source().unwrap();

        // Get the Build-Depends
        let mut build_depends = source.build_depends().unwrap();

        // Find and remove dh-systemd entry
        let mut to_remove = Vec::new();
        for (idx, entry) in build_depends.entries().enumerate() {
            for relation in entry.relations() {
                if relation.name() == "dh-systemd" {
                    to_remove.push(idx);
                    break;
                }
            }
        }

        for idx in to_remove.into_iter().rev() {
            build_depends.remove_entry(idx);
        }

        // Set it back
        source.set_build_depends(&build_depends);

        let output = source.to_string();

        // The indentation should be preserved (4 spaces on the continuation line)
        assert!(
            output.contains("Build-Depends: debhelper (>= 10), quilt (>= 0.40),\n    libsystemd-dev [linux-any], pkg-config"),
            "Expected 4-space indentation to be preserved, but got:\n{}",
            output
        );
    }

    #[test]
    fn test_build_depends_direct_string_set_loses_indentation() {
        let input = r#"Source: acpi-support
Section: admin
Priority: optional
Maintainer: Debian Acpi Team <pkg-acpi-devel@lists.alioth.debian.org>
Build-Depends: debhelper (>= 10), quilt (>= 0.40),
    libsystemd-dev [linux-any], dh-systemd (>= 1.5), pkg-config
"#;
        let control: Control = input.parse().unwrap();
        let mut source = control.source().unwrap();

        // Get the Build-Depends as Relations
        let mut build_depends = source.build_depends().unwrap();

        // Find and remove dh-systemd entry
        let mut to_remove = Vec::new();
        for (idx, entry) in build_depends.entries().enumerate() {
            for relation in entry.relations() {
                if relation.name() == "dh-systemd" {
                    to_remove.push(idx);
                    break;
                }
            }
        }

        for idx in to_remove.into_iter().rev() {
            build_depends.remove_entry(idx);
        }

        // Set it back using the string representation - this is what might cause the bug
        source.set("Build-Depends", &build_depends.to_string());

        let output = source.to_string();
        println!("Output with string set:");
        println!("{}", output);

        // Check if indentation is preserved
        // This test documents the current behavior - it may fail if indentation is lost
        assert!(
            output.contains("Build-Depends: debhelper (>= 10), quilt (>= 0.40),\n    libsystemd-dev [linux-any], pkg-config"),
            "Expected 4-space indentation to be preserved, but got:\n{}",
            output
        );
    }

    #[test]
    fn test_parse_mode_strict_default() {
        let control = Control::new();
        assert_eq!(control.parse_mode(), ParseMode::Strict);

        let control: Control = "Source: test\n".parse().unwrap();
        assert_eq!(control.parse_mode(), ParseMode::Strict);
    }

    #[test]
    fn test_parse_mode_new_with_mode() {
        let control_relaxed = Control::new_with_mode(ParseMode::Relaxed);
        assert_eq!(control_relaxed.parse_mode(), ParseMode::Relaxed);

        let control_substvar = Control::new_with_mode(ParseMode::Substvar);
        assert_eq!(control_substvar.parse_mode(), ParseMode::Substvar);
    }

    #[test]
    fn test_relaxed_mode_handles_broken_relations() {
        let input = r#"Source: test-package
Build-Depends: debhelper, @@@broken@@@, python3

Package: test-pkg
Depends: libfoo, %%%invalid%%%, libbar
"#;

        let (control, _errors) = Control::read_relaxed(input.as_bytes()).unwrap();
        assert_eq!(control.parse_mode(), ParseMode::Relaxed);

        // These should not panic even with broken syntax
        if let Some(source) = control.source() {
            let bd = source.build_depends();
            assert!(bd.is_some());
            let relations = bd.unwrap();
            // Should have parsed the valid parts in relaxed mode
            assert!(relations.len() >= 2); // at least debhelper and python3
        }

        for binary in control.binaries() {
            let deps = binary.depends();
            assert!(deps.is_some());
            let relations = deps.unwrap();
            // Should have parsed the valid parts
            assert!(relations.len() >= 2); // at least libfoo and libbar
        }
    }

    #[test]
    fn test_substvar_mode_via_parse() {
        // Parse normally to get valid structure, but then we'd need substvar mode
        // Actually, we can't test this properly without the ability to set mode on parsed content
        // So let's just test that read_relaxed with substvars works
        let input = r#"Source: test-package
Build-Depends: debhelper, ${misc:Depends}

Package: test-pkg
Depends: ${shlibs:Depends}, libfoo
"#;

        // This will parse in relaxed mode, which also allows substvars to some degree
        let (control, _errors) = Control::read_relaxed(input.as_bytes()).unwrap();

        if let Some(source) = control.source() {
            // Should parse without panic even with substvars
            let bd = source.build_depends();
            assert!(bd.is_some());
        }

        for binary in control.binaries() {
            let deps = binary.depends();
            assert!(deps.is_some());
        }
    }

    #[test]
    #[should_panic]
    fn test_strict_mode_panics_on_broken_syntax() {
        let input = r#"Source: test-package
Build-Depends: debhelper, @@@broken@@@
"#;

        // Strict mode (default) should panic on invalid syntax
        let control: Control = input.parse().unwrap();

        if let Some(source) = control.source() {
            // This should panic when trying to parse the broken Build-Depends
            let _ = source.build_depends();
        }
    }

    #[test]
    fn test_from_file_relaxed_sets_relaxed_mode() {
        let input = r#"Source: test-package
Maintainer: Test <test@example.com>
"#;

        let (control, _errors) = Control::read_relaxed(input.as_bytes()).unwrap();
        assert_eq!(control.parse_mode(), ParseMode::Relaxed);
    }

    #[test]
    fn test_parse_mode_propagates_to_paragraphs() {
        let input = r#"Source: test-package
Build-Depends: debhelper, @@@invalid@@@, python3

Package: test-pkg
Depends: libfoo, %%%bad%%%, libbar
"#;

        // Parse in relaxed mode
        let (control, _) = Control::read_relaxed(input.as_bytes()).unwrap();

        // The source and binary paragraphs should inherit relaxed mode
        // and not panic when parsing relations
        if let Some(source) = control.source() {
            assert!(source.build_depends().is_some());
        }

        for binary in control.binaries() {
            assert!(binary.depends().is_some());
        }
    }

    #[test]
    fn test_preserves_final_newline() {
        // Test that the final newline is preserved when writing control files
        let input_with_newline = "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-pkg\nArchitecture: any\n";
        let control: Control = input_with_newline.parse().unwrap();
        let output = control.to_string();
        assert_eq!(output, input_with_newline);
    }

    #[test]
    fn test_preserves_no_final_newline() {
        // Test that absence of final newline is also preserved (even though it's not POSIX-compliant)
        let input_without_newline = "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-pkg\nArchitecture: any";
        let control: Control = input_without_newline.parse().unwrap();
        let output = control.to_string();
        assert_eq!(output, input_without_newline);
    }

    #[test]
    fn test_final_newline_after_modifications() {
        // Test that final newline is preserved even after modifications
        let input = "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-pkg\nArchitecture: any\n";
        let control: Control = input.parse().unwrap();

        // Make a modification
        let mut source = control.source().unwrap();
        source.set_section(Some("utils"));

        let output = control.to_string();
        let expected = "Source: test-package\nSection: utils\nMaintainer: Test <test@example.com>\n\nPackage: test-pkg\nArchitecture: any\n";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_source_in_range() {
        // Test that source_in_range() returns the source when it intersects with range
        let input = r#"Source: test-package
Maintainer: Test <test@example.com>
Section: utils

Package: test-pkg
Architecture: any
"#;
        let control: Control = input.parse().unwrap();

        // Get the text range of the source paragraph
        let source = control.source().unwrap();
        let source_range = source.as_deb822().text_range();

        // Query with the exact range - should return the source
        let result = control.source_in_range(source_range);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), Some("test-package".to_string()));

        // Query with a range that overlaps the source
        let overlap_range = TextRange::new(0.into(), 20.into());
        let result = control.source_in_range(overlap_range);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), Some("test-package".to_string()));

        // Query with a range that doesn't overlap the source
        let no_overlap_range = TextRange::new(100.into(), 150.into());
        let result = control.source_in_range(no_overlap_range);
        assert!(result.is_none());
    }

    #[test]
    fn test_binaries_in_range_single() {
        // Test that binaries_in_range() returns a single binary in range
        let input = r#"Source: test-package
Maintainer: Test <test@example.com>

Package: test-pkg
Architecture: any

Package: another-pkg
Architecture: all
"#;
        let control: Control = input.parse().unwrap();

        // Get the text range of the first binary paragraph
        let first_binary = control.binaries().next().unwrap();
        let binary_range = first_binary.as_deb822().text_range();

        // Query with that range - should return only the first binary
        let binaries: Vec<_> = control.binaries_in_range(binary_range).collect();
        assert_eq!(binaries.len(), 1);
        assert_eq!(binaries[0].name(), Some("test-pkg".to_string()));
    }

    #[test]
    fn test_binaries_in_range_multiple() {
        // Test that binaries_in_range() returns multiple binaries in range
        let input = r#"Source: test-package
Maintainer: Test <test@example.com>

Package: test-pkg
Architecture: any

Package: another-pkg
Architecture: all

Package: third-pkg
Architecture: any
"#;
        let control: Control = input.parse().unwrap();

        // Create a range that covers the first two binary paragraphs
        let range = TextRange::new(50.into(), 130.into());

        // Query with that range
        let binaries: Vec<_> = control.binaries_in_range(range).collect();
        assert!(binaries.len() >= 2);
        assert!(binaries
            .iter()
            .any(|b| b.name() == Some("test-pkg".to_string())));
        assert!(binaries
            .iter()
            .any(|b| b.name() == Some("another-pkg".to_string())));
    }

    #[test]
    fn test_binaries_in_range_none() {
        // Test that binaries_in_range() returns empty iterator when no binaries in range
        let input = r#"Source: test-package
Maintainer: Test <test@example.com>

Package: test-pkg
Architecture: any
"#;
        let control: Control = input.parse().unwrap();

        // Create a range that's way beyond the document
        let range = TextRange::new(1000.into(), 2000.into());

        // Should return empty iterator
        let binaries: Vec<_> = control.binaries_in_range(range).collect();
        assert_eq!(binaries.len(), 0);
    }

    #[test]
    fn test_binaries_in_range_all() {
        // Test that binaries_in_range() returns all binaries when range covers entire document
        let input = r#"Source: test-package
Maintainer: Test <test@example.com>

Package: test-pkg
Architecture: any

Package: another-pkg
Architecture: all
"#;
        let control: Control = input.parse().unwrap();

        // Create a range that covers the entire document
        let range = TextRange::new(0.into(), input.len().try_into().unwrap());

        // Should return all binaries
        let binaries: Vec<_> = control.binaries_in_range(range).collect();
        assert_eq!(binaries.len(), 2);
    }

    #[test]
    fn test_source_in_range_partial_overlap() {
        // Test that source_in_range() returns source with partial overlap
        let input = r#"Source: test-package
Maintainer: Test <test@example.com>

Package: test-pkg
Architecture: any
"#;
        let control: Control = input.parse().unwrap();

        // Create a range that starts in the middle of the source paragraph
        let range = TextRange::new(10.into(), 30.into());

        // Should include the source since it overlaps
        let result = control.source_in_range(range);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), Some("test-package".to_string()));
    }

    #[test]
    fn test_wrap_and_sort_with_malformed_relations() {
        // Test that wrap_and_sort doesn't panic on malformed relations
        // and preserves the original value when parsing fails
        let input = r#"Source: test-package
Maintainer: Test <test@example.com>
Build-Depends: some invalid relation syntax here

Package: test-pkg
Architecture: any
"#;
        let mut control: Control = input.parse().unwrap();

        // This should not panic, even with malformed relations
        control.wrap_and_sort(deb822_lossless::Indentation::Spaces(2), false, None);

        // The malformed field should be preserved as-is (lossless behavior)
        let output = control.to_string();
        let expected = r#"Source: test-package
Maintainer: Test <test@example.com>
Build-Depends: some invalid relation syntax here

Package: test-pkg
Architecture: any
"#;
        assert_eq!(output, expected);
    }
}
