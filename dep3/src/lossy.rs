//! A library for parsing and generating Debian patch headers.
//!
//! # Examples
//!
//! ```rust
//! use dep3::lossy::PatchHeader;
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
//! assert_eq!(patch_header.description, Some("[PATCH] fix a bug".to_string()));
//! assert_eq!(patch_header.bug_debian, Some("https://bugs.debian.org/123456".parse().unwrap()));
//! ```
use crate::fields::*;
use deb822_fast::{FromDeb822, FromDeb822Paragraph, Paragraph, ToDeb822, ToDeb822Paragraph};

fn deserialize_date(s: &str) -> Result<chrono::NaiveDate, String> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| e.to_string())
}

fn serialize_date(date: &chrono::NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn deserialize_origin(s: &str) -> Result<(Option<OriginCategory>, Origin), String> {
    Ok(crate::fields::parse_origin(s))
}

fn serialize_origin((category, origin): &(Option<OriginCategory>, Origin)) -> String {
    crate::fields::format_origin(category, origin)
}

/// A patch header.
#[derive(Debug, Clone, PartialEq, FromDeb822, ToDeb822)]
pub struct PatchHeader {
    #[deb822(field = "Origin", serialize_with = serialize_origin, deserialize_with = deserialize_origin)]
    /// The origin of the patch.
    pub origin: Option<(Option<OriginCategory>, Origin)>,

    #[deb822(field = "Forwarded")]
    /// Whether the patch has been forwarded upstream.
    pub forwarded: Option<Forwarded>,

    #[deb822(field = "Author")]
    /// The author of the patch.
    pub author: Option<String>,

    #[deb822(field = "Reviewed-by")]
    /// The person who reviewed the patch.
    pub reviewed_by: Option<String>,

    #[deb822(field = "Bug-Debian")]
    /// The URL of the Debian bug report.
    pub bug_debian: Option<url::Url>,

    #[deb822(field = "Last-Update", deserialize_with = deserialize_date, serialize_with = serialize_date)]
    /// The date of the last update.
    pub last_update: Option<chrono::NaiveDate>,

    #[deb822(field = "Applied-Upstream")]
    /// Whether the patch has been applied upstream.
    pub applied_upstream: Option<AppliedUpstream>,

    #[deb822(field = "Bug")]
    /// The URL of the upstream bug report.
    pub bug: Option<url::Url>,

    #[deb822(field = "Description")]
    /// The description of the patch.
    pub description: Option<String>,
}

impl PatchHeader {
    /// Create a new patch header.
    pub fn vendor_bugs(&self, vendor: &str) -> Option<&str> {
        match vendor {
            "Debian" => self.bug_debian.as_ref().map(|u| u.as_str()),
            _ => None,
        }
    }

    /// The patch synopsis: the first line of the `Description` (or `Subject`)
    /// field. Per DEP-3 the field carries a one-line synopsis optionally followed
    /// by a longer description; this returns just the synopsis.
    pub fn synopsis(&self) -> Option<&str> {
        self.description
            .as_deref()
            .map(|d| d.split('\n').next().unwrap_or(d).trim_end())
    }

    /// The long description: everything after the synopsis line of the
    /// `Description` (or `Subject`) field, or `None` when there is only a
    /// synopsis.
    ///
    /// A lone `.` line (the deb822 blank-line marker) is decoded back to an empty
    /// line, so paragraph breaks read naturally.
    pub fn long_description(&self) -> Option<String> {
        let rest = self.description.as_deref()?.split_once('\n').map(|x| x.1)?;
        let trimmed = rest.trim_matches('\n');
        if trimmed.is_empty() {
            return None;
        }
        let decoded = trimmed
            .split('\n')
            .map(|line| if line == "." { "" } else { line })
            .collect::<Vec<_>>()
            .join("\n");
        Some(decoded)
    }
}

impl std::fmt::Display for PatchHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let paragraph: deb822_fast::Paragraph = self.to_paragraph();
        paragraph.fmt(f)
    }
}

impl std::str::FromStr for PatchHeader {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let paragraph = Paragraph::from_str(s).map_err(|e| e.to_string())?;
        let mut header = PatchHeader::from_paragraph(&paragraph)?;
        if header.author.is_none() {
            header.author = paragraph.get("From").map(ToString::to_string);
        }
        if header.description.is_none() {
            header.description = paragraph.get("Subject").map(ToString::to_string);
        }
        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::PatchHeader;
    use std::borrow::Cow;

    #[test]
    fn test_upstream() {
        let text = r#"From: Ulrich Drepper <drepper@redhat.com>
Subject: Fix regex problems with some multi-bytes characters
 
 * posix/bug-regex17.c: Add testcases.
 * posix/regcomp.c (re_compile_fastmap_iter): Rewrite COMPLEX_BRACKET
   handling.
 
Origin: upstream, http://sourceware.org/git/?p=glibc.git;a=commitdiff;h=bdb56bac
Bug: http://sourceware.org/bugzilla/show_bug.cgi?id=9697
Bug-Debian: http://bugs.debian.org/510219
"#;

        let header: PatchHeader = text.parse().unwrap();

        assert_eq!(
            header.origin,
            Some((
                Some(super::OriginCategory::Upstream),
                super::Origin::Other(Cow::Borrowed(
                    "http://sourceware.org/git/?p=glibc.git;a=commitdiff;h=bdb56bac"
                ))
            ))
        );
        assert_eq!(header.forwarded, None);
        assert_eq!(
            header.author,
            Some("Ulrich Drepper <drepper@redhat.com>".to_string())
        );
        assert_eq!(header.reviewed_by, None);
        assert_eq!(header.last_update, None);
        assert_eq!(header.applied_upstream, None);
        assert_eq!(
            header.bug,
            "http://sourceware.org/bugzilla/show_bug.cgi?id=9697"
                .parse()
                .ok()
        );
        assert_eq!(
            header.bug_debian,
            "http://bugs.debian.org/510219".parse().ok()
        );
        assert_eq!(
            header.description,
            Some("Fix regex problems with some multi-bytes characters\n\n* posix/bug-regex17.c: Add testcases.\n* posix/regcomp.c (re_compile_fastmap_iter): Rewrite COMPLEX_BRACKET\nhandling.\n".to_string())
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
        let header: PatchHeader = text.parse().unwrap();

        assert_eq!(header.origin, None);
        assert_eq!(
            header.forwarded,
            Some(super::Forwarded::Yes(Cow::Borrowed(
                "http://lists.example.com/oct-2006/1234.html"
            )))
        );
        assert_eq!(
            header.author,
            Some("John Doe <johndoe-guest@users.alioth.debian.org>".to_string())
        );
        assert_eq!(header.reviewed_by, None);
        assert_eq!(
            header.last_update,
            Some(chrono::NaiveDate::from_ymd_opt(2006, 12, 21).unwrap())
        );
        assert_eq!(header.applied_upstream, None);
        assert_eq!(
            header.description,
            Some("Use FHS compliant paths by default\nUpstream is not interested in switching to those paths.\n.\nBut we will continue using them in Debian nevertheless to comply with\nour policy.".to_string())
        );
    }

    #[test]
    fn test_synopsis_and_long_description() {
        let text = r#"Description: Use FHS compliant paths by default
 Upstream is not interested in switching to those paths.
 .
 But we will continue using them in Debian nevertheless.
Author: John Doe <johndoe@example.com>
"#;
        let header: PatchHeader = text.parse().unwrap();
        assert_eq!(
            header.synopsis(),
            Some("Use FHS compliant paths by default")
        );
        assert_eq!(
            header.long_description().as_deref(),
            Some(
                "Upstream is not interested in switching to those paths.\n\nBut we will continue using them in Debian nevertheless."
            )
        );
    }

    #[test]
    fn test_synopsis_only_has_no_long_description() {
        let text = "Description: A one-line synopsis only\nAuthor: X <x@example.com>\n";
        let header: PatchHeader = text.parse().unwrap();
        assert_eq!(header.synopsis(), Some("A one-line synopsis only"));
        assert_eq!(header.long_description(), None);
    }

    #[test]
    fn test_synopsis_from_subject() {
        // The git-style `Subject` aliases `Description`, so synopsis works too.
        let text = "Subject: Fix the thing\n Details here.\nFrom: X <x@example.com>\n";
        let header: PatchHeader = text.parse().unwrap();
        assert_eq!(header.synopsis(), Some("Fix the thing"));
        assert_eq!(header.long_description().as_deref(), Some("Details here."));
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

        let header: PatchHeader = text.parse().unwrap();

        assert_eq!(
            header.origin,
            Some((
                Some(super::OriginCategory::Vendor),
                super::Origin::Other(Cow::Borrowed(
                    "http://bugs.debian.org/cgi-bin/bugreport.cgi?msg=80;bug=265678"
                ))
            ))
        );
        assert_eq!(header.forwarded, Some(super::Forwarded::NotNeeded));
        assert_eq!(
            header.author,
            Some("Thiemo Seufer <ths@debian.org>".to_string())
        );
        assert_eq!(header.reviewed_by, None);
        assert_eq!(header.last_update, None);
        assert_eq!(header.applied_upstream, None);
        assert_eq!(
            header.bug_debian,
            "http://bugs.debian.org/265678".parse().ok()
        );

        assert_eq!(
            header.description,
            Some(
                "Workaround for broken symbol resolving on mips/mipsel
The correct fix will be done in etch and it will require toolchain
fixes."
                    .to_string()
            )
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
        let header: PatchHeader = text.parse().unwrap();

        assert_eq!(header.origin, None);
        assert_eq!(
            header.forwarded,
            Some(super::Forwarded::Yes(Cow::Borrowed(
                "http://lists.example.com/2010/03/1234.html"
            )))
        );
        assert_eq!(
            header.author,
            Some("John Doe <johndoe-guest@users.alioth.debian.org>".to_string())
        );
        assert_eq!(header.reviewed_by, None);
        assert_eq!(
            header.last_update,
            Some(chrono::NaiveDate::from_ymd_opt(2010, 3, 29).unwrap())
        );
        assert_eq!(
            header.applied_upstream,
            Some(super::AppliedUpstream::Other(Cow::Borrowed(
                "1.2, http://bzr.example.com/frobnicator/trunk/revision/123"
            )))
        );
        assert_eq!(
            header.description,
            Some("Fix widget frobnication speeds\nFrobnicating widgets too quickly tended to cause explosions.".to_string())
        );
    }
}
