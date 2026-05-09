//! Text wrapping functions
//!
//! These functions are used to wrap text for use in a changelog.
//! The main function is `textwrap`, which takes a string and wraps it to a
//! specified width, without breaking in between "Closes: #XXXXXX" fragments.

use lazy_regex::regex_captures;
use std::borrow::Cow;
use textwrap::core::Word;

/// Default width for text wrapping
pub const DEFAULT_WIDTH: usize = 78;

/// Initial indent for text wrapping
pub const INITIAL_INDENT: &str = "* ";

#[inline]
fn can_break_word(line: &str, pos: usize) -> bool {
    // Don't break if we're not at a space
    if !line[pos..].starts_with(' ') {
        return false;
    }

    // Check if breaking here would split "Closes: #" or "LP: #"
    // We need to look at the context around this position

    // Pattern: "Closes: #" - don't break between "Closes:" and "#"
    // or between ":" and " #"
    if pos >= 7 && &line[pos.saturating_sub(8)..pos] == "Closes: " && line[pos..].starts_with(" #")
    {
        // Don't break right after "Closes: " if followed by "#"
        return false;
    }

    // Also check if we're right after "Closes:" (before the space)
    if pos >= 7 && line[pos.saturating_sub(7)..pos].ends_with("Closes:") {
        return false;
    }

    // Pattern: "LP: #" - don't break between "LP:" and "#"
    if pos >= 3 && &line[pos.saturating_sub(4)..pos] == "LP: " && line[pos..].starts_with(" #") {
        return false;
    }

    if pos >= 3 && line[pos.saturating_sub(3)..pos].ends_with("LP:") {
        return false;
    }

    true
}

#[cfg(test)]
mod can_break_word_tests {
    #[test]
    fn test_can_break_word() {
        assert!(super::can_break_word("foo bar", 3));
        assert!(!super::can_break_word("foo bar", 0));
        assert!(!super::can_break_word("foo bar", 5));
    }

    #[test]
    fn test_can_break_word_edge_cases() {
        // Test position at end of string
        assert!(!super::can_break_word("foo", 3));

        // Test empty string
        assert!(!super::can_break_word("", 0));
    }

    #[test]
    fn test_closes() {
        // Test "Closes: #" at the start of line
        assert!(
            !super::can_break_word("Closes: #123456", 6),
            "Should not break after 'Closes:'"
        );
        assert!(
            !super::can_break_word("Closes: #123456", 7),
            "Should not break between 'Closes:' and '#'"
        );
        assert!(
            super::can_break_word("Closes: #123456 foo", 15),
            "Should break after bug number"
        );

        // Test "Closes: #" in the middle of line (the bug scenario)
        assert!(
            !super::can_break_word("Fix bug (Closes: #123456)", 16),
            "Should not break after 'Closes:' in middle of line"
        );
        assert!(
            !super::can_break_word("Fix bug (Closes: #123456)", 17),
            "Should not break between 'Closes:' and '#' in middle"
        );

        // Test that we can break before "(Closes:"
        assert!(
            super::can_break_word("Fix bug (Closes: #123456)", 7),
            "Should be able to break before '(Closes:'"
        );
    }

    #[test]
    fn test_lp() {
        // Test "LP: #" pattern
        assert!(
            !super::can_break_word("LP: #123456", 2),
            "Should not break after 'LP:'"
        );
        assert!(
            !super::can_break_word("LP: #123456", 3),
            "Should not break between 'LP:' and '#'"
        );
        assert!(
            super::can_break_word("LP: #123456 foo", 11),
            "Should break after bug number"
        );

        // Test "LP: #" in the middle of line
        assert!(
            !super::can_break_word("Fix bug (LP: #123456)", 12),
            "Should not break after 'LP:' in middle of line"
        );
        assert!(
            !super::can_break_word("Fix bug (LP: #123456)", 13),
            "Should not break between 'LP:' and '#' in middle"
        );
    }
}

fn find_words<'a>(line: &'a str) -> Box<dyn Iterator<Item = Word<'a>> + 'a> {
    let mut start = 0;
    let mut can_break = false;
    let mut char_indices = line.char_indices();

    Box::new(std::iter::from_fn(move || {
        for (idx, ch) in char_indices.by_ref() {
            let word_finished = can_break && ch != ' ';
            can_break = can_break_word(&line[start..], idx - start);
            if word_finished {
                let word = Word::from(&line[start..idx]);
                start = idx;
                return Some(word);
            }
        }

        if start < line.len() {
            let word = Word::from(&line[start..]);
            start = line.len();
            return Some(word);
        }

        None
    }))
}

#[cfg(test)]
mod find_words_tests {
    use super::find_words;
    use textwrap::core::Word;
    use textwrap::WordSeparator;
    #[test]
    fn test_find_words() {
        let ws = WordSeparator::Custom(find_words);
        assert_eq!(
            vec![Word::from("foo")],
            ws.find_words("foo").collect::<Vec<_>>()
        );
        assert_eq!(
            vec![Word::from("foo "), Word::from("bar")],
            ws.find_words("foo bar").collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_split_closes() {
        let ws = WordSeparator::Custom(find_words);
        assert_eq!(
            vec![
                Word::from("This "),
                Word::from("test "),
                Word::from("Closes: #123456 "),
                Word::from("foo"),
            ],
            ws.find_words("This test Closes: #123456 foo")
                .collect::<Vec<_>>()
        );

        assert_eq!(
            vec![
                Word::from("This "),
                Word::from("test "),
                Word::from("Closes: #123456"),
            ],
            ws.find_words("This test Closes: #123456")
                .collect::<Vec<_>>()
        );
    }
}

fn options<'a>(
    width: Option<usize>,
    initial_indent: Option<&'a str>,
    subsequent_indent: Option<&'a str>,
) -> textwrap::Options<'a> {
    let width = width.unwrap_or(DEFAULT_WIDTH);
    let mut options = textwrap::Options::new(width)
        .break_words(false)
        .word_splitter(textwrap::WordSplitter::NoHyphenation)
        .word_separator(textwrap::WordSeparator::Custom(find_words));
    if let Some(initial_indent) = initial_indent {
        options = options.initial_indent(initial_indent);
    }
    if let Some(subsequent_indent) = subsequent_indent {
        options = options.subsequent_indent(subsequent_indent);
    }
    options
}

/// Wrap a string of text, without breaking in between "Closes: #XXXXXX" fragments
pub fn textwrap<'a>(
    text: &'a str,
    width: Option<usize>,
    initial_indent: Option<&str>,
    subsequent_indent: Option<&str>,
) -> Vec<Cow<'a, str>> {
    let options = options(width, initial_indent, subsequent_indent);
    // Actual text wrapping using textwrap crate
    textwrap::wrap(text, options)
}

#[cfg(test)]
mod textwrap_tests {
    #[test]
    fn test_wrap_closes() {
        assert_eq!(
            vec!["And", "this", "fixes", "something.", "Closes: #123456"],
            super::textwrap(
                "And this fixes something. Closes: #123456",
                Some(5),
                None,
                None
            )
        );
    }

    #[test]
    fn test_wrap() {
        let ws = textwrap::WordSeparator::Custom(super::find_words);
        let options = textwrap::Options::new(30)
            .break_words(false)
            .word_separator(ws);
        assert_eq!(
            vec!["This", "is", "a", "line", "that", "has", "been", "broken"],
            ws.find_words("This is a line that has been broken")
                .map(|w| w.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            vec!["This is a line that has been", "broken"],
            textwrap::wrap("This is a line that has been broken", options)
        );

        assert_eq!(
            vec!["This is a line that has been", "broken"],
            super::textwrap("This is a line that has been broken", Some(30), None, None)
        );
    }
}

/// Check if two lines can join
fn can_join(line1: &str, line2: &str) -> bool {
    if line1.ends_with(':') {
        return false;
    }
    if let Some(first_char) = line2.chars().next() {
        if first_char.is_uppercase() {
            if line1.ends_with(']') || line1.ends_with('}') {
                return false;
            }
            if !line1.ends_with('.') {
                return false;
            }
        }
    }
    if line2.trim_start().starts_with('*')
        || line2.trim_start().starts_with('-')
        || line2.trim_start().starts_with('+')
    {
        return false;
    }

    // don't let lines with different indentation join
    let line1_indent = line1.len() - line1.trim_start_matches(' ').len();
    let line2_indent = line2.len() - line2.trim_start_matches(' ').len();
    if line1_indent != line2_indent {
        return false;
    }
    true
}

#[cfg(test)]
mod can_join_tests {
    #[test]
    fn test_can_join() {
        assert!(super::can_join("This is a line.", "This is a line."));
        assert!(super::can_join(
            "This is a line.",
            "This is a line. And this is another."
        ));
        assert!(!super::can_join(
            "This is a line.",
            "+ This is a submititem."
        ));
        assert!(!super::can_join(
            "This is a line introducing:",
            "  * A list item."
        ));
        assert!(!super::can_join(
            " Lines with different indentation",
            "  can not join."
        ));
    }

    #[test]
    fn test_can_join_edge_cases() {
        // Test line ending with bracket
        assert!(!super::can_join("Some text]", "Uppercase text"));
        assert!(!super::can_join("Some text}", "Uppercase text"));

        // Test line ending with period and uppercase next line
        assert!(super::can_join("End with period.", "Uppercase text"));

        // Test line not ending with period and uppercase next line
        assert!(!super::can_join("No period", "Uppercase text"));

        // Test line2 starting with bullet points
        assert!(!super::can_join("Some text", "  * bullet"));
        assert!(!super::can_join("Some text", "  - bullet"));
        assert!(!super::can_join("Some text", "  + bullet"));

        // Test line1 ending with colon
        assert!(!super::can_join("Introduction:", "some text"));

        // Test same indentation
        assert!(super::can_join("  same indent", "  can join"));

        // Test empty lines
        assert!(super::can_join("", ""));
    }
}

// Check if any lines are longer than the specified width
fn any_long_lines(lines: &[&str], width: usize) -> bool {
    lines.iter().any(|line| line.len() > width)
}

#[derive(Debug, PartialEq)]
/// Text wrapping error
pub enum Error {
    /// Missing bullet point in a line
    MissingBulletPoint {
        /// Line with missing bullet point
        line: String,
    },

    /// Unexpected indent in a line
    UnexpectedIndent {
        /// Line number
        lineno: usize,

        /// Line with unexpected indent
        line: String,

        /// Found indent
        indent: usize,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::MissingBulletPoint { line } => {
                write!(f, "Missing bullet point in line: {}", line)
            }
            Error::UnexpectedIndent {
                lineno,
                line,
                indent,
            } => write!(
                f,
                "Unexpected indent in line {}: {} (expected {} spaces)",
                lineno, line, indent
            ),
        }
    }
}

impl std::error::Error for Error {}

// Rewrap lines from a list of changes
//
// E.g.:
//
// * This is a long line that needs to be wrapped
//
// =>
//
// * This is a short line that
//   needs to be wrappd
//
fn rewrap_change<'a>(change: &[&'a str], width: Option<usize>) -> Result<Vec<Cow<'a, str>>, Error> {
    let width = width.unwrap_or(DEFAULT_WIDTH);
    assert!(width > 4);

    if change.is_empty() {
        return Ok(vec![]);
    }

    let mut initial_indent = match regex_captures!(r"^[  ]*[\+\-\*] ", change[0]) {
        Some(initial_indent) => initial_indent.to_string(),
        None => {
            return Err(Error::MissingBulletPoint {
                line: change[0].to_string(),
            })
        }
    };
    let prefix_len = initial_indent.len();

    if !any_long_lines(change, width) {
        return Ok(change.iter().map(|line| (*line).into()).collect());
    }
    let mut subsequent_indent = " ".repeat(prefix_len);

    let mut lines = vec![&change[0][prefix_len..]];

    // Strip the leading indentation from continuation lines
    // Accept any indentation >= 0, to handle varying indentation levels
    for line in change[1..].iter() {
        if line.is_empty() {
            // Empty line
            lines.push(line);
        } else if line.starts_with(' ') {
            // Line with indentation - determine how much to strip
            let line_indent = line.len() - line.trim_start_matches(' ').len();
            if line_indent >= prefix_len {
                // Strip the prefix indentation
                lines.push(&line[prefix_len..]);
            } else {
                // Less indentation than prefix - just use the line as-is
                lines.push(line);
            }
        } else {
            // No indentation - use line as-is
            lines.push(line);
        }
    }

    let mut ret: Vec<Cow<'a, str>> = Vec::new();
    let mut todo = vec![lines.remove(0)];

    for line in lines.into_iter() {
        if can_join(todo.last().unwrap(), line) {
            todo.push(line);
        } else {
            ret.extend(
                textwrap(
                    todo.join(" ").as_str(),
                    Some(width),
                    Some(initial_indent.as_str()),
                    Some(subsequent_indent.as_str()),
                )
                .iter()
                .map(|s| Cow::Owned(s.to_string())),
            );
            initial_indent =
                " ".repeat(prefix_len + line.len() - line.trim_start_matches(' ').len());
            subsequent_indent = " ".repeat(initial_indent.len());
            todo = vec![line.trim_start_matches(' ')];
        }
    }
    ret.extend(
        textwrap(
            todo.join(" ").as_str(),
            Some(width),
            Some(initial_indent.as_str()),
            Some(subsequent_indent.as_str()),
        )
        .iter()
        .map(|s| Cow::Owned(s.to_string())),
    );
    Ok(ret)
}

/// Rewrap lines from an iterator of changes
///
/// Returns a Result containing the rewrapped lines or an error if rewrapping fails.
pub fn try_rewrap_changes<'a>(
    changes: impl Iterator<Item = &'a str>,
) -> Result<Vec<Cow<'a, str>>, Error> {
    let mut change = Vec::new();
    let mut indent_len: Option<usize> = None;
    let mut ret = vec![];
    for line in changes {
        // Start of a new change
        if let Some(indent) = regex_captures!(r"^[  ]*[\+\-\*] ", line) {
            if !change.is_empty() {
                ret.extend(rewrap_change(change.as_slice(), None)?);
            }
            indent_len = Some(indent.len());
            change = vec![line];
        } else if let Some(_current_indent) = indent_len {
            // Continuation line - keep full line with indentation
            change.push(line);
        } else {
            if !change.is_empty() {
                ret.extend(rewrap_change(change.as_slice(), None)?);
            }
            ret.push(line.into());
            change = vec![];
        }
    }
    if !change.is_empty() {
        ret.extend(rewrap_change(change.as_slice(), None)?);
    }
    Ok(ret)
}

/// Rewrap lines from an iterator of changes
///
/// # Deprecated
///
/// This function panics on errors. Use [`try_rewrap_changes`] instead for proper error handling.
///
/// # Panics
///
/// Panics if rewrapping fails (e.g., due to invalid formatting).
#[deprecated(
    since = "0.2.10",
    note = "Use try_rewrap_changes for proper error handling"
)]
pub fn rewrap_changes<'a>(
    changes: impl Iterator<Item = &'a str>,
) -> impl Iterator<Item = Cow<'a, str>> {
    try_rewrap_changes(changes).unwrap().into_iter()
}

#[cfg(test)]
mod rewrap_tests {
    use super::rewrap_change;
    const LONG_LINE: &str = "This is a very long line that could have been broken and should have been broken but was not broken.";

    #[test]
    fn test_too_short() {
        assert_eq!(Vec::<&str>::new(), rewrap_change(&[][..], None).unwrap());
        assert_eq!(
            vec!["* Foo bar"],
            rewrap_change(&["* Foo bar"][..], None).unwrap()
        );
        assert_eq!(
            vec!["* Foo", "  bar"],
            rewrap_change(&["* Foo", "  bar"][..], None).unwrap()
        );
        assert_eq!(
            vec!["  * Beginning", "  next line"],
            rewrap_change(&["  * Beginning", "  next line"][..], None).unwrap()
        );
    }

    #[test]
    fn test_no_initial() {
        let long = "x".repeat(100);
        assert_eq!(
            super::Error::MissingBulletPoint { line: long.clone() },
            rewrap_change(&[&long], None).unwrap_err()
        );
    }

    #[test]
    fn test_wrap() {
        assert_eq!(
            vec![
                super::Cow::Borrowed(
                    "* This is a very long line that could have been broken and should have been"
                ),
                "  broken but was not broken.".into()
            ],
            rewrap_change(&[format!("* {}", LONG_LINE).as_str()][..], None).unwrap()
        );
        assert_eq!(r###" * Build-Depend on libsdl1.2-dev, libsdl-ttf2.0-dev and libsdl-mixer1.2-dev
   instead of with the embedded version, add -lSDL_ttf to --with-py-libs in
   debian/rules and rebootstrap (Closes: #382202)"###.split('\n').collect::<Vec<_>>(), rewrap_change(r###" * Build-Depend on libsdl1.2-dev, libsdl-ttf2.0-dev and libsdl-mixer1.2-dev instead
   of with the embedded version, add -lSDL_ttf to --with-py-libs in debian/rules
   and rebootstrap (Closes: #382202)
"###.split('\n').collect::<Vec<_>>().as_slice(), None).unwrap());
    }

    #[test]
    fn test_no_join() {
        assert_eq!(r###" - Translators know why this sign has been put here:
        _Choices: ${FOO}, !Other[ You only have to translate Other, remove the
        exclamation mark and this comment between brackets]
      Currently text, newt, slang and gtk frontends support this feature."###.split('\n').collect::<Vec<_>>(), rewrap_change(r###" - Translators know why this sign has been put here:
        _Choices: ${FOO}, !Other[ You only have to translate Other, remove the exclamation mark and this comment between brackets]
      Currently text, newt, slang and gtk frontends support this feature.
"###.split('\n').collect::<Vec<_>>().as_slice(), None).unwrap());
    }
}

#[cfg(test)]
mod rewrap_changes_tests {
    use super::try_rewrap_changes;

    /// Test that long unbreakable lines (e.g., URLs) don't cause errors
    #[test]
    fn test_long_url() {
        let changes = vec![
            "  * Fix bug",
            "    https://www.example.com/this/is/a/very/long/url/that/can/not/be/broken/because/it/is/longer/than/80/characters.",
        ];

        let result = try_rewrap_changes(changes.into_iter());
        assert!(result.is_ok(), "Should handle long URLs without error");

        let lines = result.unwrap();
        assert_eq!(
            lines,
            vec![
                "  * Fix bug",
                "    https://www.example.com/this/is/a/very/long/url/that/can/not/be/broken/because/it/is/longer/than/80/characters."
            ]
        );
    }

    /// Test that continuation lines have proper 4-space indentation after wrapping
    #[test]
    fn test_continuation_indent() {
        let changes = vec![
            "  * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.",
            "  * Provide informative error message when unarchive fails because the bug is not archived.",
        ];

        let result = try_rewrap_changes(changes.into_iter());
        assert!(result.is_ok(), "Should wrap successfully");

        let lines = result.unwrap();
        assert_eq!(
            lines,
            vec![
                "  * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to",
                "    somebody who fixed it.",
                "  * Provide informative error message when unarchive fails because the bug is",
                "    not archived."
            ]
        );
    }

    /// Test that "Closes: #" pattern stays together when wrapping
    #[test]
    fn test_closes_tag_not_broken() {
        let changes = vec![
            "  * Fix blocks/blockedby of archived bugs and more blah blah blah bl (Closes: #XXXXXXX).",
        ];

        let result = try_rewrap_changes(changes.into_iter());
        assert!(result.is_ok(), "Should wrap successfully");

        let lines = result.unwrap();
        assert_eq!(
            lines,
            vec![
                "  * Fix blocks/blockedby of archived bugs and more blah blah blah bl",
                "    (Closes: #XXXXXXX)."
            ]
        );
    }

    /// Test handling of complex nested indentation structures
    #[test]
    fn test_complex_nested_indentation() {
        let changes = vec![
            "  * Main change item",
            "    - Sub-item with 4 spaces",
            "      + Nested sub-item with 6 spaces",
            "        More text in nested item",
            "    - Another sub-item",
        ];

        let result = try_rewrap_changes(changes.into_iter());
        assert!(result.is_ok(), "Should handle nested indentation");

        let lines = result.unwrap();
        assert_eq!(
            lines,
            vec![
                "  * Main change item",
                "    - Sub-item with 4 spaces",
                "      + Nested sub-item with 6 spaces",
                "        More text in nested item",
                "    - Another sub-item",
            ]
        );
    }

    /// Test handling of empty lines between changes
    #[test]
    fn test_empty_lines() {
        let changes = vec!["  * First change", "", "  * Second change"];

        let result = try_rewrap_changes(changes.into_iter());
        assert!(result.is_ok(), "Should handle empty lines");

        let lines = result.unwrap();
        assert_eq!(lines, vec!["  * First change", "", "  * Second change"]);
    }
}
