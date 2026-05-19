//! Utilities for working with upstream versions.

use std::borrow::Cow;

static DFSG_REGEX: &lazy_regex::Lazy<lazy_regex::Regex> =
    lazy_regex::regex!(r"^(.*)([\+~])(dfsg|ds)([0-9]*)$");
const DFSG_DEFAULT_STYLE: &str = "+ds";

/// Strip the DFSG suffix from a version.
///
/// # Example
/// ```
/// use debversion::upstream::strip_dfsg_suffix;
/// let version = "1.2.3+dfsg1";
/// assert_eq!(strip_dfsg_suffix(version), Some("1.2.3"));
/// ```
pub fn strip_dfsg_suffix(version: &str) -> Option<&str> {
    if let Some(m) = DFSG_REGEX.captures(version) {
        Some(m.get(1).unwrap().as_str())
    } else {
        None
    }
}

/// Add a dfsg suffix to an version version string.
///
/// Allow old_upstream_version to be passed in so optionally the format can be
/// kept consistent.
///
/// # Example
/// ```
/// use debversion::upstream::add_dfsg_suffix;
/// assert_eq!(add_dfsg_suffix("1.2.3", None), "1.2.3+ds");
/// assert_eq!(add_dfsg_suffix("1.2.3", Some("1.2.2+dfsg1")), "1.2.3+dfsg1");
/// assert_eq!(add_dfsg_suffix("1.2.3", Some("1.2.2+ds1")), "1.2.3+ds1");
/// assert_eq!(add_dfsg_suffix("1.2.3", Some("1.2.3")), "1.2.3+ds");
/// ```
pub fn add_dfsg_suffix(upstream_version: &str, old_upstream_version: Option<&str>) -> String {
    let style = if let Some(m) = old_upstream_version.and_then(|d| DFSG_REGEX.captures(d)) {
        let part2 = m.get(2).unwrap().as_str();
        let part3 = m.get(3).unwrap().as_str();
        let part4 = m.get(4).unwrap().as_str();

        Cow::Owned(if part4.is_empty() {
            format!("{}{}", part2, part3)
        } else {
            format!("{}{}1", part2, part3)
        })
    } else {
        Cow::Borrowed(DFSG_DEFAULT_STYLE)
    };
    format!("{}{}", upstream_version, style)
}

#[derive(Debug, Clone, PartialEq)]
/// VCS snapshot information.
pub enum VcsSnapshot {
    /// Git snapshot information.
    Git {
        /// Date of the snapshot.
        date: Option<chrono::NaiveDate>,

        /// SHA of the snapshot, usually the first 7 characters.
        sha: Option<String>,

        /// Snapshot number.
        snapshot: Option<usize>,
    },
    /// Bazaar snapshot information.
    Bzr {
        /// Revision number, possibly dotted.
        revno: String,
    },
    /// Subversion snapshot information.
    Svn {
        /// Revision number.
        revno: usize,
    },
}

impl VcsSnapshot {
    /// Convert the VCS snapshot information to a suffix.
    fn to_suffix(&self) -> String {
        match self {
            VcsSnapshot::Git {
                date,
                sha,
                snapshot,
            } => match (sha.as_deref(), snapshot, date.as_ref()) {
                (Some(sha), Some(snapshot), Some(date)) => {
                    let gitid = &sha[..sha.len().min(7)];
                    format!("git{}.{}.{}", date.format("%Y%m%d"), snapshot, gitid)
                }
                (Some(sha), None, Some(date)) => {
                    let gitid = &sha[..sha.len().min(7)];
                    format!("git{}.{}", date.format("%Y%m%d"), gitid)
                }
                (Some(sha), _, None) => {
                    let gitid = &sha[..sha.len().min(7)];
                    format!("git{}", gitid)
                }
                (None, _, Some(date)) => {
                    format!("git{}", date.format("%Y%m%d"))
                }
                (None, _, None) => "git".to_string(),
            },
            VcsSnapshot::Bzr { revno } => format!("bzr{}", revno),
            VcsSnapshot::Svn { revno } => format!("svn{}", revno),
        }
    }
}

/// Direction to add the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Direction {
    /// Snapshot predates the version.
    Before,

    /// Snapshot postdates the version.
    After,
}

impl From<&str> for Direction {
    fn from(s: &str) -> Self {
        match s {
            "~" => Direction::Before,
            "+" => Direction::After,
            _ => panic!("Invalid direction"),
        }
    }
}

impl Direction {
    /// Convert direction to its string representation
    pub const fn as_str(&self) -> &'static str {
        match self {
            Direction::Before => "~",
            Direction::After => "+",
        }
    }
}

impl From<Direction> for &str {
    fn from(d: Direction) -> Self {
        d.as_str()
    }
}

/// Get the revision from a version string.
///
/// # Example
/// ```
/// use debversion::upstream::{get_revision, Direction, VcsSnapshot};
/// assert_eq!(get_revision("1.2.3+bzr123"), ("1.2.3", Some((Direction::After, VcsSnapshot::Bzr
/// { revno: "123".to_string() }))));
/// assert_eq!(get_revision("1.2.3+git20210101.abcdefa"), ("1.2.3",
/// Some((Direction::After, VcsSnapshot::Git { date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1,
/// 1).unwrap()), sha: Some("abcdefa".to_string()), snapshot: None }))));
/// assert_eq!(get_revision("1.2.3+git20210101.1.abcdefa"), ("1.2.3",
/// Some((Direction::After, VcsSnapshot::Git { date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1,
/// 1).unwrap()), sha: Some("abcdefa".to_string()), snapshot: Some(1) }))));
/// assert_eq!(get_revision("1.2.3+svn123"), ("1.2.3", Some((Direction::After,
/// VcsSnapshot::Svn { revno: 123 }))));
/// ```
pub fn get_revision(version_string: &str) -> (&str, Option<(Direction, VcsSnapshot)>) {
    if let Some((_, b, s, r)) =
        lazy_regex::regex_captures!(r"^(.*)([\+~])bzr(\d+)$", version_string)
    {
        (
            b,
            Some((
                s.into(),
                VcsSnapshot::Bzr {
                    revno: r.to_string(),
                },
            )),
        )
    } else if let Some((_, b, s, d, i)) =
        lazy_regex::regex_captures!(r"^(.*)([\+~-])git(\d{8})\.([a-f0-9]{7})$", version_string)
    {
        (
            b,
            Some((
                s.into(),
                VcsSnapshot::Git {
                    date: chrono::NaiveDate::parse_from_str(d, "%Y%m%d").ok(),
                    sha: Some(i.to_string()),
                    snapshot: None,
                },
            )),
        )
    } else if let Some((_, b, s, d, r, i)) = lazy_regex::regex_captures!(
        r"^(.*)([\+~-])git(\d{8})\.(\d+)\.([a-f0-9]{7})$",
        version_string
    ) {
        (
            b,
            Some((
                s.into(),
                VcsSnapshot::Git {
                    date: chrono::NaiveDate::parse_from_str(d, "%Y%m%d").ok(),
                    sha: Some(i.to_string()),
                    snapshot: r.parse().ok(),
                },
            )),
        )
    } else if let Some((_, b, s, r)) =
        lazy_regex::regex_captures!(r"^(.*)([\+~-])svn(\d+)$", version_string)
    {
        (
            b,
            Some((
                s.into(),
                VcsSnapshot::Svn {
                    revno: r.parse().unwrap(),
                },
            )),
        )
    } else {
        (version_string, None)
    }
}

/// Update the revision in a upstream version string.
///
/// # Arguments
/// * `version_string` - Original version string
/// * `sep` - Separator to use when adding snapshot
/// * `vcs_snapshot` - VCS snapshot information
///
/// # Example
/// ```
/// use debversion::upstream::{upstream_version_add_revision, VcsSnapshot};
/// assert_eq!(upstream_version_add_revision("1.2.3", VcsSnapshot::Bzr { revno:
/// "123".to_string() }, None), "1.2.3+bzr123");
/// assert_eq!(upstream_version_add_revision("1.2.3+bzr123", VcsSnapshot::Bzr { revno:
/// "124".to_string() }, None), "1.2.3+bzr124");
/// assert_eq!(upstream_version_add_revision("1.2.3", VcsSnapshot::Git { date:
/// Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()), sha: None, snapshot: None }, None),
/// "1.2.3+git20210101");
/// assert_eq!(upstream_version_add_revision("1.2.3+git20210101.abcdefa",
/// VcsSnapshot::Git { date: None, sha: Some("abcdefa".to_string()), snapshot: None }, None),
/// "1.2.3+gitabcdefa");
/// ```
pub fn upstream_version_add_revision(
    version_string: &str,
    mut vcs_snapshot: VcsSnapshot,
    sep: Option<Direction>,
) -> String {
    let plain_version = strip_dfsg_suffix(version_string).unwrap_or(version_string);

    let (base_version, current_sep, current_vcs) = match get_revision(plain_version) {
        (base_version, Some((sep, current_vcs))) => (base_version, Some(sep), Some(current_vcs)),
        (base_version, None) => (base_version, None, None),
    };

    let sep = sep.or(current_sep).unwrap_or(Direction::After);

    if let (
        VcsSnapshot::Git {
            date,
            sha,
            snapshot,
        },
        Some(VcsSnapshot::Git {
            date: c_date,
            sha: c_sha,
            snapshot: c_snapshot,
        }),
    ) = (&mut vcs_snapshot, current_vcs.as_ref())
    {
        if snapshot.is_none() {
            *snapshot = if date.as_ref() == c_date.as_ref() && sha.as_ref() != c_sha.as_ref() {
                c_snapshot.map(|s| s + 1)
            } else {
                Some(1)
            };
        }
        if c_date.is_none() {
            *date = None;
        }
        if c_sha.is_none() {
            *sha = None;
        }
    }

    let sep = sep.as_str();

    format!("{}{}{}", base_version, sep, vcs_snapshot.to_suffix())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_strip_dfsg_suffix() {
        assert_eq!(super::strip_dfsg_suffix("1.2.3+dfsg1"), Some("1.2.3"));
        assert_eq!(super::strip_dfsg_suffix("1.2.3+ds1"), Some("1.2.3"));
        assert_eq!(super::strip_dfsg_suffix("1.2.3"), None);
    }

    #[test]
    fn test_add_dfsg_suffix() {
        assert_eq!(super::add_dfsg_suffix("1.2.3", None), "1.2.3+ds");
        assert_eq!(
            super::add_dfsg_suffix("1.2.3", Some("1.2.3+dfsg1")),
            "1.2.3+dfsg1"
        );
        assert_eq!(
            super::add_dfsg_suffix("1.2.3", Some("1.2.3+ds1")),
            "1.2.3+ds1"
        );
        assert_eq!(super::add_dfsg_suffix("1.2.3", Some("1.2.3")), "1.2.3+ds");
    }

    #[test]
    fn test_to_suffix() {
        assert_eq!(
            "git",
            super::VcsSnapshot::Git {
                date: None,
                sha: None,
                snapshot: None,
            }
            .to_suffix()
        );
        assert_eq!(
            "git20210101",
            super::VcsSnapshot::Git {
                date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()),
                sha: None,
                snapshot: None,
            }
            .to_suffix()
        );
        assert_eq!(
            "gitabcdefa",
            super::VcsSnapshot::Git {
                date: None,
                sha: Some("abcdefa".to_string()),
                snapshot: None,
            }
            .to_suffix()
        );
        assert_eq!(
            "git20210101.abcdefa",
            super::VcsSnapshot::Git {
                date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()),
                sha: Some("abcdefa".to_string()),
                snapshot: None,
            }
            .to_suffix()
        );
        assert_eq!(
            "git20210101.1.abcdefa",
            super::VcsSnapshot::Git {
                date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()),
                sha: Some("abcdefa".to_string()),
                snapshot: Some(1),
            }
            .to_suffix()
        );
        assert_eq!(
            "bzr123",
            super::VcsSnapshot::Bzr {
                revno: "123".to_string(),
            }
            .to_suffix()
        );
        assert_eq!("svn123", super::VcsSnapshot::Svn { revno: 123 }.to_suffix());
    }

    #[test]
    fn test_upstream_version_add_new_suffix_bzr() {
        assert_eq!(
            "1.2.3+bzr123",
            super::upstream_version_add_revision(
                "1.2.3",
                super::VcsSnapshot::Bzr {
                    revno: "123".to_string()
                },
                None
            )
        );
    }

    #[test]
    fn test_upstream_version_add_existing_suffix_bzr() {
        assert_eq!(
            "1.2.3+bzr124",
            super::upstream_version_add_revision(
                "1.2.3+bzr123",
                super::VcsSnapshot::Bzr {
                    revno: "124".to_string()
                },
                None
            )
        );
    }

    #[test]
    fn test_upstream_version_add_new_suffix_git() {
        assert_eq!(
            "1.2.3+git20210101",
            super::upstream_version_add_revision(
                "1.2.3",
                super::VcsSnapshot::Git {
                    date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()),
                    sha: None,
                    snapshot: None,
                },
                None
            )
        );
    }

    #[test]
    fn test_upstream_version_add_existing_suffix_git() {
        assert_eq!(
            super::VcsSnapshot::Git {
                date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()),
                sha: Some("abcdefa".to_string()),
                snapshot: None,
            },
            super::get_revision("1.2.3+git20210101.abcdefa")
                .1
                .unwrap()
                .1
        );
        assert_eq!(
            "1.2.3+gitabcdefa",
            super::upstream_version_add_revision(
                "1.2.3+git20210101.1.abcdefa",
                super::VcsSnapshot::Git {
                    date: None,
                    sha: Some("abcdefa".to_string()),
                    snapshot: None,
                },
                None
            )
        );
    }

    #[test]
    fn test_upstream_version_add_new_suffix_svn() {
        assert_eq!(
            "1.2.3+svn123",
            super::upstream_version_add_revision(
                "1.2.3",
                super::VcsSnapshot::Svn { revno: 123 },
                None
            )
        );
    }

    #[test]
    fn test_upstream_version_add_existing_suffix_svn() {
        assert_eq!(
            super::VcsSnapshot::Svn { revno: 123 },
            super::get_revision("1.2.3+svn123").1.unwrap().1
        );
        assert_eq!(
            "1.2.3+svn124",
            super::upstream_version_add_revision(
                "1.2.3+svn123",
                super::VcsSnapshot::Svn { revno: 124 },
                None
            )
        );
    }

    #[test]
    fn test_upstream_version_add_revision_git_snapshot_increment() {
        // Test the missed mutants in line 273-274 around snapshot increment logic

        // Test when dates are equal but SHAs are different - should increment snapshot
        let result = super::upstream_version_add_revision(
            "1.2.3+git20210101.1.abcdefa",
            super::VcsSnapshot::Git {
                date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()),
                sha: Some("bcdefgh".to_string()),
                snapshot: None,
            },
            None,
        );
        // The SHA is truncated to 7 characters, so "bcdefgh" becomes "bcdefgh"
        assert_eq!("1.2.3+git20210101.2.bcdefgh", result);

        // Test when dates are different - should reset snapshot to 1
        let result2 = super::upstream_version_add_revision(
            "1.2.3+git20210101.5.abcdefa",
            super::VcsSnapshot::Git {
                date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 2).unwrap()),
                sha: Some("bcdefgh".to_string()),
                snapshot: None,
            },
            None,
        );
        assert_eq!("1.2.3+git20210102.1.bcdefgh", result2);

        // Test when dates are equal and SHAs are equal - should reset snapshot to 1
        let result3 = super::upstream_version_add_revision(
            "1.2.3+git20210101.1.abcdefa",
            super::VcsSnapshot::Git {
                date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()),
                sha: Some("abcdefa".to_string()),
                snapshot: None,
            },
            None,
        );
        // Since date == date and sha == sha (NOT !=), condition is false, so snapshot = Some(1)
        assert_eq!("1.2.3+git20210101.1.abcdefa", result3);

        // Test different dates should set snapshot to 1 (not increment)
        let result4 = super::upstream_version_add_revision(
            "1.2.3+git20210101.3.abcdefa",
            super::VcsSnapshot::Git {
                date: Some(chrono::NaiveDate::from_ymd_opt(2021, 1, 3).unwrap()),
                sha: Some("bcdefgh".to_string()),
                snapshot: None,
            },
            None,
        );
        assert_eq!("1.2.3+git20210103.1.bcdefgh", result4);
    }
}
