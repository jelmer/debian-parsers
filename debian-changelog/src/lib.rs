#![deny(missing_docs)]
//! A lossless parser for Debian changelog files.
//!
//! See https://manpages.debian.org/bookworm/dpkg-dev/deb-changelog.5.en.html
//!
//! For its format specification, see [Debian Policy](https://www.debian.org/doc/debian-policy/ch-source.html#debian-changelog-debian-changelog).
//!
//! Example:
//!
//! ```rust
//! use std::io::Read;
//! let contents = r#"rustc (1.70.0+dfsg1-1) unstable; urgency=medium
//!
//!   * Upload to unstable
//!
//!  -- Jelmer Vernooĳ <jelmer@debian.org>  Wed, 20 Sep 2023 20:18:40 +0200
//! "#;
//! let changelog: debian_changelog::ChangeLog = contents.parse().unwrap();
//! assert_eq!(
//!     vec![("rustc".to_string(), "1.70.0+dfsg1-1".parse().unwrap())],
//!     changelog.iter().map(
//!         |e| (e.package().unwrap(), e.version().unwrap()))
//!     .collect::<Vec<_>>());
//! ```

mod lex;
mod parse;
use lazy_regex::regex_captures;
pub mod bugs;
pub mod changes;
pub mod textwrap;
use crate::parse::{SyntaxNode, SyntaxToken};
use debversion::Version;
use rowan::ast::AstNode;

pub use crate::changes::changes_by_author;
pub use crate::parse::{
    ChangeLog, Entry, EntryBody, EntryFooter, EntryHeader, Error, IntoTimestamp, Maintainer,
    MetadataEntry, MetadataKey, MetadataValue, Parse, ParseError, Timestamp, Urgency,
};

/// Represents a logical change within a changelog entry.
///
/// This struct wraps specific DETAIL tokens within an Entry's syntax tree
/// and provides methods to manipulate them while maintaining the AST structure.
#[derive(Debug, Clone)]
pub struct Change {
    /// The parent entry containing this change
    entry: Entry,
    /// The author of the change (if attributed)
    author: Option<String>,
    /// Line numbers in the original entry where this change appears
    line_numbers: Vec<usize>,
    /// The actual change lines as tokens in the syntax tree
    detail_tokens: Vec<SyntaxToken>,
}

impl Change {
    /// Create a new Change instance.
    pub(crate) fn new(
        entry: Entry,
        author: Option<String>,
        line_numbers: Vec<usize>,
        detail_tokens: Vec<SyntaxToken>,
    ) -> Self {
        Self {
            entry,
            author,
            line_numbers,
            detail_tokens,
        }
    }

    /// Get the author of this change.
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// Get the line numbers in the original entry where this change appears.
    pub fn line_numbers(&self) -> &[usize] {
        &self.line_numbers
    }

    /// Get the lines of this change.
    pub fn lines(&self) -> Vec<String> {
        self.detail_tokens
            .iter()
            .map(|token| token.text().to_string())
            .collect()
    }

    /// Get the package name this change belongs to.
    pub fn package(&self) -> Option<String> {
        self.entry.package()
    }

    /// Get the version this change belongs to, returning an error if the version string is invalid.
    ///
    /// Returns:
    /// - `Some(Ok(version))` if a valid version is found
    /// - `Some(Err(err))` if a version token exists but cannot be parsed
    /// - `None` if no version token is present
    pub fn try_version(&self) -> Option<Result<Version, debversion::ParseError>> {
        self.entry.try_version()
    }

    /// Get the version this change belongs to.
    ///
    /// Note: This method silently returns `None` if the version string is invalid.
    /// Consider using [`try_version`](Self::try_version) instead to handle parsing errors properly.
    pub fn version(&self) -> Option<Version> {
        self.try_version().and_then(|r| r.ok())
    }

    /// Check if this change is attributed to a specific author.
    pub fn is_attributed(&self) -> bool {
        self.author.is_some()
    }

    /// Get a reference to the parent entry.
    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    /// Get the line number (0-indexed) where this change starts.
    ///
    /// Returns the line number of the first detail token, or None if the change has no tokens.
    pub fn line(&self) -> Option<usize> {
        self.detail_tokens.first().map(|token| {
            parse::line_col_at_offset(self.entry.syntax(), token.text_range().start()).0
        })
    }

    /// Get the column number (0-indexed, in bytes) where this change starts.
    ///
    /// Returns the column number of the first detail token, or None if the change has no tokens.
    pub fn column(&self) -> Option<usize> {
        self.detail_tokens.first().map(|token| {
            parse::line_col_at_offset(self.entry.syntax(), token.text_range().start()).1
        })
    }

    /// Get both line and column (0-indexed) where this change starts.
    ///
    /// Returns (line, column) where column is measured in bytes from the start of the line,
    /// or None if the change has no tokens.
    pub fn line_col(&self) -> Option<(usize, usize)> {
        self.detail_tokens
            .first()
            .map(|token| parse::line_col_at_offset(self.entry.syntax(), token.text_range().start()))
    }

    /// Remove this change from its parent entry.
    ///
    /// This removes all DETAIL tokens (ENTRY_BODY nodes) associated with this change
    /// from the syntax tree. If this removes the last change in an author section,
    /// the empty section header will also be removed.
    pub fn remove(self) {
        // Store info we'll need after removal
        let author = self.author.clone();

        // Collect the parent ENTRY_BODY nodes that contain our detail tokens
        let mut body_nodes_to_remove = Vec::new();

        for token in &self.detail_tokens {
            if let Some(parent) = token.parent() {
                if parent.kind() == SyntaxKind::ENTRY_BODY {
                    // Check if we haven't already marked this node for removal
                    if !body_nodes_to_remove
                        .iter()
                        .any(|n: &SyntaxNode| n == &parent)
                    {
                        body_nodes_to_remove.push(parent);
                    }
                }
            }
        }

        // Find the section header node if this is an attributed change
        // and capture its index BEFORE we remove any nodes
        let section_header_index = if author.is_some() && !body_nodes_to_remove.is_empty() {
            Self::find_section_header_for_changes(&self.entry, &body_nodes_to_remove)
                .map(|node| node.index())
        } else {
            None
        };

        // Remove the ENTRY_BODY nodes from the entry's syntax tree
        // We need to remove from highest index to lowest to avoid index shifting issues
        let mut sorted_nodes = body_nodes_to_remove;
        sorted_nodes.sort_by_key(|n| std::cmp::Reverse(n.index()));

        // Track which indices to remove (ENTRY_BODY nodes and trailing EMPTY_LINE nodes)
        let mut indices_to_remove = Vec::new();
        let children: Vec<_> = self.entry.syntax().children().collect();

        for body_node in &sorted_nodes {
            let index = body_node.index();
            indices_to_remove.push(index);

            // Remove trailing EMPTY_LINE if it exists and would create consecutive blanks
            if Self::should_remove_trailing_empty(&children, index) {
                indices_to_remove.push(index + 1);
            }
        }

        // Sort indices in reverse order and remove duplicates
        indices_to_remove.sort_by_key(|&i| std::cmp::Reverse(i));
        indices_to_remove.dedup();

        // Remove the nodes
        for index in indices_to_remove {
            self.entry
                .syntax()
                .splice_children(index..index + 1, vec![]);
        }

        // Check if section is now empty and remove header if needed
        // After removing bullets, we need to adjust the header index based on how many
        // nodes were removed before it
        if let Some(original_header_idx) = section_header_index {
            // Count how many nodes we removed that were before the header
            let nodes_removed_before_header = sorted_nodes
                .iter()
                .filter(|n| n.index() < original_header_idx)
                .count();

            // Adjust the header index
            let adjusted_header_idx = original_header_idx - nodes_removed_before_header;

            Self::remove_section_header_if_empty_at_index(&self.entry, adjusted_header_idx);
        }
    }

    /// Check if a node is a section header (e.g., "[ Author Name ]")
    fn is_section_header(node: &SyntaxNode) -> bool {
        if node.kind() != SyntaxKind::ENTRY_BODY {
            return false;
        }

        for token in node.descendants_with_tokens() {
            if let Some(token) = token.as_token() {
                if token.kind() == SyntaxKind::DETAIL
                    && crate::changes::extract_author_name(token.text()).is_some()
                {
                    return true;
                }
            }
        }

        false
    }

    /// Check if the trailing EMPTY_LINE after an entry should be removed
    /// Returns true if removing it would prevent consecutive blank lines
    fn should_remove_trailing_empty(children: &[SyntaxNode], entry_index: usize) -> bool {
        // Check if there's a trailing EMPTY_LINE
        let has_trailing_empty = children
            .get(entry_index + 1)
            .is_some_and(|n| n.kind() == SyntaxKind::EMPTY_LINE);

        if !has_trailing_empty {
            return false;
        }

        // Remove if there's already an EMPTY_LINE before (would create consecutive blanks)
        let has_preceding_empty = entry_index > 0
            && children
                .get(entry_index - 1)
                .is_some_and(|n| n.kind() == SyntaxKind::EMPTY_LINE);

        if has_preceding_empty {
            return true;
        }

        // Remove if what follows would create consecutive blanks or be a section header
        match children.get(entry_index + 2) {
            Some(node) if node.kind() == SyntaxKind::EMPTY_LINE => true,
            Some(node) if Self::is_section_header(node) => true,
            _ => false,
        }
    }

    /// Check if the preceding EMPTY_LINE before a section header should be removed
    /// Preserves the blank line if it's the first one after the entry header
    fn should_remove_preceding_empty(children: &[SyntaxNode], header_index: usize) -> bool {
        if header_index == 0 {
            return false;
        }

        // Check if there's a preceding EMPTY_LINE
        let has_preceding_empty = children
            .get(header_index - 1)
            .is_some_and(|n| n.kind() == SyntaxKind::EMPTY_LINE);

        if !has_preceding_empty {
            return false;
        }

        // Don't remove if it's the first blank line after the entry header
        let is_first_blank_after_header = header_index >= 2
            && children
                .get(header_index - 2)
                .is_some_and(|n| n.kind() == SyntaxKind::ENTRY_HEADER);

        !is_first_blank_after_header
    }

    /// Find the section header that precedes the given change nodes
    fn find_section_header_for_changes(
        entry: &Entry,
        change_nodes: &[SyntaxNode],
    ) -> Option<SyntaxNode> {
        if change_nodes.is_empty() {
            return None;
        }

        let first_change_index = change_nodes.iter().map(|n| n.index()).min().unwrap();
        let mut header_node = None;

        for child in entry.syntax().children() {
            for token_or_node in child.children_with_tokens() {
                let Some(token) = token_or_node.as_token() else {
                    continue;
                };
                if token.kind() != SyntaxKind::DETAIL {
                    continue;
                }

                let Some(parent) = token.parent() else {
                    continue;
                };
                if parent.kind() != SyntaxKind::ENTRY_BODY {
                    continue;
                }

                let parent_index = parent.index();
                if parent_index >= first_change_index {
                    continue;
                }

                if crate::changes::extract_author_name(token.text()).is_some() {
                    header_node = Some(parent);
                }
            }
        }

        header_node
    }

    /// Remove a section header if its section is now empty
    fn remove_section_header_if_empty_at_index(entry: &Entry, header_index: usize) {
        // Check if there are any bullet points after this header and before the next header
        let mut has_bullets_in_section = false;

        'outer: for child in entry.syntax().children() {
            for token_or_node in child.children_with_tokens() {
                let Some(token) = token_or_node.as_token() else {
                    continue;
                };
                if token.kind() != SyntaxKind::DETAIL {
                    continue;
                }

                let Some(parent) = token.parent() else {
                    continue;
                };
                if parent.kind() != SyntaxKind::ENTRY_BODY {
                    continue;
                }

                let parent_index = parent.index();
                if parent_index <= header_index {
                    continue;
                }

                let text = token.text();
                // If we hit another section header, stop searching
                if crate::changes::extract_author_name(text).is_some() {
                    break 'outer;
                }

                // If we find a bullet point, section is not empty
                if text.starts_with("* ") {
                    has_bullets_in_section = true;
                    break 'outer;
                }
            }
        }

        // Remove the header if section is empty
        if !has_bullets_in_section {
            let children: Vec<_> = entry.syntax().children().collect();

            // Determine if we should also remove the preceding EMPTY_LINE
            // (but preserve the blank line right after the entry header)
            let start_index = if Self::should_remove_preceding_empty(&children, header_index) {
                header_index - 1
            } else {
                header_index
            };

            // Important: rowan's splice_children iterates and detaches nodes in order.
            // When a node is detached, it changes the tree immediately, which can cause
            // the iteration to skip nodes. Removing in reverse order avoids this issue.
            for idx in (start_index..=header_index).rev() {
                entry.syntax().splice_children(idx..idx + 1, vec![]);
            }
        }
    }

    /// Replace this change with new lines.
    ///
    /// This removes the current change lines and replaces them with the provided lines.
    ///
    /// # Arguments
    /// * `new_lines` - The new change lines to replace with (e.g., `["* Updated feature"]`)
    pub fn replace_with(&self, new_lines: Vec<&str>) {
        use rowan::GreenNodeBuilder;

        // Find the first ENTRY_BODY node to determine insertion point
        let first_body_node = self
            .detail_tokens
            .first()
            .and_then(|token| token.parent())
            .filter(|parent| parent.kind() == SyntaxKind::ENTRY_BODY);

        if let Some(_first_node) = first_body_node {
            // Collect all ENTRY_BODY nodes to remove
            let mut body_nodes_to_remove = Vec::new();
            for token in &self.detail_tokens {
                if let Some(parent) = token.parent() {
                    if parent.kind() == SyntaxKind::ENTRY_BODY
                        && !body_nodes_to_remove
                            .iter()
                            .any(|n: &SyntaxNode| n == &parent)
                    {
                        body_nodes_to_remove.push(parent);
                    }
                }
            }

            // Build replacement nodes
            let mut new_nodes = Vec::new();
            for line in new_lines {
                let mut builder = GreenNodeBuilder::new();
                builder.start_node(SyntaxKind::ENTRY_BODY.into());
                if !line.is_empty() {
                    builder.token(SyntaxKind::INDENT.into(), "  ");
                    builder.token(SyntaxKind::DETAIL.into(), line);
                }
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
                builder.finish_node();

                let syntax = SyntaxNode::new_root_mut(builder.finish());
                new_nodes.push(syntax.into());
            }

            // Remove old nodes and insert new ones
            // We need to remove from highest index to lowest to avoid index shifting issues
            let mut sorted_nodes = body_nodes_to_remove.clone();
            sorted_nodes.sort_by_key(|n| std::cmp::Reverse(n.index()));

            for (i, node) in sorted_nodes.iter().enumerate() {
                let idx = node.index();
                if i == 0 {
                    // For the first removal, insert the new nodes
                    self.entry
                        .syntax()
                        .splice_children(idx..idx + 1, new_nodes.clone());
                } else {
                    // For subsequent removals, just remove
                    self.entry.syntax().splice_children(idx..idx + 1, vec![]);
                }
            }
        }
    }

    /// Replace a specific line in this change by index.
    ///
    /// # Arguments
    /// * `index` - The zero-based index of the line to replace
    /// * `new_text` - The new text for the line
    ///
    /// # Returns
    /// * `Ok(())` if the line was replaced successfully
    /// * `Err(Error)` if the index is out of bounds
    ///
    /// # Examples
    /// ```
    /// use debian_changelog::{ChangeLog, iter_changes_by_author};
    ///
    /// let changelog_text = r#"blah (1.0-1) unstable; urgency=low
    ///
    ///   * First change
    ///   * Second change
    ///
    ///  -- Author <email@example.com>  Mon, 01 Jan 2024 00:00:00 +0000
    /// "#;
    ///
    /// let changelog = ChangeLog::read_relaxed(changelog_text.as_bytes()).unwrap();
    /// let changes = iter_changes_by_author(&changelog);
    /// changes[0].replace_line(0, "* Updated first change").unwrap();
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn replace_line(&self, index: usize, new_text: &str) -> Result<(), Error> {
        if index >= self.detail_tokens.len() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Line index {} out of bounds (0..{})",
                    index,
                    self.detail_tokens.len()
                ),
            )));
        }

        let mut new_lines = self.lines();
        new_lines[index] = new_text.to_string();

        self.replace_with(new_lines.iter().map(|s| s.as_str()).collect());
        Ok(())
    }

    /// Update lines in this change that match a predicate.
    ///
    /// This method finds all lines that match the predicate function and replaces
    /// them with the result of the updater function.
    ///
    /// # Arguments
    /// * `predicate` - A function that returns true for lines that should be updated
    /// * `updater` - A function that takes the old line text and returns the new line text
    ///
    /// # Returns
    /// The number of lines that were updated
    ///
    /// # Examples
    /// ```
    /// use debian_changelog::{ChangeLog, iter_changes_by_author};
    ///
    /// let changelog_text = r#"blah (1.0-1) unstable; urgency=low
    ///
    ///   * First change
    ///   * Second change
    ///   * Third change
    ///
    ///  -- Author <email@example.com>  Mon, 01 Jan 2024 00:00:00 +0000
    /// "#;
    ///
    /// let changelog = ChangeLog::read_relaxed(changelog_text.as_bytes()).unwrap();
    /// let changes = iter_changes_by_author(&changelog);
    ///
    /// // Update lines containing "First" or "Second"
    /// let count = changes[0].update_lines(
    ///     |line| line.contains("First") || line.contains("Second"),
    ///     |line| format!("{} (updated)", line)
    /// );
    /// assert_eq!(count, 2);
    /// ```
    pub fn update_lines<F, G>(&self, predicate: F, updater: G) -> usize
    where
        F: Fn(&str) -> bool,
        G: Fn(&str) -> String,
    {
        let mut new_lines = self.lines();
        let mut update_count = 0;

        for line in &mut new_lines {
            if predicate(line) {
                *line = updater(line);
                update_count += 1;
            }
        }

        if update_count > 0 {
            self.replace_with(new_lines.iter().map(|s| s.as_str()).collect());
        }

        update_count
    }

    /// Split this change into individual bullet points.
    ///
    /// Each bullet point (line starting with "* ") and its continuation lines
    /// (indented lines that follow) become a separate Change object.
    ///
    /// # Returns
    /// A vector of Change objects, one per bullet point. Each Change contains:
    /// - The same entry and author as the parent
    /// - Subset of line_numbers for that specific bullet
    /// - Subset of detail_tokens for that bullet and its continuation lines
    ///
    /// # Examples
    /// ```
    /// use debian_changelog::{ChangeLog, iter_changes_by_author};
    ///
    /// let changelog_text = r#"blah (1.0-1) unstable; urgency=low
    ///
    ///   * First change
    ///   * Second change
    ///     with continuation
    ///
    ///  -- Author <email@example.com>  Mon, 01 Jan 2024 00:00:00 +0000
    /// "#;
    ///
    /// let changelog = ChangeLog::read_relaxed(changelog_text.as_bytes()).unwrap();
    /// let changes = iter_changes_by_author(&changelog);
    /// let bullets = changes[0].split_into_bullets();
    /// assert_eq!(bullets.len(), 2);
    /// assert_eq!(bullets[0].lines(), vec!["* First change"]);
    /// assert_eq!(bullets[1].lines(), vec!["* Second change", "  with continuation"]);
    /// ```
    pub fn split_into_bullets(&self) -> Vec<Change> {
        let mut result = Vec::new();
        let mut current_bullet_tokens = Vec::new();
        let mut current_bullet_line_numbers = Vec::new();

        for (i, token) in self.detail_tokens.iter().enumerate() {
            let text = token.text();
            let line_number = self.line_numbers.get(i).copied().unwrap_or(0);

            // Check if this is a new bullet point (starts with "* ")
            if text.starts_with("* ") {
                // If we have a previous bullet, save it
                if !current_bullet_tokens.is_empty() {
                    result.push(Change::new(
                        self.entry.clone(),
                        self.author.clone(),
                        current_bullet_line_numbers.clone(),
                        current_bullet_tokens.clone(),
                    ));
                    current_bullet_tokens.clear();
                    current_bullet_line_numbers.clear();
                }

                // Start a new bullet
                current_bullet_tokens.push(token.clone());
                current_bullet_line_numbers.push(line_number);
            } else {
                // This is a continuation line, add to current bullet
                current_bullet_tokens.push(token.clone());
                current_bullet_line_numbers.push(line_number);
            }
        }

        // Don't forget the last bullet
        if !current_bullet_tokens.is_empty() {
            result.push(Change::new(
                self.entry.clone(),
                self.author.clone(),
                current_bullet_line_numbers,
                current_bullet_tokens,
            ));
        }

        result
    }
}

/// Let's start with defining all kinds of tokens and
/// composite nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
#[repr(u16)]
#[allow(missing_docs)]
pub enum SyntaxKind {
    IDENTIFIER = 0,
    INDENT,
    TEXT,
    WHITESPACE,
    VERSION,   // "(3.3.4-1)"
    SEMICOLON, // ";"
    EQUALS,    // "="
    DETAIL,    // "* New upstream release."
    NEWLINE,   // newlines are explicit
    ERROR,     // as well as errors
    COMMENT,   // "#"

    // composite nodes
    ROOT,  // The entire file
    ENTRY, // A single entry
    ENTRY_HEADER,
    ENTRY_FOOTER,
    METADATA,
    METADATA_ENTRY,
    METADATA_KEY,
    METADATA_VALUE,
    ENTRY_BODY,
    DISTRIBUTIONS,
    EMPTY_LINE,

    TIMESTAMP,
    MAINTAINER,
    EMAIL,
}

/// Convert our `SyntaxKind` into the rowan `SyntaxKind`.
impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

/// Parse a identity string
///
/// # Arguments
/// * `s` - The string to parse
///
/// # Returns
/// A tuple with name and email address
pub fn parseaddr(s: &str) -> (Option<&str>, &str) {
    if let Some((_, name, email)) = regex_captures!(r"^(.*)\s+<(.*)>$", s) {
        if name.is_empty() {
            (None, email)
        } else {
            (Some(name), email)
        }
    } else {
        (None, s)
    }
}

/// Get the maintainer information from the environment.
pub fn get_maintainer_from_env(
    get_env: impl Fn(&str) -> Option<String>,
) -> Option<(String, String)> {
    use std::io::BufRead;

    let mut debemail = get_env("DEBEMAIL");
    let mut debfullname = get_env("DEBFULLNAME");

    // Split email and name
    if let Some(email) = debemail.as_ref() {
        let (parsed_name, parsed_email) = parseaddr(email);
        if let Some(parsed_name) = parsed_name {
            if debfullname.is_none() {
                debfullname = Some(parsed_name.to_string());
            }
        }
        debemail = Some(parsed_email.to_string());
    }
    if debfullname.is_none() || debemail.is_none() {
        if let Some(email) = get_env("EMAIL") {
            let (parsed_name, parsed_email) = parseaddr(email.as_str());
            if let Some(parsed_name) = parsed_name {
                if debfullname.is_none() {
                    debfullname = Some(parsed_name.to_string());
                }
            }
            debemail = Some(parsed_email.to_string());
        }
    }

    // Get maintainer's name
    let maintainer = if let Some(m) = debfullname {
        Some(m.trim().to_string())
    } else if let Some(m) = get_env("NAME") {
        Some(m.trim().to_string())
    } else {
        Some(whoami::realname())
    };

    // Get maintainer's mail address
    let email_address = if let Some(email) = debemail {
        Some(email)
    } else if let Some(email) = get_env("EMAIL") {
        Some(email)
    } else {
        // Read /etc/mailname or use hostname
        let mut addr: Option<String> = None;

        if let Ok(mailname_file) = std::fs::File::open("/etc/mailname") {
            let mut reader = std::io::BufReader::new(mailname_file);
            if let Ok(line) = reader.fill_buf() {
                if !line.is_empty() {
                    addr = Some(String::from_utf8_lossy(line).trim().to_string());
                }
            }
        }

        if addr.is_none() {
            match whoami::fallible::hostname() {
                Ok(hostname) => {
                    addr = Some(hostname);
                }
                Err(e) => {
                    log::debug!("Failed to get hostname: {}", e);
                    addr = None;
                }
            }
        }

        addr.map(|hostname| format!("{}@{}", whoami::username(), hostname))
    };

    if let (Some(maintainer), Some(email_address)) = (maintainer, email_address) {
        Some((maintainer, email_address))
    } else {
        None
    }
}

/// Get the maintainer information in the same manner as dch.
///
/// This function gets the information about the current user for
/// the maintainer field using environment variables of gecos
/// information as appropriate.
///
/// It uses the same algorithm as dch to get the information, namely
/// DEBEMAIL, DEBFULLNAME, EMAIL, NAME, /etc/mailname and gecos.
///
/// # Returns
///
/// a tuple of the full name, email pair as strings.
///     Either of the pair may be None if that value couldn't
///     be determined.
pub fn get_maintainer() -> Option<(String, String)> {
    get_maintainer_from_env(|s| std::env::var(s).ok())
}

#[cfg(test)]
mod get_maintainer_from_env_tests {
    use super::*;

    #[test]
    fn test_normal() {
        get_maintainer();
    }

    #[test]
    fn test_deb_vars() {
        let mut d = std::collections::HashMap::new();
        d.insert("DEBFULLNAME".to_string(), "Jelmer".to_string());
        d.insert("DEBEMAIL".to_string(), "jelmer@example.com".to_string());
        let t = get_maintainer_from_env(|s| d.get(s).cloned());
        assert_eq!(
            Some(("Jelmer".to_string(), "jelmer@example.com".to_string())),
            t
        );
    }

    #[test]
    fn test_email_var() {
        let mut d = std::collections::HashMap::new();
        d.insert("NAME".to_string(), "Jelmer".to_string());
        d.insert("EMAIL".to_string(), "foo@example.com".to_string());
        let t = get_maintainer_from_env(|s| d.get(s).cloned());
        assert_eq!(
            Some(("Jelmer".to_string(), "foo@example.com".to_string())),
            t
        );
    }
}

/// Simple representation of an identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identity {
    /// Name of the maintainer
    pub name: String,

    /// Email address of the maintainer
    pub email: String,
}

impl Identity {
    /// Create a new identity.
    pub fn new(name: String, email: String) -> Self {
        Self { name, email }
    }

    /// Get the maintainer information from the environment.
    pub fn from_env() -> Option<Self> {
        get_maintainer().map(|(name, email)| Self { name, email })
    }
}

impl From<(String, String)> for Identity {
    fn from((name, email): (String, String)) -> Self {
        Self { name, email }
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} <{}>", self.name, self.email)
    }
}

/// Constant for the unreleased distribution name
pub const UNRELEASED: &str = "UNRELEASED";
/// Prefix for unreleased distribution variants
const UNRELEASED_PREFIX: &str = "UNRELEASED-";

/// Check if the given distribution marks an unreleased entry.
pub fn distribution_is_unreleased(distribution: &str) -> bool {
    distribution == UNRELEASED || distribution.starts_with(UNRELEASED_PREFIX)
}

/// Check if any of the given distributions marks an unreleased entry.
pub fn distributions_is_unreleased(distributions: &[&str]) -> bool {
    distributions.iter().any(|x| distribution_is_unreleased(x))
}

#[test]
fn test_distributions_is_unreleased() {
    assert!(distributions_is_unreleased(&["UNRELEASED"]));
    assert!(distributions_is_unreleased(&[
        "UNRELEASED-1",
        "UNRELEASED-2"
    ]));
    assert!(distributions_is_unreleased(&["UNRELEASED", "UNRELEASED-2"]));
    assert!(!distributions_is_unreleased(&["stable"]));
}

/// Check whether this is a traditional inaugural release
pub fn is_unreleased_inaugural(cl: &ChangeLog) -> bool {
    let mut entries = cl.iter();
    if let Some(entry) = entries.next() {
        if entry.is_unreleased() == Some(false) {
            return false;
        }
        let changes = entry.change_lines().collect::<Vec<_>>();
        if changes.len() > 1 || !changes[0].starts_with("* Initial release") {
            return false;
        }
        entries.next().is_none()
    } else {
        false
    }
}

#[cfg(test)]
mod is_unreleased_inaugural_tests {
    use super::*;

    #[test]
    fn test_empty() {
        assert!(!is_unreleased_inaugural(&ChangeLog::new()));
    }

    #[test]
    fn test_unreleased_inaugural() {
        let mut cl = ChangeLog::new();
        cl.new_entry()
            .maintainer(("Jelmer Vernooĳ".into(), "jelmer@debian.org".into()))
            .distribution(UNRELEASED.to_string())
            .version("1.0.0".parse().unwrap())
            .change_line("* Initial release".to_string())
            .finish();
        assert!(is_unreleased_inaugural(&cl));
    }

    #[test]
    fn test_not_unreleased_inaugural() {
        let mut cl = ChangeLog::new();
        cl.new_entry()
            .maintainer(("Jelmer Vernooĳ".into(), "jelmer@debian.org".into()))
            .distributions(vec!["unstable".to_string()])
            .version("1.0.0".parse().unwrap())
            .change_line("* Initial release".to_string())
            .finish();
        assert_eq!(cl.iter().next().unwrap().is_unreleased(), Some(false));

        // Not unreleased
        assert!(!is_unreleased_inaugural(&cl));

        cl.new_entry()
            .maintainer(("Jelmer Vernooĳ".into(), "jelmer@debian.org".into()))
            .distribution(UNRELEASED.to_string())
            .version("1.0.1".parse().unwrap())
            .change_line("* Some change".to_string())
            .finish();
        // Not inaugural
        assert!(!is_unreleased_inaugural(&cl));
    }
}

const DEFAULT_DISTRIBUTION: &[&str] = &[UNRELEASED];

/// Create a release for a changelog file.
///
/// # Arguments
/// * `cl` - The changelog to release
/// * `distribution` - The distribution to release to. If None, the distribution
///   of the previous entry is used.
/// * `timestamp` - The timestamp to use for the release. If None, the current time is used (requires chrono feature).
/// * `maintainer` - The maintainer to use for the release. If None, the maintainer
///   is extracted from the environment.
///
/// # Returns
/// Whether a release was created.
///
/// # Panics
/// Panics if timestamp is None and the chrono feature is not enabled.
pub fn release(
    cl: &mut ChangeLog,
    distribution: Option<Vec<String>>,
    timestamp: Option<impl IntoTimestamp>,
    maintainer: Option<(String, String)>,
) -> bool {
    let mut entries = cl.iter();
    let mut first_entry = entries.next().unwrap();
    let second_entry = entries.next();
    let distribution = distribution.unwrap_or_else(|| {
        // Inherit from previous entry
        second_entry
            .and_then(|e| e.distributions())
            .unwrap_or_else(|| {
                DEFAULT_DISTRIBUTION
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
    });
    if first_entry.is_unreleased() == Some(false) {
        take_uploadership(&mut first_entry, maintainer);
        first_entry.set_distributions(distribution);
        let timestamp_str = if let Some(ts) = timestamp {
            ts.into_timestamp()
        } else {
            #[cfg(feature = "chrono")]
            {
                chrono::offset::Utc::now().into_timestamp()
            }
            #[cfg(not(feature = "chrono"))]
            {
                panic!("timestamp is required when chrono feature is disabled");
            }
        };
        first_entry.set_timestamp(timestamp_str);
        true
    } else {
        false
    }
}

/// Take uploadership of a changelog entry, but attribute contributors.
///
/// # Arguments
/// * `entry` - Changelog entry to modify
/// * `maintainer` - Tuple with (name, email) of maintainer to take ownership
pub fn take_uploadership(entry: &mut Entry, maintainer: Option<(String, String)>) {
    let (maintainer_name, maintainer_email) = if let Some(m) = maintainer {
        m
    } else {
        get_maintainer().unwrap()
    };
    if let (Some(current_maintainer), Some(current_email)) = (entry.maintainer(), entry.email()) {
        if current_maintainer != maintainer_name || current_email != maintainer_email {
            if let Some(first_line) = entry.change_lines().next() {
                if first_line.starts_with("[ ") {
                    entry.prepend_change_line(
                        crate::changes::format_section_title(current_maintainer.as_str()).as_str(),
                    );
                }
            }
        }
    }
    entry.set_maintainer((maintainer_name, maintainer_email));
}

/// Update changelog with commit messages from commits
pub fn gbp_dch(path: &std::path::Path) -> std::result::Result<(), std::io::Error> {
    // Run the "gbp dch" command with working copy at `path`
    let output = std::process::Command::new("gbp")
        .arg("dch")
        .arg("--ignore-branch")
        .current_dir(path)
        .output()?;

    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "gbp dch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(())
}

/// Iterator over changelog entries grouped by author (maintainer).
///
/// This function returns an iterator that groups changelog entries by their maintainer
/// (author), similar to debmutate.changelog functionality.
///
/// # Arguments
/// * `changelog` - The changelog to iterate over
///
/// # Returns
/// An iterator over tuples of (author_name, author_email, Vec<Entry>)
pub fn iter_entries_by_author(
    changelog: &ChangeLog,
) -> impl Iterator<Item = (String, String, Vec<Entry>)> + '_ {
    use std::collections::BTreeMap;

    let mut grouped: BTreeMap<(String, String), Vec<Entry>> = BTreeMap::new();

    for entry in changelog.iter() {
        let maintainer_name = entry.maintainer().unwrap_or_else(|| "Unknown".to_string());
        let maintainer_email = entry
            .email()
            .unwrap_or_else(|| "unknown@unknown".to_string());
        let key = (maintainer_name, maintainer_email);

        grouped.entry(key).or_default().push(entry);
    }

    grouped
        .into_iter()
        .map(|((name, email), entries)| (name, email, entries))
}

/// Iterator over all changes across all entries, grouped by author.
///
/// This function iterates through all entries in a changelog and returns changes
/// grouped by their attributed authors, including those in author sections like [ Author Name ].
///
/// # Arguments
/// * `changelog` - The changelog to iterate over
///
/// # Returns
/// A vector of Change objects that can be manipulated or filtered
pub fn iter_changes_by_author(changelog: &ChangeLog) -> Vec<Change> {
    let mut result = Vec::new();

    for entry in changelog.iter() {
        let changes: Vec<String> = entry.change_lines().map(|s| s.to_string()).collect();

        // Collect all DETAIL tokens from the entry with their text
        let all_detail_tokens: Vec<SyntaxToken> = entry
            .syntax()
            .children()
            .flat_map(|n| {
                n.children_with_tokens()
                    .filter_map(|it| it.as_token().cloned())
                    .filter(|token| token.kind() == SyntaxKind::DETAIL)
            })
            .collect();

        // Track which tokens have been used to avoid matching duplicates to the same token
        let mut token_index = 0;

        for (author, linenos, lines) in
            crate::changes::changes_by_author(changes.iter().map(|s| s.as_str()))
        {
            let author_name = author.map(|s| s.to_string());

            // Extract the specific DETAIL tokens for this change by matching text content
            // We iterate through tokens in order to handle duplicate lines correctly
            let detail_tokens: Vec<SyntaxToken> = lines
                .iter()
                .filter_map(|line_text| {
                    // Find the next token matching this line's text
                    while token_index < all_detail_tokens.len() {
                        let token = &all_detail_tokens[token_index];
                        token_index += 1;
                        if token.text() == *line_text {
                            return Some(token.clone());
                        }
                    }
                    None
                })
                .collect();

            let change = Change::new(entry.clone(), author_name, linenos, detail_tokens);
            result.push(change);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parseaddr() {
        assert_eq!(
            (Some("Jelmer"), "jelmer@jelmer.uk"),
            parseaddr("Jelmer <jelmer@jelmer.uk>")
        );
        assert_eq!((None, "jelmer@jelmer.uk"), parseaddr("jelmer@jelmer.uk"));
    }

    #[test]
    fn test_parseaddr_empty() {
        assert_eq!((None, ""), parseaddr(""));
    }

    #[test]
    #[cfg(feature = "chrono")]
    fn test_release_already_released() {
        use crate::parse::ChangeLog;

        let mut changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let result = release(
            &mut changelog,
            Some(vec!["unstable".to_string()]),
            None::<String>,
            None,
        );

        // The function returns true if the entry is NOT unreleased (already released)
        assert!(result);
    }

    #[test]
    #[cfg(feature = "chrono")]
    fn test_release_unreleased() {
        use crate::parse::ChangeLog;

        let mut changelog: ChangeLog = r#"breezy (3.3.4-1) UNRELEASED; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let result = release(
            &mut changelog,
            Some(vec!["unstable".to_string()]),
            None::<String>,
            Some(("Test User".to_string(), "test@example.com".to_string())),
        );

        // The function returns false if the entry is unreleased
        assert!(!result);
    }

    #[test]
    fn test_take_uploadership_same_maintainer() {
        use crate::parse::ChangeLog;

        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Test User <test@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let mut entries: Vec<Entry> = changelog.into_iter().collect();
        take_uploadership(
            &mut entries[0],
            Some(("Test User".to_string(), "test@example.com".to_string())),
        );

        // Should not add author section when maintainer is the same
        assert!(!entries[0].to_string().contains("[ Test User ]"));
    }

    #[test]
    fn test_take_uploadership_different_maintainer() {
        use crate::parse::ChangeLog;

        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Original User <original@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let mut entries: Vec<Entry> = changelog.into_iter().collect();

        take_uploadership(
            &mut entries[0],
            Some(("New User".to_string(), "new@example.com".to_string())),
        );

        // The take_uploadership function updates the maintainer in the footer
        assert!(entries[0]
            .to_string()
            .contains("New User <new@example.com>"));
        assert_eq!(entries[0].email(), Some("new@example.com".to_string()));
    }

    #[test]
    fn test_identity_display() {
        let identity = Identity {
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
        };
        assert_eq!(identity.to_string(), "Test User <test@example.com>");

        let identity_empty_name = Identity {
            name: "".to_string(),
            email: "test@example.com".to_string(),
        };
        assert_eq!(identity_empty_name.to_string(), " <test@example.com>");
    }

    #[test]
    fn test_gbp_dch_failure() {
        // Test with invalid path that would cause gbp dch to fail
        let result = gbp_dch(std::path::Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_iter_entries_by_author() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500

breezy (3.3.3-1) unstable; urgency=low

  * Bug fix release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sun, 03 Sep 2023 17:12:30 -0500

breezy (3.3.2-1) unstable; urgency=low

  * Another release.

 -- Jane Doe <jane@example.com>  Sat, 02 Sep 2023 16:11:15 -0500
"#
        .parse()
        .unwrap();

        let authors: Vec<(String, String, Vec<Entry>)> =
            iter_entries_by_author(&changelog).collect();

        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0].0, "Jane Doe");
        assert_eq!(authors[0].1, "jane@example.com");
        assert_eq!(authors[0].2.len(), 1);
        assert_eq!(authors[1].0, "Jelmer Vernooĳ");
        assert_eq!(authors[1].1, "jelmer@debian.org");
        assert_eq!(authors[1].2.len(), 2);
    }

    #[test]
    fn test_iter_changes_by_author() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Author 1 ]
  * Change by Author 1

  [ Author 2 ]
  * Change by Author 2

  * Unattributed change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);

        assert_eq!(changes.len(), 3);

        // First change attributed to Author 1
        assert_eq!(changes[0].author(), Some("Author 1"));
        assert_eq!(changes[0].package(), Some("breezy".to_string()));
        assert_eq!(changes[0].lines(), vec!["* Change by Author 1"]);

        // Second change attributed to Author 2
        assert_eq!(changes[1].author(), Some("Author 2"));
        assert_eq!(changes[1].package(), Some("breezy".to_string()));
        assert_eq!(changes[1].lines(), vec!["* Change by Author 2"]);

        // Third change unattributed
        assert_eq!(changes[2].author(), None);
        assert_eq!(changes[2].package(), Some("breezy".to_string()));
        assert_eq!(changes[2].lines(), vec!["* Unattributed change"]);
    }

    #[test]
    fn test_change_remove() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Author 1 ]
  * Change by Author 1

  [ Author 2 ]
  * Change by Author 2

  * Unattributed change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 3);

        // Remove the second change (Author 2)
        changes[1].clone().remove();

        // Re-read the changes
        let remaining_changes = iter_changes_by_author(&changelog);

        // The Author 2 section header remains but with no changes,
        // so it will show up as an empty change for Author 2,
        // followed by the unattributed change
        assert_eq!(remaining_changes.len(), 2);

        // Should have Author 1 and Author 2 (but with no lines)
        assert_eq!(remaining_changes[0].author(), Some("Author 1"));
        assert_eq!(remaining_changes[0].lines(), vec!["* Change by Author 1"]);

        // Author 2's section header remains but the change is removed
        assert_eq!(remaining_changes[1].author(), Some("Author 2"));
        assert_eq!(remaining_changes[1].lines(), vec!["* Unattributed change"]);
    }

    #[test]
    fn test_change_replace_with() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Author 1 ]
  * Change by Author 1

  [ Author 2 ]
  * Change by Author 2

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 2);

        // Replace Author 2's change
        changes[1].replace_with(vec!["* Updated change by Author 2", "* Another line"]);

        // Re-read the changes
        let updated_changes = iter_changes_by_author(&changelog);
        assert_eq!(updated_changes.len(), 2);

        // Author 1's change should be unchanged
        assert_eq!(updated_changes[0].author(), Some("Author 1"));
        assert_eq!(updated_changes[0].lines(), vec!["* Change by Author 1"]);

        // Author 2's change should be replaced
        assert_eq!(updated_changes[1].author(), Some("Author 2"));
        assert_eq!(
            updated_changes[1].lines(),
            vec!["* Updated change by Author 2", "* Another line"]
        );
    }

    #[test]
    fn test_change_replace_with_single_line() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * Old change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 1);

        // Replace with a new single line
        changes[0].replace_with(vec!["* New change"]);

        // Re-read the changes
        let updated_changes = iter_changes_by_author(&changelog);
        assert_eq!(updated_changes.len(), 1);
        assert_eq!(updated_changes[0].lines(), vec!["* New change"]);
    }

    #[test]
    fn test_change_accessors() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * Change by Alice

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 1);

        let change = &changes[0];

        // Test all accessors
        assert_eq!(change.author(), Some("Alice"));
        assert_eq!(change.package(), Some("breezy".to_string()));
        assert_eq!(
            change.version().map(|v| v.to_string()),
            Some("3.3.4-1".to_string())
        );
        assert_eq!(change.is_attributed(), true);
        assert_eq!(change.lines(), vec!["* Change by Alice"]);

        // Test entry accessor
        assert_eq!(change.entry().package(), Some("breezy".to_string()));
    }

    #[test]
    fn test_change_unattributed_accessors() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * Unattributed change

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 1);

        let change = &changes[0];
        assert_eq!(change.author(), None);
        assert_eq!(change.is_attributed(), false);
    }

    #[test]
    fn test_replace_single_line_with_multiple() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * Single line change

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        changes[0].replace_with(vec!["* First line", "* Second line", "* Third line"]);

        let updated = iter_changes_by_author(&changelog);
        assert_eq!(
            updated[0].lines(),
            vec!["* First line", "* Second line", "* Third line"]
        );
    }

    #[test]
    fn test_replace_multiple_lines_with_single() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First line
  * Second line
  * Third line

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes[0].lines().len(), 3);

        changes[0].replace_with(vec!["* Single replacement line"]);

        let updated = iter_changes_by_author(&changelog);
        assert_eq!(updated[0].lines(), vec!["* Single replacement line"]);
    }

    #[test]
    fn test_split_into_bullets_single_line() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change
  * Second change
  * Third change

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 1);

        // Split the single Change into individual bullets
        let bullets = changes[0].split_into_bullets();

        assert_eq!(bullets.len(), 3);
        assert_eq!(bullets[0].lines(), vec!["* First change"]);
        assert_eq!(bullets[1].lines(), vec!["* Second change"]);
        assert_eq!(bullets[2].lines(), vec!["* Third change"]);

        // Each bullet should have the same package and version
        for bullet in &bullets {
            assert_eq!(bullet.package(), Some("breezy".to_string()));
            assert_eq!(
                bullet.version().map(|v| v.to_string()),
                Some("3.3.4-1".to_string())
            );
        }
    }

    #[test]
    fn test_split_into_bullets_with_continuations() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change
    with a continuation line
  * Second change
    with multiple
    continuation lines
  * Third change

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 1);

        let bullets = changes[0].split_into_bullets();

        assert_eq!(bullets.len(), 3);
        assert_eq!(
            bullets[0].lines(),
            vec!["* First change", "  with a continuation line"]
        );
        assert_eq!(
            bullets[1].lines(),
            vec!["* Second change", "  with multiple", "  continuation lines"]
        );
        assert_eq!(bullets[2].lines(), vec!["* Third change"]);
    }

    #[test]
    fn test_split_into_bullets_mixed() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * Single line bullet
  * Multi-line bullet
    with continuation
  * Another single line

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        let bullets = changes[0].split_into_bullets();

        assert_eq!(bullets.len(), 3);
        assert_eq!(bullets[0].lines(), vec!["* Single line bullet"]);
        assert_eq!(
            bullets[1].lines(),
            vec!["* Multi-line bullet", "  with continuation"]
        );
        assert_eq!(bullets[2].lines(), vec!["* Another single line"]);
    }

    #[test]
    fn test_split_into_bullets_with_author() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * Change by Alice
  * Another change by Alice

  [ Bob ]
  * Change by Bob

 -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 2);

        // Split Alice's changes
        let alice_bullets = changes[0].split_into_bullets();
        assert_eq!(alice_bullets.len(), 2);
        assert_eq!(alice_bullets[0].lines(), vec!["* Change by Alice"]);
        assert_eq!(alice_bullets[1].lines(), vec!["* Another change by Alice"]);

        // Both bullets should preserve the author
        for bullet in &alice_bullets {
            assert_eq!(bullet.author(), Some("Alice"));
        }

        // Split Bob's changes
        let bob_bullets = changes[1].split_into_bullets();
        assert_eq!(bob_bullets.len(), 1);
        assert_eq!(bob_bullets[0].lines(), vec!["* Change by Bob"]);
        assert_eq!(bob_bullets[0].author(), Some("Bob"));
    }

    #[test]
    fn test_split_into_bullets_single_bullet() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * Single bullet point

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        let bullets = changes[0].split_into_bullets();

        assert_eq!(bullets.len(), 1);
        assert_eq!(bullets[0].lines(), vec!["* Single bullet point"]);
    }

    #[test]
    fn test_split_into_bullets_and_remove() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change
  * Duplicate change
  * Duplicate change
  * Last change

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        let bullets = changes[0].split_into_bullets();

        assert_eq!(bullets.len(), 4);

        // Remove the second duplicate (index 2)
        bullets[2].clone().remove();

        // Re-read and verify
        let updated_changes = iter_changes_by_author(&changelog);
        let updated_bullets = updated_changes[0].split_into_bullets();

        assert_eq!(updated_bullets.len(), 3);
        assert_eq!(updated_bullets[0].lines(), vec!["* First change"]);
        assert_eq!(updated_bullets[1].lines(), vec!["* Duplicate change"]);
        assert_eq!(updated_bullets[2].lines(), vec!["* Last change"]);
    }

    #[test]
    fn test_split_into_bullets_preserves_line_numbers() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change
  * Second change
  * Third change

 -- Bob <bob@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        let bullets = changes[0].split_into_bullets();

        // Each bullet should have distinct line numbers
        assert_eq!(bullets.len(), 3);
        assert_eq!(bullets[0].line_numbers().len(), 1);
        assert_eq!(bullets[1].line_numbers().len(), 1);
        assert_eq!(bullets[2].line_numbers().len(), 1);

        // Line numbers should be in ascending order
        assert!(bullets[0].line_numbers()[0] < bullets[1].line_numbers()[0]);
        assert!(bullets[1].line_numbers()[0] < bullets[2].line_numbers()[0]);
    }

    #[test]
    fn test_split_and_remove_from_multi_author_entry() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * Change 1 by Alice
  * Change 2 by Alice
  * Change 3 by Alice

  [ Bob ]
  * Change 1 by Bob
  * Change 2 by Bob

  [ Charlie ]
  * Change 1 by Charlie

  * Unattributed change

 -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 4); // Alice, Bob, Charlie, Unattributed

        // Split Alice's changes and remove the second one
        let alice_bullets = changes[0].split_into_bullets();
        assert_eq!(alice_bullets.len(), 3);
        alice_bullets[1].clone().remove(); // Remove "Change 2 by Alice"

        // Re-read and verify
        let updated_changes = iter_changes_by_author(&changelog);
        assert_eq!(updated_changes.len(), 4);

        // Alice should now have 2 changes
        let updated_alice_bullets = updated_changes[0].split_into_bullets();
        assert_eq!(updated_alice_bullets.len(), 2);
        assert_eq!(
            updated_alice_bullets[0].lines(),
            vec!["* Change 1 by Alice"]
        );
        assert_eq!(
            updated_alice_bullets[1].lines(),
            vec!["* Change 3 by Alice"]
        );
        assert_eq!(updated_alice_bullets[0].author(), Some("Alice"));

        // Bob should be unchanged
        let bob_bullets = updated_changes[1].split_into_bullets();
        assert_eq!(bob_bullets.len(), 2);
        assert_eq!(bob_bullets[0].lines(), vec!["* Change 1 by Bob"]);
        assert_eq!(bob_bullets[1].lines(), vec!["* Change 2 by Bob"]);

        // Charlie should be unchanged
        let charlie_bullets = updated_changes[2].split_into_bullets();
        assert_eq!(charlie_bullets.len(), 1);
        assert_eq!(charlie_bullets[0].lines(), vec!["* Change 1 by Charlie"]);

        // Unattributed should be unchanged
        let unattributed_bullets = updated_changes[3].split_into_bullets();
        assert_eq!(unattributed_bullets.len(), 1);
        assert_eq!(
            unattributed_bullets[0].lines(),
            vec!["* Unattributed change"]
        );
    }

    #[test]
    fn test_remove_multiple_bullets_from_different_authors() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * Alice change 1
  * Alice change 2
  * Alice change 3

  [ Bob ]
  * Bob change 1
  * Bob change 2
  * Bob change 3

 -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 2);

        // Remove Alice's first and third changes
        let alice_bullets = changes[0].split_into_bullets();
        alice_bullets[0].clone().remove();
        alice_bullets[2].clone().remove();

        // Remove Bob's second change
        let bob_bullets = changes[1].split_into_bullets();
        bob_bullets[1].clone().remove();

        // Re-read and verify
        let updated_changes = iter_changes_by_author(&changelog);

        let updated_alice = updated_changes[0].split_into_bullets();
        assert_eq!(updated_alice.len(), 1);
        assert_eq!(updated_alice[0].lines(), vec!["* Alice change 2"]);

        let updated_bob = updated_changes[1].split_into_bullets();
        assert_eq!(updated_bob.len(), 2);
        assert_eq!(updated_bob[0].lines(), vec!["* Bob change 1"]);
        assert_eq!(updated_bob[1].lines(), vec!["* Bob change 3"]);
    }

    #[test]
    fn test_remove_bullet_with_continuation_from_multi_author() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * Simple change by Alice

  [ Bob ]
  * Multi-line change by Bob
    with a continuation line
    and another continuation
  * Simple change by Bob

  [ Charlie ]
  * Change by Charlie

 -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 3);

        // Remove Bob's multi-line change
        let bob_bullets = changes[1].split_into_bullets();
        assert_eq!(bob_bullets.len(), 2);
        assert_eq!(
            bob_bullets[0].lines(),
            vec![
                "* Multi-line change by Bob",
                "  with a continuation line",
                "  and another continuation"
            ]
        );
        bob_bullets[0].clone().remove();

        // Re-read and verify
        let updated_changes = iter_changes_by_author(&changelog);

        // Alice unchanged
        let alice_bullets = updated_changes[0].split_into_bullets();
        assert_eq!(alice_bullets.len(), 1);
        assert_eq!(alice_bullets[0].lines(), vec!["* Simple change by Alice"]);

        // Bob now has only the simple change
        let updated_bob = updated_changes[1].split_into_bullets();
        assert_eq!(updated_bob.len(), 1);
        assert_eq!(updated_bob[0].lines(), vec!["* Simple change by Bob"]);

        // Charlie unchanged
        let charlie_bullets = updated_changes[2].split_into_bullets();
        assert_eq!(charlie_bullets.len(), 1);
        assert_eq!(charlie_bullets[0].lines(), vec!["* Change by Charlie"]);
    }

    #[test]
    fn test_remove_all_bullets_from_one_author_section() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * Change 1 by Alice
  * Change 2 by Alice

  [ Bob ]
  * Change 1 by Bob

 -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 2);

        // Remove all of Alice's changes
        let alice_bullets = changes[0].split_into_bullets();
        for bullet in alice_bullets {
            bullet.remove();
        }

        // Re-read and verify
        let updated_changes = iter_changes_by_author(&changelog);

        // Alice's section header remains but with no changes
        // Bob's section follows with its change, so only Bob's change remains
        assert_eq!(updated_changes.len(), 1);
        assert_eq!(updated_changes[0].author(), Some("Bob"));

        let bob_bullets = updated_changes[0].split_into_bullets();
        assert_eq!(bob_bullets.len(), 1);
        assert_eq!(bob_bullets[0].lines(), vec!["* Change 1 by Bob"]);

        // Verify the section header was removed from the changelog text
        let changelog_text = changelog.to_string();
        assert!(
            !changelog_text.contains("[ Alice ]"),
            "Alice's empty section header should be removed"
        );
    }

    #[test]
    fn test_remove_section_header_with_multiple_sections() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * Alice's first section change

  [ Bob ]
  * Bob's change

  [ Alice ]
  * Alice's second section change 1
  * Alice's second section change 2

 -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 3);

        // Remove all changes from the second Alice section only
        let alice_second = &changes[2];
        assert_eq!(alice_second.author(), Some("Alice"));

        let alice_second_bullets = alice_second.split_into_bullets();
        assert_eq!(alice_second_bullets.len(), 2);

        // Remove all bullets from the second Alice section
        for bullet in alice_second_bullets {
            bullet.remove();
        }

        // Re-read and verify
        let updated_changes = iter_changes_by_author(&changelog);

        // Should now only have Alice's first section and Bob's section
        assert_eq!(updated_changes.len(), 2);
        assert_eq!(updated_changes[0].author(), Some("Alice"));
        assert_eq!(updated_changes[1].author(), Some("Bob"));

        // Verify the first Alice section is intact
        let alice_first = updated_changes[0].split_into_bullets();
        assert_eq!(alice_first.len(), 1);
        assert_eq!(
            alice_first[0].lines(),
            vec!["* Alice's first section change"]
        );

        // Verify the changelog text - second Alice header should be gone
        let changelog_text = changelog.to_string();
        let alice_header_count = changelog_text.matches("[ Alice ]").count();
        assert_eq!(
            alice_header_count, 1,
            "Should only have one Alice section header remaining"
        );
    }

    #[test]
    fn test_remove_duplicate_from_specific_author() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * New upstream release
  * Fix typo in documentation
  * New upstream release

  [ Bob ]
  * New upstream release
  * Update dependencies

 -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 2);

        // Find and remove duplicate "New upstream release" from Alice
        let alice_bullets = changes[0].split_into_bullets();
        assert_eq!(alice_bullets.len(), 3);

        // Verify the order before removal
        assert_eq!(alice_bullets[0].lines(), vec!["* New upstream release"]);
        assert_eq!(
            alice_bullets[1].lines(),
            vec!["* Fix typo in documentation"]
        );
        assert_eq!(alice_bullets[2].lines(), vec!["* New upstream release"]);

        // Remove the duplicate (third item)
        alice_bullets[2].clone().remove();

        // Re-read and verify
        let updated_changes = iter_changes_by_author(&changelog);

        // Alice should now have 2 changes (first "New upstream release" and "Fix typo")
        let updated_alice = updated_changes[0].split_into_bullets();
        assert_eq!(updated_alice.len(), 2);
        assert_eq!(updated_alice[0].lines(), vec!["* New upstream release"]);
        assert_eq!(
            updated_alice[1].lines(),
            vec!["* Fix typo in documentation"]
        );

        // Bob should be unchanged
        let bob_bullets = updated_changes[1].split_into_bullets();
        assert_eq!(bob_bullets.len(), 2);
        assert_eq!(bob_bullets[0].lines(), vec!["* New upstream release"]);
        assert_eq!(bob_bullets[1].lines(), vec!["* Update dependencies"]);
    }

    #[test]
    fn test_remove_empty_section_headers_and_blank_lines() {
        // Test that when all bullets are removed from a section, the section header
        // and its preceding blank line are also removed
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * Change 1 by Alice
  * Change 2 by Alice

  [ Bob ]
  * Change 1 by Bob

 -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 2);

        // Remove all of Alice's changes
        let alice_bullets = changes[0].split_into_bullets();
        for bullet in alice_bullets {
            bullet.remove();
        }

        // Verify Alice's section is completely gone
        let updated_changes = iter_changes_by_author(&changelog);
        assert_eq!(updated_changes.len(), 1);
        assert_eq!(updated_changes[0].author(), Some("Bob"));

        // Verify the changelog text has no Alice header or extra blank lines
        let changelog_text = changelog.to_string();
        assert!(!changelog_text.contains("[ Alice ]"));

        // Count blank lines before signature - should be exactly 1
        let lines: Vec<&str> = changelog_text.lines().collect();
        let sig_idx = lines.iter().position(|l| l.starts_with(" --")).unwrap();
        let mut blank_count = 0;
        for i in (0..sig_idx).rev() {
            if lines[i].trim().is_empty() {
                blank_count += 1;
            } else {
                break;
            }
        }
        assert_eq!(
            blank_count, 1,
            "Should have exactly 1 blank line before signature"
        );
    }

    #[test]
    fn test_remove_first_entry_before_author_section() {
        // Test that when removing the first entry before an author section,
        // the extra newline is properly removed
        let changelog: ChangeLog = r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * Team upload.

  [ Jelmer Vernooĳ ]
  * blah

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000

lintian-brush (0.1-1) unstable; urgency=medium

  * Initial release. (Closes: #XXXXXX)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sun, 28 Oct 2018 00:09:52 +0000
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);

        // Find and remove the "Team upload" entry (should be the first one, unattributed)
        let team_upload_change = changes
            .iter()
            .find(|c| c.lines().iter().any(|l| l.contains("Team upload")))
            .unwrap();

        team_upload_change.clone().remove();

        let expected = r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  [ Jelmer Vernooĳ ]
  * blah

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000

lintian-brush (0.1-1) unstable; urgency=medium

  * Initial release. (Closes: #XXXXXX)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sun, 28 Oct 2018 00:09:52 +0000
"#;

        assert_eq!(changelog.to_string(), expected);
    }

    // Helper function for remove tests to reduce repetition
    // Splits changes into individual bullets before applying filter
    fn test_remove_change(input: &str, change_filter: impl Fn(&Change) -> bool, expected: &str) {
        let changelog: ChangeLog = input.parse().unwrap();
        let changes = iter_changes_by_author(&changelog);

        // Split all changes into individual bullets
        let mut all_bullets = Vec::new();
        for change in changes {
            all_bullets.extend(change.split_into_bullets());
        }

        let change = all_bullets.iter().find(|c| change_filter(c)).unwrap();
        change.clone().remove();
        assert_eq!(changelog.to_string(), expected);
    }

    #[test]
    fn test_remove_entry_followed_by_regular_bullet() {
        // Empty line should be preserved when followed by a regular bullet, not a section header
        test_remove_change(
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * First change.

  * Second change.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
            |c| c.lines().iter().any(|l| l.contains("First change")),
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * Second change.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
        );
    }

    #[test]
    fn test_remove_entry_not_followed_by_empty_line() {
        // No trailing empty line to remove
        test_remove_change(
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * First change.
  * Second change.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
            |c| c.lines().iter().any(|l| l.contains("First change")),
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * Second change.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
        );
    }

    #[test]
    fn test_remove_only_entry() {
        // Empty line before footer should be preserved
        test_remove_change(
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * Only change.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
            |c| c.lines().iter().any(|l| l.contains("Only change")),
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
        );
    }

    #[test]
    fn test_remove_middle_entry_between_bullets() {
        // Empty lines around remaining bullets should be preserved
        test_remove_change(
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * First change.

  * Middle change.

  * Last change.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
            |c| c.lines().iter().any(|l| l.contains("Middle change")),
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * First change.

  * Last change.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
        );
    }

    #[test]
    fn test_remove_entry_before_multiple_section_headers() {
        // Empty line before first section header should be removed
        test_remove_change(
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * Team upload.

  [ Author One ]
  * Change by author one.

  [ Author Two ]
  * Change by author two.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
            |c| c.lines().iter().any(|l| l.contains("Team upload")),
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  [ Author One ]
  * Change by author one.

  [ Author Two ]
  * Change by author two.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
        );
    }

    #[test]
    fn test_remove_first_of_two_section_headers() {
        // Empty line before remaining section should be preserved
        test_remove_change(
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  [ Author One ]
  * Change by author one.

  [ Author Two ]
  * Change by author two.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
            |c| c.author() == Some("Author One"),
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  [ Author Two ]
  * Change by author two.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
        );
    }

    #[test]
    fn test_remove_last_entry_no_empty_line_follows() {
        // Edge case: last entry with no trailing empty before footer
        test_remove_change(
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * First change.
  * Last change.
 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
            |c| c.lines().iter().any(|l| l.contains("Last change")),
            r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * First change.
 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#,
        );
    }

    #[test]
    fn test_remove_first_unattributed_before_section_exact() {
        // Exact reproduction of the lintian-brush test case
        // Using the exact sequence: iter_changes_by_author -> split_into_bullets -> remove
        let changelog: ChangeLog = r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  * Team upload.

  [ Jelmer Vernooĳ ]
  * blah

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#
        .parse()
        .unwrap();

        // Exact sequence from lintian-brush: iter_changes_by_author -> split_into_bullets -> remove
        let changes = iter_changes_by_author(&changelog);
        let team_upload_change = changes
            .iter()
            .find(|c| c.author().is_none() && c.lines().iter().any(|l| l.contains("Team upload")))
            .unwrap();

        let bullets = team_upload_change.split_into_bullets();
        bullets[0].clone().remove();

        let result = changelog.to_string();

        // Should have exactly one blank line after header, not two
        let expected = r#"lintian-brush (0.1-2) UNRELEASED; urgency=medium

  [ Jelmer Vernooĳ ]
  * blah

 -- Jelmer Vernooĳ <jelmer@debian.org>  Fri, 23 Nov 2018 14:00:02 +0000
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_replace_with_preserves_first_blank_line() {
        // Test that replace_with preserves the blank line after the entry header
        // This reproduces the issue from debian-changelog-line-too-long/subitem test
        let changelog: ChangeLog = r#"blah (2.6.0) unstable; urgency=medium

  * New upstream release.
   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);

        // Replace all changes with wrapped version
        changes[0].replace_with(vec![
            "* New upstream release.",
            " * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to",
            "   somebody who fixed it.",
        ]);

        let result = changelog.to_string();

        // The blank line after the header should be preserved
        let expected = r#"blah (2.6.0) unstable; urgency=medium

  * New upstream release.
   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to
     somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_serialize_preserves_blank_line() {
        // Test that simply parsing and serializing preserves the blank line
        let input = r#"blah (2.6.0) unstable; urgency=medium

  * New upstream release.
   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;

        let changelog: ChangeLog = input.parse().unwrap();
        let output = changelog.to_string();

        assert_eq!(output, input, "Parse/serialize should not modify changelog");
    }

    #[test]
    fn test_replace_with_first_entry_preserves_blank() {
        // Simulate what a Rust line-too-long fixer would do:
        // Replace the changes in the first entry with wrapped versions
        let changelog: ChangeLog = r#"blah (2.6.0) unstable; urgency=medium

  * New upstream release.
   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 1);

        // Replace with wrapped version (what the fixer would do)
        changes[0].replace_with(vec![
            "* New upstream release.",
            " * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to",
            "   somebody who fixed it.",
        ]);

        let result = changelog.to_string();

        // The blank line after header MUST be preserved
        let expected = r#"blah (2.6.0) unstable; urgency=medium

  * New upstream release.
   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to
     somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_pop_append_preserves_first_blank() {
        // Test the exact pattern used by the Rust line-too-long fixer:
        // pop all lines, then append wrapped ones
        let changelog: ChangeLog = r#"blah (2.6.0) unstable; urgency=medium

  * New upstream release.
   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#
        .parse()
        .unwrap();

        let entry = changelog.iter().next().unwrap();

        // Pop all change lines (simulating the fixer)
        while entry.pop_change_line().is_some() {}

        // Append wrapped lines
        entry.append_change_line("* New upstream release.");
        entry.append_change_line(
            " * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to",
        );
        entry.append_change_line("   somebody who fixed it.");

        let result = changelog.to_string();

        // The blank line after header MUST be preserved
        let expected = r#"blah (2.6.0) unstable; urgency=medium

  * New upstream release.
   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to
     somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_replace_line() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change
  * Second change
  * Third change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 1);

        // Replace the second line
        changes[0]
            .replace_line(1, "* Updated second change")
            .unwrap();

        // Re-read and verify
        let updated_changes = iter_changes_by_author(&changelog);
        assert_eq!(
            updated_changes[0].lines(),
            vec![
                "* First change",
                "* Updated second change",
                "* Third change"
            ]
        );
    }

    #[test]
    fn test_replace_line_out_of_bounds() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);
        assert_eq!(changes.len(), 1);

        // Try to replace a line that doesn't exist
        let result = changes[0].replace_line(5, "* Updated");
        assert!(result.is_err());
    }

    #[test]
    fn test_update_lines() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change
  * Second change
  * Third change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);

        // Update lines containing "First" or "Second"
        let count = changes[0].update_lines(
            |line| line.contains("First") || line.contains("Second"),
            |line| format!("{} (updated)", line),
        );

        assert_eq!(count, 2);

        // Verify the changes
        let updated_changes = iter_changes_by_author(&changelog);
        assert_eq!(
            updated_changes[0].lines(),
            vec![
                "* First change (updated)",
                "* Second change (updated)",
                "* Third change"
            ]
        );
    }

    #[test]
    fn test_update_lines_no_matches() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change
  * Second change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);

        // Update lines that don't exist
        let count = changes[0].update_lines(
            |line| line.contains("NonExistent"),
            |line| format!("{} (updated)", line),
        );

        assert_eq!(count, 0);

        // Verify nothing changed
        let updated_changes = iter_changes_by_author(&changelog);
        assert_eq!(
            updated_changes[0].lines(),
            vec!["* First change", "* Second change"]
        );
    }

    #[test]
    fn test_update_lines_with_continuation() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change
    with continuation line
  * Second change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);

        // Update the continuation line
        let count = changes[0].update_lines(
            |line| line.contains("continuation"),
            |line| line.replace("continuation", "updated"),
        );

        assert_eq!(count, 1);

        // Verify the changes
        let updated_changes = iter_changes_by_author(&changelog);
        assert_eq!(
            updated_changes[0].lines(),
            vec!["* First change", "  with updated line", "* Second change"]
        );
    }

    #[test]
    fn test_add_bullet() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        // Add bullets - always prepends "* " automatically
        entry.add_bullet("First change");
        entry.add_bullet("Second change");
        entry.add_bullet("Third change");

        let lines: Vec<_> = entry.change_lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "* First change");
        assert_eq!(lines[1], "* Second change");
        assert_eq!(lines[2], "* Third change");
    }

    #[test]
    fn test_add_bullet_empty_entry() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        entry.add_bullet("Only bullet");

        let lines: Vec<_> = entry.change_lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "* Only bullet");
    }

    #[test]
    fn test_add_bullet_long_text() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        // Add a bullet with text that's too long and should be wrapped
        entry.add_bullet("This is a very long line that exceeds the 78 column limit and should be automatically wrapped to multiple lines with proper indentation");

        let lines: Vec<_> = entry.change_lines().collect();
        // Should be wrapped into multiple lines
        assert!(lines.len() > 1);
        // First line should start with "* "
        assert!(lines[0].starts_with("* "));
        // Continuation lines should start with "  " (two spaces)
        for line in &lines[1..] {
            assert!(line.starts_with("  "));
        }
        // No line should exceed 78 characters
        for line in &lines {
            assert!(line.len() <= 78, "Line exceeds 78 chars: {}", line);
        }
    }

    #[test]
    fn test_add_bullet_preserves_closes() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        // Add a bullet with "Closes: #" that should not be broken
        entry.add_bullet("Fix a very important bug that was causing problems (Closes: #123456)");

        let lines: Vec<_> = entry.change_lines().collect();
        let text = lines.join(" ");
        // "Closes: #123456" should not be split across lines
        assert!(text.contains("Closes: #123456"));
    }

    #[test]
    fn test_add_bullet_multiple_closes() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        // Add bullet with multiple bug references
        entry.add_bullet("Fix several bugs (Closes: #123456, #789012)");

        let lines: Vec<_> = entry.change_lines().collect();
        let text = lines.join(" ");
        assert!(text.contains("Closes: #123456"));
        assert!(text.contains("#789012"));
    }

    #[test]
    fn test_add_bullet_preserves_lp() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        // Add bullet with Launchpad bug reference
        entry.add_bullet("Fix bug (LP: #123456)");

        let lines: Vec<_> = entry.change_lines().collect();
        let text = lines.join(" ");
        // "LP: #123456" should not be split
        assert!(text.contains("LP: #123456"));
    }

    #[test]
    fn test_add_bullet_with_existing_bullets() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .change_line("* Existing change".to_string())
            .finish();

        // Add more bullets
        entry.add_bullet("New change");

        let lines: Vec<_> = entry.change_lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "* Existing change");
        assert_eq!(lines[1], "* New change");
    }

    #[test]
    fn test_add_bullet_special_characters() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        entry.add_bullet("Fix issue with \"quotes\" and 'apostrophes'");
        entry.add_bullet("Handle paths like /usr/bin/foo");
        entry.add_bullet("Support $VARIABLES and ${EXPANSIONS}");

        let lines: Vec<_> = entry.change_lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("\"quotes\""));
        assert!(lines[1].contains("/usr/bin/foo"));
        assert!(lines[2].contains("$VARIABLES"));
    }

    #[test]
    fn test_add_bullet_empty_string() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        // Empty string gets filtered out by textwrap - this is expected behavior
        entry.add_bullet("");

        let lines: Vec<_> = entry.change_lines().collect();
        // textwrap filters out empty strings, so no line is added
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn test_add_bullet_url() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        // Long URL should not be broken
        entry.add_bullet("Update documentation at https://www.example.com/very/long/path/to/documentation/page.html");

        let lines: Vec<_> = entry.change_lines().collect();
        let text = lines.join(" ");
        assert!(text.contains("https://www.example.com"));
    }

    #[test]
    fn test_add_bullet_mixed_with_manual_changes() {
        let mut changelog = ChangeLog::new();
        let entry = changelog
            .new_entry()
            .maintainer(("Test User".into(), "test@example.com".into()))
            .distribution("unstable".to_string())
            .version("1.0.0".parse().unwrap())
            .finish();

        // Mix add_bullet with manual append_change_line
        entry.add_bullet("First bullet");
        entry.append_change_line("  Manual continuation line");
        entry.add_bullet("Second bullet");

        let lines: Vec<_> = entry.change_lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "* First bullet");
        assert_eq!(lines[1], "  Manual continuation line");
        assert_eq!(lines[2], "* Second bullet");
    }

    #[test]
    fn test_replace_line_with_continuation() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * First change
    with continuation line
  * Second change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);

        // Replace the continuation line
        changes[0]
            .replace_line(1, "  with updated continuation")
            .unwrap();

        let updated_changes = iter_changes_by_author(&changelog);
        assert_eq!(
            updated_changes[0].lines(),
            vec![
                "* First change",
                "  with updated continuation",
                "* Second change"
            ]
        );
    }

    #[test]
    fn test_change_line_col() {
        let changelog: ChangeLog = r#"foo (1.0-1) unstable; urgency=low

  * First change
  * Second change

 -- Maintainer <email@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

bar (2.0-1) experimental; urgency=high

  [ Alice ]
  * Alice's change
  * Alice's second change

  [ Bob ]
  * Bob's change

 -- Another <another@example.com>  Tue, 02 Jan 2024 13:00:00 +0000
"#
        .parse()
        .unwrap();

        let changes = iter_changes_by_author(&changelog);

        // Total: 1 unattributed (first entry) + Alice + Bob = 3 changes
        assert_eq!(changes.len(), 3);

        // First change (unattributed) should be at line 2 (0-indexed)
        assert_eq!(changes[0].line(), Some(2));
        assert_eq!(changes[0].column(), Some(2)); // After "  "
        assert_eq!(changes[0].line_col(), Some((2, 2)));
        assert_eq!(changes[0].lines().len(), 2); // Two bullets in first entry

        // Alice's changes - starts at line 10 (after "  [ Alice ]" on line 9)
        assert_eq!(changes[1].line(), Some(10));
        assert_eq!(changes[1].column(), Some(2)); // After "  "
        assert_eq!(changes[1].lines().len(), 2); // Two bullets

        // Bob's changes - starts at line 14 (after blank line and "  [ Bob ]" on line 13)
        assert_eq!(changes[2].line(), Some(14));
        assert_eq!(changes[2].column(), Some(2)); // After "  "
        assert_eq!(changes[2].lines().len(), 1); // One bullet
    }
}
