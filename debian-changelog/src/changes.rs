//! Functions to parse the changes from a changelog entry.

use lazy_regex::regex_captures;

// A specific section in a changelog entry, e.g.:
//
// ```
// [ Joe Example]
// * Foo, bar
//  + Blah
// * Foo
// * Foo
// ```
#[derive(Default, Debug, PartialEq, Eq)]
struct Section<'a> {
    // Title of the section, if any
    title: Option<&'a str>,

    // Line numbers of the section
    linenos: Vec<usize>,

    // List of changes in the section
    changes: Vec<Vec<(usize, &'a str)>>,
}

/// Return the different sections from a set of changelog entries.
///
/// # Arguments
/// * `changes`: list of changes from a changelog entry
///
/// # Returns
///
/// An iterator over tuples with:
///    (author, list of line numbers, list of list of (lineno, line) tuples
fn changes_sections<'a>(
    changes: impl Iterator<Item = &'a str>,
) -> impl Iterator<Item = Section<'a>> {
    let mut ret: Vec<Section<'a>> = vec![];
    let mut section = Section::<'a>::default();
    let mut change = Vec::<(usize, &'a str)>::new();
    let mut saw_empty = false;

    for (i, line) in changes.enumerate() {
        if line.is_empty() && i == 0 {
            // Skip the first line
            continue;
        }

        if line.is_empty() {
            section.linenos.push(i);
            saw_empty = true;
            continue;
        }

        // Check for author section header
        if let Some(author) = extract_author_name(line) {
            if !change.is_empty() {
                section.changes.push(change);
                change = Vec::new();
            }
            if !section.changes.is_empty() {
                ret.push(section);
            }
            section = Section {
                title: Some(author),
                linenos: vec![i],
                changes: vec![],
            };
            saw_empty = false;
        } else if !line.starts_with("* ") {
            change.push((i, line));
            section.linenos.push(i);
            saw_empty = false;
        } else {
            // Starting a new bullet point
            // If we saw an empty line and we're in a titled section, start a new anonymous section
            if saw_empty && section.title.is_some() && !change.is_empty() {
                section.changes.push(change);
                change = Vec::new();
                ret.push(section);
                section = Section {
                    title: None,
                    linenos: vec![],
                    changes: vec![],
                };
            }

            if !change.is_empty() {
                section.changes.push(change);
            }
            change = vec![(i, line)];
            section.linenos.push(i);
            saw_empty = false;
        }
    }
    if !change.is_empty() {
        section.changes.push(change);
    }
    if !section.changes.is_empty() {
        ret.push(section);
    }

    ret.into_iter()
}

/// Iterate over changes by author
///
/// # Arguments
/// * `changes`: list of changes from a changelog entry
///
/// # Returns
/// An iterator over tuples with:
///   (author, list of line numbers, list of lines)
pub fn changes_by_author<'a>(
    changes: impl Iterator<Item = &'a str>,
) -> impl Iterator<Item = (Option<&'a str>, Vec<usize>, Vec<&'a str>)> {
    changes_sections(changes).map(|section| {
        let mut all_linenos = Vec::new();
        let mut all_lines = Vec::new();

        for change_entry in section.changes {
            for (lineno, line) in change_entry {
                all_linenos.push(lineno);
                all_lines.push(line);
            }
        }

        (section.title, all_linenos, all_lines)
    })
}

#[cfg(test)]
mod changes_sections_tests {
    #[test]
    fn test_simple() {
        let iter =
            super::changes_sections(vec!["", "* Change 1", "* Change 2", "  rest", ""].into_iter());
        assert_eq!(
            vec![super::Section {
                title: None,
                linenos: vec![1, 2, 3, 4],
                changes: vec![
                    (vec![(1, "* Change 1")]),
                    (vec![(2, "* Change 2"), (3, "  rest")])
                ]
            }],
            iter.collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_with_header() {
        assert_eq!(
            vec![
                super::Section {
                    title: Some("Author 1"),
                    linenos: vec![1, 2, 3],
                    changes: vec![(vec![(2, "* Change 1")])]
                },
                super::Section {
                    title: Some("Author 2"),
                    linenos: vec![4, 5, 6, 7],
                    changes: vec![(vec![(5, "* Change 2"), (6, "  rest")])]
                },
            ],
            super::changes_sections(
                vec![
                    "",
                    "[ Author 1 ]",
                    "* Change 1",
                    "",
                    "[ Author 2 ]",
                    "* Change 2",
                    "  rest",
                    "",
                ]
                .into_iter()
            )
            .collect::<Vec<_>>()
        );
    }
}

/// Strip a changelog message like debcommit does.
///
/// Takes a list of changes from a changelog entry and applies a transformation
/// so the message is well formatted for a commit message.
///
/// # Arguments:
/// * `changes` - a list of lines from the changelog entry
///
/// # Returns
/// Another list of lines with blank lines stripped from the start and the
/// spaces the start of the lines split if there is only one logical entry.
pub fn strip_for_commit_message(mut changes: Vec<&str>) -> Vec<&str> {
    if changes.is_empty() {
        return vec![];
    }
    while let Some(last) = changes.last() {
        if last.trim().is_empty() {
            changes.pop();
        } else {
            break;
        }
    }

    while let Some(first) = changes.first() {
        if first.trim().is_empty() {
            changes.remove(0);
        } else {
            break;
        }
    }

    let changes = changes
        .into_iter()
        .map(|mut line| loop {
            if line.starts_with("  ") {
                line = &line[2..];
            } else if line.starts_with('\t') {
                line = &line[1..];
            } else {
                break line;
            }
        })
        .collect::<Vec<_>>();

    // Drop bullet points
    let bullet_points_dropped = changes
        .iter()
        .map(|line| {
            let line = line.trim_start();
            if line.starts_with("* ") || line.starts_with("+ ") || line.starts_with("- ") {
                line[1..].trim_start()
            } else {
                line
            }
        })
        .collect::<Vec<_>>();
    if bullet_points_dropped.len() == 1 {
        bullet_points_dropped
    } else {
        changes
    }
}

#[cfg(test)]
mod strip_for_commit_message_tests {
    #[test]
    fn test_no_changes() {
        assert_eq!(super::strip_for_commit_message(vec![]), Vec::<&str>::new());
    }

    #[test]
    fn test_empty_changes() {
        assert_eq!(
            super::strip_for_commit_message(vec![""]),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn test_removes_leading_whitespace() {
        assert_eq!(
            super::strip_for_commit_message(vec!["foo", "bar", "\tbaz", " bang"]),
            vec!["foo", "bar", "baz", " bang"]
        );
    }

    #[test]
    fn test_removes_star_if_one() {
        assert_eq!(super::strip_for_commit_message(vec!["* foo"]), vec!["foo"]);
        assert_eq!(
            super::strip_for_commit_message(vec!["\t* foo"]),
            vec!["foo"]
        );
        assert_eq!(super::strip_for_commit_message(vec!["+ foo"]), vec!["foo"]);
        assert_eq!(super::strip_for_commit_message(vec!["- foo"]), vec!["foo"]);
        assert_eq!(super::strip_for_commit_message(vec!["*  foo"]), vec!["foo"]);
        assert_eq!(
            super::strip_for_commit_message(vec!["*  foo", "   bar"]),
            vec!["*  foo", " bar"]
        );
    }

    #[test]
    fn test_leaves_start_if_multiple() {
        assert_eq!(
            super::strip_for_commit_message(vec!["* foo", "* bar"]),
            vec!["* foo", "* bar"]
        );
        assert_eq!(
            super::strip_for_commit_message(vec!["* foo", "+ bar"]),
            vec!["* foo", "+ bar"]
        );
        assert_eq!(
            super::strip_for_commit_message(vec!["* foo", "bar", "* baz"]),
            vec!["* foo", "bar", "* baz"]
        );
    }
}

/// Format a section title.
pub fn format_section_title(title: &str) -> String {
    format!("[ {} ]", title)
}

#[cfg(test)]
mod format_section_title_tests {
    #[test]
    fn test() {
        assert_eq!(super::format_section_title("foo"), "[ foo ]");
    }
}

/// Extract the author name from an author section header.
///
/// Returns `Some(author)` if the line is an author section header,
/// or `None` if it's not.
///
/// # Example
///
/// ```
/// assert_eq!(debian_changelog::changes::extract_author_name("[ Alice ]"), Some("Alice"));
/// assert_eq!(debian_changelog::changes::extract_author_name("  [ Bob Smith ]  "), Some("Bob Smith"));
/// assert_eq!(debian_changelog::changes::extract_author_name("* Change line"), None);
/// ```
pub fn extract_author_name(line: &str) -> Option<&str> {
    regex_captures!(r"^\s*\[\s*(.*?)\s*\]\s*$", line).map(|(_, author)| author)
}

#[cfg(test)]
mod extract_author_name_tests {
    #[test]
    fn test() {
        assert_eq!(super::extract_author_name("[ Alice ]"), Some("Alice"));
        assert_eq!(super::extract_author_name("  [ Bob ]  "), Some("Bob"));
        assert_eq!(
            super::extract_author_name("[ Multi Word Name ]"),
            Some("Multi Word Name")
        );
        assert_eq!(super::extract_author_name("* Change line"), None);
        assert_eq!(super::extract_author_name("Regular text"), None);
        assert_eq!(super::extract_author_name(""), None);
    }
}

/// Add a change to the list of changes, attributed to a specific author.
///
/// This will add a new section for the author if there are no sections yet.
///
/// Returns an error if text rewrapping fails.
///
/// # Example
///
/// ```
/// let mut changes = vec![];
/// debian_changelog::changes::try_add_change_for_author(&mut changes, "Author 1", vec!["* Change 1"], None);
/// assert_eq!(changes, vec!["* Change 1"]);
/// ```
pub fn try_add_change_for_author(
    changes: &mut Vec<String>,
    author_name: &str,
    change: Vec<&str>,
    default_author: Option<(String, String)>,
) -> Result<(), crate::textwrap::Error> {
    let by_author = changes_by_author(changes.iter().map(|s| s.as_str())).collect::<Vec<_>>();

    // There are no per author sections yet, so attribute current changes to changelog entry author
    if by_author.iter().all(|(a, _, _)| a.is_none()) {
        if let Some((default_name, _default_email)) = default_author {
            if author_name != default_name.as_str() {
                if !changes.is_empty() {
                    changes.insert(0, format_section_title(default_name.as_str()));
                    if !changes.last().unwrap().is_empty() {
                        changes.push("".to_string());
                    }
                }
                changes.push(format_section_title(author_name));
            }
        }
    } else if let Some(last_section) = by_author.last().as_ref() {
        // There is a last section, so add a new section only if it is not for the same author
        if last_section.0 != Some(author_name) {
            changes.push("".to_string());
            changes.push(format_section_title(author_name));
        }
    }

    changes.extend(
        crate::textwrap::try_rewrap_changes(change.into_iter())?
            .iter()
            .map(|s| s.to_string()),
    );
    Ok(())
}

/// Add a change to the list of changes, attributed to a specific author.
///
/// This will add a new section for the author if there are no sections yet.
///
/// # Deprecated
///
/// This function panics on errors. Use [`try_add_change_for_author`] instead for proper error handling.
///
/// # Panics
///
/// Panics if text rewrapping fails.
///
/// # Example
///
/// ```
/// let mut changes = vec![];
/// debian_changelog::changes::add_change_for_author(&mut changes, "Author 1", vec!["* Change 1"], None);
/// assert_eq!(changes, vec!["* Change 1"]);
/// ```
#[deprecated(
    since = "0.2.10",
    note = "Use try_add_change_for_author for proper error handling"
)]
pub fn add_change_for_author(
    changes: &mut Vec<String>,
    author_name: &str,
    change: Vec<&str>,
    default_author: Option<(String, String)>,
) {
    try_add_change_for_author(changes, author_name, change, default_author).unwrap()
}

#[cfg(test)]
mod add_change_for_author_tests {
    use super::*;

    #[test]
    fn test_matches_default() {
        let mut changes = vec![];
        try_add_change_for_author(
            &mut changes,
            "Author 1",
            vec!["* Change 1"],
            Some(("Author 1".to_string(), "jelmer@debian.org".to_string())),
        )
        .unwrap();
        assert_eq!(changes, vec!["* Change 1"]);
    }

    #[test]
    fn test_not_matches_default() {
        let mut changes = vec![];
        try_add_change_for_author(
            &mut changes,
            "Author 1",
            vec!["* Change 1"],
            Some((
                "Default Author".to_string(),
                "jelmer@debian.org".to_string(),
            )),
        )
        .unwrap();
        assert_eq!(changes, vec!["[ Author 1 ]", "* Change 1"]);
    }
}

/// Find additional authors from a changelog entry
pub fn find_extra_authors<'a>(changes: &'a [&'a str]) -> std::collections::HashSet<&'a str> {
    changes_by_author(changes.iter().copied())
        .filter_map(|(author, _, _)| author)
        .collect::<std::collections::HashSet<_>>()
}

#[test]
fn test_find_extra_authors() {
    assert_eq!(
        find_extra_authors(&["[ Author 1 ]", "* Change 1"]),
        maplit::hashset! {"Author 1"}
    );
    assert_eq!(
        find_extra_authors(&["[ Author 1 ]", "[ Author 2 ]", "* Change 1"]),
        maplit::hashset! {"Author 2"}
    );
    assert_eq!(
        find_extra_authors(&["[ Author 1 ]", "[ Author 2 ]", "* Change 1", "* Change 2"]),
        maplit::hashset! {"Author 2"}
    );
    assert_eq!(
        find_extra_authors(&["[ Author 1 ]", "* Change 1", "[ Author 2 ]", "* Change 2"]),
        maplit::hashset! {"Author 1", "Author 2"}
    );

    assert_eq!(
        find_extra_authors(&["* Change 1", "* Change 2",]),
        maplit::hashset! {}
    );
}

/// Find authors that are thanked in a changelog entry
pub fn find_thanks<'a>(changes: &'a [&'a str]) -> std::collections::HashSet<&'a str> {
    let regex = lazy_regex::regex!(
        r"[tT]hank(?:(?:s)|(?:you))(?:\s*to)?((?:\s+(?:(?:\w\.)|(?:\w+(?:-\w+)*)))+(?:\s+<[^@>]+@[^@>]+>)?)"
    );
    changes_by_author(changes.iter().copied())
        .flat_map(|(_, _, lines)| {
            lines.into_iter().map(|line| {
                regex
                    .captures_iter(line)
                    .map(|m| m.get(1).unwrap().as_str().trim())
            })
        })
        .flatten()
        .collect::<std::collections::HashSet<_>>()
}

#[test]
fn test_find_thanks() {
    assert_eq!(find_thanks(&[]), maplit::hashset! {});
    assert_eq!(find_thanks(&["* Do foo", "* Do bar"]), maplit::hashset! {});
    assert_eq!(
        find_thanks(&["* Thanks to A. Hacker"]),
        maplit::hashset! {"A. Hacker"}
    );
    assert_eq!(
        find_thanks(&["* Thanks to James A. Hacker"]),
        maplit::hashset! {"James A. Hacker"}
    );
    assert_eq!(
        find_thanks(&["* Thankyou to B. Hacker"]),
        maplit::hashset! {"B. Hacker"}
    );
    assert_eq!(
        find_thanks(&["* thanks to A. Hacker"]),
        maplit::hashset! {"A. Hacker"}
    );
    assert_eq!(
        find_thanks(&["* thankyou to B. Hacker"]),
        maplit::hashset! {"B. Hacker"}
    );
    assert_eq!(
        find_thanks(&["* Thanks A. Hacker"]),
        maplit::hashset! {"A. Hacker"}
    );
    assert_eq!(
        find_thanks(&["* Thankyou B.  Hacker"]),
        maplit::hashset! {"B.  Hacker"}
    );
    assert_eq!(
        find_thanks(&["* Thanks to Mark A. Super-Hacker"]),
        maplit::hashset! {"Mark A. Super-Hacker"}
    );
    assert_eq!(
        find_thanks(&["* Thanks to A. Hacker <ahacker@example.com>"]),
        maplit::hashset! {"A. Hacker <ahacker@example.com>"}
    );
    assert_eq!(
        find_thanks(&["* Thanks to Adeodato Simó"]),
        maplit::hashset! {"Adeodato Simó"}
    );
}

/// Find Debian bugs that are closed in a changelog entry
///
/// Looks for patterns like `Closes: #123456` or `closes: #123456, #789012`
///
/// # Example
///
/// ```
/// let changes = vec!["* Fix bug. Closes: #123456"];
/// let bugs = debian_changelog::changes::find_closed_debian_bugs(&changes);
/// assert!(bugs.contains(&123456));
/// ```
pub fn find_closed_debian_bugs(changes: &[&str]) -> std::collections::HashSet<u32> {
    find_closed_bugs_for_marker(changes, lazy_regex::regex!(r"[Cc]loses:\s*[#\d\s,]+"))
}

/// Find Launchpad bugs that are closed in a changelog entry
///
/// Looks for patterns like `LP: #123456` or `lp: #123456, #789012`
///
/// # Example
///
/// ```
/// let changes = vec!["* Fix bug. LP: #123456"];
/// let bugs = debian_changelog::changes::find_closed_launchpad_bugs(&changes);
/// assert!(bugs.contains(&123456));
/// ```
pub fn find_closed_launchpad_bugs(changes: &[&str]) -> std::collections::HashSet<u32> {
    find_closed_bugs_for_marker(changes, lazy_regex::regex!(r"[Ll][Pp]:\s*[#\d\s,]+"))
}

fn find_closed_bugs_for_marker(
    changes: &[&str],
    marker_regex: &lazy_regex::Regex,
) -> std::collections::HashSet<u32> {
    let bug_regex = lazy_regex::regex!(r"\d+");

    changes_sections(changes.iter().copied())
        .flat_map(|section| {
            section.changes.into_iter().flat_map(|entry| {
                let joined = entry
                    .iter()
                    .map(|(_, line)| *line)
                    .collect::<Vec<_>>()
                    .join(" ");
                marker_regex
                    .find_iter(&joined)
                    .flat_map(|m| {
                        bug_regex
                            .find_iter(m.as_str())
                            .filter_map(|d| d.as_str().parse::<u32>().ok())
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect()
}

// Bug reference types and offset-based lookups have moved to crate::bugs.

// Re-export for backwards compatibility.
pub use crate::bugs::{Bug, BugRefSpan, BugTracker};

/// Check if all lines in a changelog entry are prefixed with a sha.
///
/// This is generally done by gbp-dch(1).
pub fn all_sha_prefixed(changes: &[&str]) -> bool {
    changes_sections(changes.iter().cloned())
        .flat_map(|section| {
            section
                .changes
                .into_iter()
                .flat_map(|ls| ls.into_iter().map(|(_, l)| l))
        })
        .all(|line| lazy_regex::regex_is_match!(r"^\* \[[0-9a-f]{7}\] ", line))
}

#[test]
fn test_find_closed_debian_bugs() {
    assert_eq!(find_closed_debian_bugs(&[]), maplit::hashset! {});
    assert_eq!(
        find_closed_debian_bugs(&["* Do foo", "* Do bar"]),
        maplit::hashset! {}
    );
    assert_eq!(
        find_closed_debian_bugs(&["* Fix bug. Closes: #123456"]),
        maplit::hashset! {123456}
    );
    assert_eq!(
        find_closed_debian_bugs(&["* Fix bug. closes: #123456"]),
        maplit::hashset! {123456}
    );
    assert_eq!(
        find_closed_debian_bugs(&["* Fix bugs. Closes: #123456, #789012"]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_debian_bugs(&["* Fix bugs. Closes: #123456,#789012"]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_debian_bugs(&["* Fix bugs. Closes: 123456, 789012"]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_debian_bugs(&["* Fix bugs. Closes: #123456 #789012"]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_debian_bugs(&[
            "* Fix bug 1. Closes: #123456",
            "* Fix bug 2. Closes: #789012"
        ]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_debian_bugs(&["* Fix bug. (Closes: #123456)"]),
        maplit::hashset! {123456}
    );
    // Bug number on continuation line
    assert_eq!(
        find_closed_debian_bugs(&[
            "  * no-priority-field fixer: stop adding \"Priority: optional\" when",
            "    targeting dpkg >= 1.22.13, since it is now the default. Closes:",
            "    #1128554"
        ]),
        maplit::hashset! {1128554}
    );
    // Closes: with multiple bugs split across lines
    assert_eq!(
        find_closed_debian_bugs(&["  * Fix several issues. Closes: #123456,", "    #789012"]),
        maplit::hashset! {123456, 789012}
    );
}

#[test]
fn test_find_closed_launchpad_bugs() {
    assert_eq!(find_closed_launchpad_bugs(&[]), maplit::hashset! {});
    assert_eq!(
        find_closed_launchpad_bugs(&["* Do foo", "* Do bar"]),
        maplit::hashset! {}
    );
    assert_eq!(
        find_closed_launchpad_bugs(&["* Fix bug. LP: #123456"]),
        maplit::hashset! {123456}
    );
    assert_eq!(
        find_closed_launchpad_bugs(&["* Fix bug. lp: #123456"]),
        maplit::hashset! {123456}
    );
    assert_eq!(
        find_closed_launchpad_bugs(&["* Fix bugs. LP: #123456, #789012"]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_launchpad_bugs(&["* Fix bugs. LP: #123456,#789012"]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_launchpad_bugs(&["* Fix bugs. LP: 123456, 789012"]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_launchpad_bugs(&["* Fix bugs. LP: #123456 #789012"]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_launchpad_bugs(&["* Fix bug 1. LP: #123456", "* Fix bug 2. LP: #789012"]),
        maplit::hashset! {123456, 789012}
    );
    assert_eq!(
        find_closed_launchpad_bugs(&["* Fix bug. (LP: #123456)"]),
        maplit::hashset! {123456}
    );
}

#[test]
fn test_all_sha_prefixed() {
    assert!(all_sha_prefixed(&[
        "* [a1b2c3d] foo",
        "* [a1b2c3d] bar",
        "* [a1b2c3d] baz",
    ]));
    assert!(!all_sha_prefixed(&[
        "* [a1b2c3d] foo",
        "* bar",
        "* [a1b2c3d] baz",
    ]));
}
