//! A library for parsing and generating Debian patch headers.
//!
//! # Examples
//!
//! ```rust
//! use dep3::lossless::PatchHeader;
//! use std::str::FromStr;
//! let text = r#"From: John Doe <john.doe@example>
//! Date: Mon, 1 Jan 2000 00:00:00 +0000
//! Subject: [PATCH] fix a bug
//! Bug-Debian: https://bugs.debian.org/123456
//! Bug: https://bugzilla.example.com/bug.cgi?id=123456
//! Forwarded: not-needed
//! "#;
//!
//! let patch_header = PatchHeader::from_str(text).unwrap();
//! assert_eq!(patch_header.description(), Some("[PATCH] fix a bug".to_string()));
//! assert_eq!(patch_header.vendor_bugs("Debian").collect::<Vec<_>>(), vec!["https://bugs.debian.org/123456".to_string()]);
//! ```
use deb822_lossless::Paragraph;

use crate::fields::*;

/// Encode free text for storage in a deb822 multi-line field value.
///
/// deb822 multi-line values cannot contain blank lines (lines that are
/// empty or contain only whitespace), because the parser interprets those
/// as paragraph separators.  By convention, a blank line is represented
/// as a continuation line containing only a single dot (` .`).  This
/// function applies that encoding so that the result is safe to pass to
/// [`Paragraph::insert`].
fn encode_multiline_value(text: &str) -> String {
    // Replace every \n followed by another \n (or by whitespace-only
    // content up to the next \n) with the deb822 blank-line marker.
    let mut out = String::with_capacity(text.len());
    for line in text.split('\n') {
        if !out.is_empty() {
            out.push('\n');
        }
        if line.trim().is_empty() && !out.is_empty() {
            out.push_str(" .");
        } else {
            out.push_str(line);
        }
    }
    out
}

/// A Debian patch header.
pub struct PatchHeader(Paragraph);

impl PatchHeader {
    /// Create a new, empty patch header.
    pub fn new() -> Self {
        PatchHeader(Paragraph::new())
    }

    /// Get a reference to the underlying `Paragraph`.
    pub fn as_deb822(&self) -> &Paragraph {
        &self.0
    }

    /// Get a mutable reference to the underlying `Paragraph`, mutably.
    pub fn as_deb822_mut(&mut self) -> &mut Paragraph {
        &mut self.0
    }

    /// The origin of the patch.
    pub fn origin(&self) -> Option<(Option<OriginCategory>, Origin)> {
        self.0
            .get("Origin")
            .as_deref()
            .map(crate::fields::parse_origin)
    }

    /// Set the origin of the patch.
    pub fn set_origin(&mut self, category: Option<OriginCategory>, origin: Origin) {
        self.0.insert(
            "Origin",
            crate::fields::format_origin(&category, &origin).as_str(),
        );
    }

    /// The `Forwarded` field.
    pub fn forwarded(&self) -> Option<Forwarded> {
        self.0
            .get("Forwarded")
            .as_deref()
            .map(|s| s.parse().unwrap())
    }

    /// Set the `Forwarded` field.
    pub fn set_forwarded(&mut self, forwarded: Forwarded) {
        self.0.insert("Forwarded", forwarded.to_string().as_str());
    }

    /// The author of the patch.
    pub fn author(&self) -> Option<String> {
        self.0.get("Author").or_else(|| self.0.get("From"))
    }

    /// Set the author of the patch.
    pub fn set_author(&mut self, author: &str) {
        if self.0.contains_key("From") {
            self.0.insert("From", author);
        } else {
            self.0.insert("Author", author);
        }
    }

    /// The `Reviewed-By` field.
    pub fn reviewed_by(&self) -> Vec<String> {
        self.0.get_all("Reviewed-By").collect()
    }

    /// Get the last update date of the patch.
    pub fn last_update(&self) -> Option<chrono::NaiveDate> {
        self.0
            .get("Last-Update")
            .as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    }

    /// Set the date of the last update
    pub fn set_last_update(&mut self, date: chrono::NaiveDate) {
        self.0
            .insert("Last-Update", date.format("%Y-%m-%d").to_string().as_str());
    }

    /// The `Applied-Upstream` field.
    pub fn applied_upstream(&self) -> Option<AppliedUpstream> {
        self.0
            .get("Applied-Upstream")
            .as_deref()
            .map(|s| s.parse().unwrap())
    }

    /// Set the `Applied-Upstream` field.
    pub fn set_applied_upstream(&mut self, applied_upstream: AppliedUpstream) {
        self.0
            .insert("Applied-Upstream", applied_upstream.to_string().as_str());
    }

    /// Get the bugs associated with the patch.
    pub fn bugs(&self) -> impl Iterator<Item = (Option<String>, String)> + '_ {
        self.0.items().filter_map(|(k, v)| {
            if k.starts_with("Bug-") {
                Some((Some(k.strip_prefix("Bug-").unwrap().to_string()), v))
            } else if k == "Bug" {
                Some((None, v))
            } else {
                None
            }
        })
    }

    /// Get the bugs associated with a specific vendor.
    pub fn vendor_bugs<'a>(&'a self, vendor: &'a str) -> impl Iterator<Item = String> + 'a {
        self.bugs().filter_map(|(k, v)| {
            if k == Some(vendor.to_string()) {
                Some(v)
            } else {
                None
            }
        })
    }

    /// Iterate over Debian BTS bug numbers parsed from `Bug-Debian:` (and a
    /// bare `Bug:` field when no vendor is specified).
    ///
    /// Accepts three value forms: a bare decimal number (`123456`), a
    /// `#`-prefixed number, or a `https://bugs.debian.org/NNNNNN` URL. Values
    /// that don't match are silently skipped.
    pub fn debian_bug_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.bugs().filter_map(|(vendor, value)| {
            if let Some(v) = vendor {
                if !v.eq_ignore_ascii_case("debian") {
                    return None;
                }
            }
            crate::fields::parse_debian_bug_id(&value)
        })
    }

    /// Set the upstream bug associated with the patch.
    pub fn set_upstream_bug(&mut self, bug: &str) {
        self.0.insert("Bug", bug);
    }

    /// Set the bug associated with a specific vendor.
    pub fn set_vendor_bug(&mut self, vendor: &str, bug: &str) {
        self.0.insert(format!("Bug-{}", vendor).as_str(), bug);
    }

    /// Get the description or subject field.
    fn description_field(&self) -> Option<String> {
        self.0.get("Description").or_else(|| self.0.get("Subject"))
    }

    /// Get the description of the patch.
    pub fn description(&self) -> Option<String> {
        self.description_field()
            .as_deref()
            .map(|s| s.split('\n').next().unwrap_or(s).to_string())
    }

    /// Set the description of the patch.
    pub fn set_description(&mut self, description: &str) {
        let description = encode_multiline_value(description);
        if let Some(subject) = self.0.get("Subject") {
            // Replace the first line with ours
            let new = format!(
                "{}\n{}",
                description,
                subject.split_once('\n').map(|x| x.1).unwrap_or("")
            );
            self.0.set("Subject", new.as_str());
        } else if let Some(existing) = self.0.get("Description") {
            // Replace the first line with ours
            let new = format!(
                "{}\n{}",
                description,
                existing.split_once('\n').map(|x| x.1).unwrap_or("")
            );
            self.0.set("Description", new.as_str());
        } else {
            self.0.insert("Description", description.as_str());
        }
    }

    /// Get the long description of the patch.
    pub fn long_description(&self) -> Option<String> {
        self.description_field()
            .as_deref()
            .map(|s| s.split_once('\n').map(|x| x.1).unwrap_or("").to_string())
    }

    /// Set the long description of the patch.
    pub fn set_long_description(&mut self, long_description: &str) {
        let long_description = encode_multiline_value(long_description);
        if let Some(subject) = self.0.get("Subject") {
            // Keep the first line, but replace the rest with our text
            let first_line = subject
                .split_once('\n')
                .map(|x| x.0)
                .unwrap_or(subject.as_str());
            let new = format!("{}\n{}", first_line, long_description);
            self.0.set("Subject", new.as_str());
        } else if let Some(description) = self.0.get("Description") {
            // Keep the first line, but replace the rest with our text
            let first_line = description
                .split_once('\n')
                .map(|x| x.0)
                .unwrap_or(description.as_str());
            let new = format!("{}\n{}", first_line, long_description);
            self.0.set("Description", new.as_str());
        } else {
            self.0.insert("Description", long_description.as_str());
        }
    }

    /// Write the patch header
    pub fn write<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(self.to_string().as_bytes())
    }
}

impl std::fmt::Display for PatchHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

impl Default for PatchHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl std::str::FromStr for PatchHeader {
    type Err = deb822_lossless::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PatchHeader(Paragraph::from_str(s)?))
    }
}

pub use crate::header_end;

impl PatchHeader {
    /// Parse a patch file's DEP-3 header, tolerating a trailing unified
    /// diff body. Splits `content` at the first `---` / `diff ` /
    /// `Index:` line and parses only the header portion.
    ///
    /// Returns the parsed header and the byte offset where the diff
    /// body starts (equal to `content.len()` if there is no diff body).
    /// Use this for files like `debian/patches/foo.patch` where the
    /// caller has the whole file in hand and wants just the header.
    ///
    /// `from_str` parses the input as deb822 in its entirety and will
    /// fail (or, with malformed continuations, misparse) when handed a
    /// patch with diff content; `parse_relaxed` is the appropriate
    /// entry point for that case.
    pub fn parse_relaxed(content: &str) -> Result<(Self, usize), deb822_lossless::ParseError> {
        use std::str::FromStr;
        let end = header_end(content);
        let header = PatchHeader::from_str(&content[..end])?;
        Ok((header, end))
    }
}

#[cfg(test)]
mod tests {
    use super::PatchHeader;
    use std::borrow::Cow;
    use std::str::FromStr;

    #[test]
    fn test_upstream() {
        let text = r#"From: Ulrich Drepper <drepper@redhat.com>
Subject: Fix regex problems with some multi-bytes characters
 .
 * posix/bug-regex17.c: Add testcases.
 * posix/regcomp.c (re_compile_fastmap_iter): Rewrite COMPLEX_BRACKET
   handling.
 .
Origin: upstream, http://sourceware.org/git/?p=glibc.git;a=commitdiff;h=bdb56bac
Bug: http://sourceware.org/bugzilla/show_bug.cgi?id=9697
Bug-Debian: http://bugs.debian.org/510219
"#;

        let header = PatchHeader::from_str(text).unwrap();

        assert_eq!(
            header.origin(),
            Some((
                Some(super::OriginCategory::Upstream),
                super::Origin::Other(Cow::Borrowed(
                    "http://sourceware.org/git/?p=glibc.git;a=commitdiff;h=bdb56bac"
                ))
            ))
        );
        assert_eq!(header.forwarded(), None);
        assert_eq!(
            header.author(),
            Some("Ulrich Drepper <drepper@redhat.com>".to_string())
        );
        assert_eq!(header.reviewed_by(), Vec::<&str>::new());
        assert_eq!(header.last_update(), None);
        assert_eq!(header.applied_upstream(), None);
        assert_eq!(
            header.bugs().collect::<Vec<_>>(),
            vec![
                (
                    None,
                    "http://sourceware.org/bugzilla/show_bug.cgi?id=9697".to_string()
                ),
                (
                    Some("Debian".to_string()),
                    "http://bugs.debian.org/510219".to_string()
                ),
            ]
        );
        assert_eq!(
            header.description(),
            Some("Fix regex problems with some multi-bytes characters".to_string())
        );
    }

    #[test]
    fn test_forwarded() {
        let text = r#"Description: Use FHS compliant paths by default
 Upstream is not interested in switching to those paths.
 .
 But we will continue using them in Debian nevertheless to comply with
 our policy.
Forwarded: http://lists.example.com/oct-2006/1234.html
Author: John Doe <johndoe-guest@users.alioth.debian.org>
Last-Update: 2006-12-21
"#;
        let header = PatchHeader::from_str(text).unwrap();

        assert_eq!(header.origin(), None);
        assert_eq!(
            header.forwarded(),
            Some(super::Forwarded::Yes(Cow::Borrowed(
                "http://lists.example.com/oct-2006/1234.html"
            )))
        );
        assert_eq!(
            header.author(),
            Some("John Doe <johndoe-guest@users.alioth.debian.org>".to_string())
        );
        assert_eq!(header.reviewed_by(), Vec::<&str>::new());
        assert_eq!(
            header.last_update(),
            Some(chrono::NaiveDate::from_ymd_opt(2006, 12, 21).unwrap())
        );
        assert_eq!(header.applied_upstream(), None);
        assert_eq!(header.bugs().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            header.description(),
            Some("Use FHS compliant paths by default".to_string())
        );
    }

    #[test]
    fn test_not_forwarded() {
        let text = r#"Description: Workaround for broken symbol resolving on mips/mipsel
 The correct fix will be done in etch and it will require toolchain
 fixes.
Forwarded: not-needed
Origin: vendor, http://bugs.debian.org/cgi-bin/bugreport.cgi?msg=80;bug=265678
Bug-Debian: http://bugs.debian.org/265678
Author: Thiemo Seufer <ths@debian.org>
"#;

        let header = PatchHeader::from_str(text).unwrap();

        assert_eq!(
            header.origin(),
            Some((
                Some(super::OriginCategory::Vendor),
                super::Origin::Other(Cow::Borrowed(
                    "http://bugs.debian.org/cgi-bin/bugreport.cgi?msg=80;bug=265678"
                ))
            ))
        );
        assert_eq!(header.forwarded(), Some(super::Forwarded::NotNeeded));
        assert_eq!(
            header.author(),
            Some("Thiemo Seufer <ths@debian.org>".to_string())
        );
        assert_eq!(header.reviewed_by(), Vec::<&str>::new());
        assert_eq!(header.last_update(), None);
        assert_eq!(header.applied_upstream(), None);
        assert_eq!(
            header.bugs().collect::<Vec<_>>(),
            vec![(
                Some("Debian".to_string()),
                "http://bugs.debian.org/265678".to_string()
            ),]
        );

        assert_eq!(
            header.description(),
            Some("Workaround for broken symbol resolving on mips/mipsel".to_string())
        );
    }

    #[test]
    fn test_applied_upstream() {
        let text = r#"Description: Fix widget frobnication speeds
 Frobnicating widgets too quickly tended to cause explosions.
Forwarded: http://lists.example.com/2010/03/1234.html
Author: John Doe <johndoe-guest@users.alioth.debian.org>
Applied-Upstream: 1.2, http://bzr.example.com/frobnicator/trunk/revision/123
Last-Update: 2010-03-29
"#;
        let header = PatchHeader::from_str(text).unwrap();

        assert_eq!(header.origin(), None);
        assert_eq!(
            header.forwarded(),
            Some(super::Forwarded::Yes(Cow::Borrowed(
                "http://lists.example.com/2010/03/1234.html"
            )))
        );
        assert_eq!(
            header.author(),
            Some("John Doe <johndoe-guest@users.alioth.debian.org>".to_string())
        );
        assert_eq!(header.reviewed_by(), Vec::<&str>::new());
        assert_eq!(
            header.last_update(),
            Some(chrono::NaiveDate::from_ymd_opt(2010, 3, 29).unwrap())
        );
        assert_eq!(
            header.applied_upstream(),
            Some(super::AppliedUpstream::Other(Cow::Borrowed(
                "1.2, http://bzr.example.com/frobnicator/trunk/revision/123"
            )))
        );
        assert_eq!(header.bugs().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            header.description(),
            Some("Fix widget frobnication speeds".to_string())
        );
    }

    #[test]
    fn test_vendor_bugs() {
        let text = r#"Description: Fix widget frobnication speeds
Bug: http://bugs.example.com/123
Bug-Debian: http://bugs.debian.org/123
Bug-Ubuntu: http://bugs.launchpad.net/123
"#;

        let header = PatchHeader::from_str(text).unwrap();

        assert_eq!(
            header.vendor_bugs("Debian").collect::<Vec<_>>(),
            vec!["http://bugs.debian.org/123".to_string()]
        );
        assert_eq!(
            header.vendor_bugs("Ubuntu").collect::<Vec<_>>(),
            vec!["http://bugs.launchpad.net/123".to_string()]
        );
    }

    #[test]
    fn parse_relaxed_splits_at_dashes() {
        let text = "Author: alice\nDescription: bla\n---\n@@ -1 +1 @@\n-x\n+y\n";
        let (header, end) = PatchHeader::parse_relaxed(text).unwrap();
        assert_eq!(end, "Author: alice\nDescription: bla\n".len());
        assert_eq!(header.author(), Some("alice".to_string()));
        assert_eq!(header.description(), Some("bla".to_string()));
    }

    #[test]
    fn parse_relaxed_splits_at_diff_word() {
        let text = "Author: alice\ndiff --git a/foo b/foo\n";
        let (header, end) = PatchHeader::parse_relaxed(text).unwrap();
        assert_eq!(end, "Author: alice\n".len());
        assert_eq!(header.author(), Some("alice".to_string()));
    }

    #[test]
    fn parse_relaxed_splits_at_index_marker() {
        let text = "Author: alice\nIndex: foo\n@@ -1 +1 @@\n";
        let (header, end) = PatchHeader::parse_relaxed(text).unwrap();
        assert_eq!(end, "Author: alice\n".len());
        assert_eq!(header.author(), Some("alice".to_string()));
    }

    #[test]
    fn parse_relaxed_handles_header_only() {
        let text = "Author: alice\nDescription: bla\n";
        let (header, end) = PatchHeader::parse_relaxed(text).unwrap();
        assert_eq!(end, text.len());
        assert_eq!(header.author(), Some("alice".to_string()));
    }

    #[test]
    fn header_end_handles_empty() {
        assert_eq!(super::header_end(""), 0);
    }

    #[test]
    fn test_set_last_update() {
        let text = r#"Description: Fix widget frobnication speeds
"#;
        let mut header = PatchHeader::from_str(text).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2023, 5, 15).unwrap();
        header.set_last_update(date);
        assert_eq!(header.last_update(), Some(date));
    }

    #[test]
    fn test_set_description_with_blank_lines() {
        // Descriptions containing blank lines (e.g. from fixer result messages)
        // must be encoded as " ." continuation lines so that deb822-lossless
        // does not reject them as empty continuation lines.
        let mut header = PatchHeader::new();
        header.set_description("Fix frobnication\n\nDetails follow.");
        // The stored Description value must be parseable (no panic / error).
        let rendered = header.to_string();
        let reparsed = PatchHeader::from_str(&rendered).unwrap();
        assert_eq!(reparsed.description(), Some("Fix frobnication".to_string()));
        // Blank line must have been encoded as " ."
        assert!(
            rendered.contains(" ."),
            "blank line should be encoded as ' .'"
        );
    }

    #[test]
    fn test_set_description_replaces_first_line() {
        // When a Description already exists, set_description replaces its
        // first line while preserving the rest.
        let text = "Description: old summary\n old long description\n";
        let mut header = PatchHeader::from_str(text).unwrap();
        header.set_description("new summary");
        assert_eq!(header.description(), Some("new summary".to_string()));
        assert_eq!(
            header.long_description(),
            Some("old long description".to_string())
        );
    }

    #[test]
    fn test_debian_bug_ids_from_url() {
        let text = "Bug-Debian: https://bugs.debian.org/510219\n";
        let header = PatchHeader::from_str(text).unwrap();
        assert_eq!(header.debian_bug_ids().collect::<Vec<_>>(), vec![510219]);
    }

    #[test]
    fn test_debian_bug_ids_bare_bug_field_included() {
        // A bare `Bug:` field with no vendor counts as Debian per the doc.
        let text = "Bug: 12345\n";
        let header = PatchHeader::from_str(text).unwrap();
        assert_eq!(header.debian_bug_ids().collect::<Vec<_>>(), vec![12345]);
    }

    #[test]
    fn test_debian_bug_ids_vendor_case_insensitive() {
        // Bug-DEBIAN should match too.
        let text = "Bug-DEBIAN: #42\n";
        let header = PatchHeader::from_str(text).unwrap();
        assert_eq!(header.debian_bug_ids().collect::<Vec<_>>(), vec![42]);
    }

    #[test]
    fn test_debian_bug_ids_skips_other_vendors() {
        // Non-Debian vendor bugs (and unparseable Debian values) are skipped.
        let text = "Bug: http://sourceware.org/bugzilla/show_bug.cgi?id=9697\n\
                    Bug-Debian: https://bugs.debian.org/510219\n\
                    Bug-Ubuntu: https://bugs.launchpad.net/bugs/12345\n";
        let header = PatchHeader::from_str(text).unwrap();
        assert_eq!(header.debian_bug_ids().collect::<Vec<_>>(), vec![510219]);
    }

    #[test]
    fn test_debian_bug_ids_multiple_entries() {
        let text = "Bug-Debian: https://bugs.debian.org/100\n\
                    Bug-Debian: #200\n\
                    Bug-Debian: 300\n";
        let header = PatchHeader::from_str(text).unwrap();
        assert_eq!(
            header.debian_bug_ids().collect::<Vec<_>>(),
            vec![100, 200, 300]
        );
    }

    #[test]
    fn test_debian_bug_ids_empty_when_no_bug_fields() {
        let text = "Description: nothing to see here\n";
        let header = PatchHeader::from_str(text).unwrap();
        assert_eq!(
            header.debian_bug_ids().collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
    }
}
