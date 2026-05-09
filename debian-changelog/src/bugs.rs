//! Bug reference detection in changelog detail lines.
//!
//! Provides types and functions for finding `Closes: #NNN` (Debian BTS) and
//! `LP: #NNN` (Launchpad) bug references in changelog text, supporting:
//!
//! - Point lookups at a byte offset (for hover / go-to-definition)
//! - Prefix lookups at a byte offset (for completion)
//! - Full span detection (for semantic highlighting)

/// Which bug tracker a reference belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BugTracker {
    /// Debian BTS (`Closes: #NNN`)
    Debian,
    /// Launchpad (`LP: #NNN`)
    Launchpad,
}

/// A resolved bug reference (tracker + number).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Bug {
    /// Debian BTS bug (`Closes: #NNN`)
    Debian(u32),
    /// Launchpad bug (`LP: #NNN`)
    Launchpad(u32),
}

impl Bug {
    /// The bug number.
    pub fn id(&self) -> u32 {
        match self {
            Bug::Debian(id) | Bug::Launchpad(id) => *id,
        }
    }

    /// Which tracker this bug belongs to.
    pub fn tracker(&self) -> BugTracker {
        match self {
            Bug::Debian(_) => BugTracker::Debian,
            Bug::Launchpad(_) => BugTracker::Launchpad,
        }
    }

    /// The URL for this bug on its tracker's web interface.
    pub fn url(&self) -> String {
        match self {
            Bug::Debian(id) => format!("https://bugs.debian.org/{}", id),
            Bug::Launchpad(id) => format!("https://bugs.launchpad.net/bugs/{}", id),
        }
    }
}

/// A bug reference span found in a detail line, with byte offsets relative
/// to the start of the detail text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BugRefSpan {
    /// Byte offset of the start of the span within the detail text.
    pub start: usize,
    /// Byte offset past the end of the span within the detail text.
    pub end: usize,
    /// Whether the reference continues on the next line (trailing comma
    /// or marker with no bug numbers yet).
    pub continues: bool,
}

/// Known bug-reference markers (case-insensitive), in scan order.
const MARKERS: &[(BugTracker, &str)] = &[
    (BugTracker::Debian, "closes:"),
    (BugTracker::Launchpad, "lp:"),
];

fn make_bug(tracker: BugTracker, id: u32) -> Bug {
    match tracker {
        BugTracker::Debian => Bug::Debian(id),
        BugTracker::Launchpad => Bug::Launchpad(id),
    }
}

/// Characters that may appear in the bug-number list after a marker.
fn is_bug_list_char(c: char) -> bool {
    c.is_ascii_whitespace() || c == ',' || c == '#' || c.is_ascii_digit()
}

/// True when `idx` sits at a word boundary in `text`.
fn is_word_boundary(text: &[u8], idx: usize) -> bool {
    idx == 0 || {
        let prev = text[idx - 1];
        !(prev.is_ascii_alphanumeric() || prev == b'-' || prev == b'_')
    }
}

/// Find all marker occurrences at word boundaries, sorted by position.
fn find_markers(text: &str) -> Vec<(BugTracker, usize, usize)> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut hits = Vec::new();
    for &(tracker, marker) in MARKERS {
        for (idx, _) in lower.match_indices(marker) {
            if is_word_boundary(bytes, idx) {
                hits.push((tracker, idx, marker.len()));
            }
        }
    }
    hits.sort_by_key(|&(_, idx, _)| idx);
    hits
}

/// Given text starting right after the marker, return
/// `(raw_len, trimmed_len)` where `raw_len` is the extent of the
/// bug-list characters and `trimmed_len` strips trailing commas/whitespace.
fn measure_bug_list(after_marker: &str) -> (usize, usize) {
    let raw_len = after_marker
        .find(|c: char| !is_bug_list_char(c))
        .unwrap_or(after_marker.len());
    let trimmed_len = after_marker[..raw_len]
        .trim_end_matches(|c: char| c == ',' || c.is_ascii_whitespace())
        .len();
    (raw_len, trimmed_len)
}

/// Convert an absolute byte offset to a relative one inside `detail_text`.
fn to_relative(detail_start: usize, detail_len: usize, offset: usize) -> Option<usize> {
    if offset < detail_start || offset > detail_start + detail_len {
        return None;
    }
    Some(std::cmp::min(offset - detail_start, detail_len))
}

/// Find the bug reference at a byte offset within a single detail line.
///
/// `detail_text` is the text of the DETAIL token, `detail_start` is its
/// absolute byte offset in the document, and `offset` is the absolute
/// cursor position.
///
/// # Example
///
/// ```
/// use debian_changelog::bugs::{bug_at_offset, Bug};
///
/// let line = "* Fix bug. (Closes: #123456)";
/// assert_eq!(bug_at_offset(line, 0, 22), Some(Bug::Debian(123456)));
/// ```
pub fn bug_at_offset(detail_text: &str, detail_start: usize, offset: usize) -> Option<Bug> {
    let rel = to_relative(detail_start, detail_text.len(), offset)?;

    // Consider only markers that start at or before the cursor.
    for (tracker, marker_idx, marker_len) in find_markers(detail_text) {
        if marker_idx > rel {
            continue;
        }
        let after = &detail_text[marker_idx + marker_len..];

        let mut pos = marker_idx + marker_len;
        for fragment in after.split(',') {
            let frag_start = pos;
            let frag_end = pos + fragment.len();
            let trimmed = fragment.trim();

            if !trimmed.is_empty() {
                let after_hash = trimmed.strip_prefix('#').unwrap_or(trimmed);
                let digits: String = after_hash
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if digits.is_empty() {
                    break;
                }
                if rel >= frag_start && rel <= frag_end {
                    return digits.parse().ok().map(|id| make_bug(tracker, id));
                }
            }

            pos = frag_end + 1; // +1 for the comma
        }
    }

    None
}

/// Find the bug-number prefix being typed at a byte offset.
///
/// Returns the tracker and the digit prefix typed so far, useful for
/// completion.  Returns `None` when the cursor is not in a bug-reference
/// context.
///
/// # Example
///
/// ```
/// use debian_changelog::bugs::{bug_prefix_at_offset, BugTracker};
///
/// let line = "* Fix bug. (Closes: #12";
/// let (tracker, prefix) = bug_prefix_at_offset(line, 0, 23).unwrap();
/// assert_eq!(tracker, BugTracker::Debian);
/// assert_eq!(prefix, "12");
/// ```
pub fn bug_prefix_at_offset(
    detail_text: &str,
    detail_start: usize,
    offset: usize,
) -> Option<(BugTracker, String)> {
    let rel = to_relative(detail_start, detail_text.len(), offset)?;

    // Find the last marker at or before the cursor.
    let (tracker, marker_idx, marker_len) = find_markers(detail_text)
        .into_iter()
        .rev()
        .find(|&(_, idx, _)| idx <= rel)?;

    let after_marker = &detail_text[marker_idx + marker_len..rel];

    // Everything between the marker and the cursor must be bug-list chars.
    if after_marker.chars().any(|c| !is_bug_list_char(c)) {
        return None;
    }

    let current_fragment = after_marker
        .rsplit(',')
        .next()
        .unwrap_or(after_marker)
        .trim_start();
    let digits = current_fragment.strip_prefix('#')?;

    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    Some((tracker, digits.to_string()))
}

/// Find all bug reference spans in a detail line.
///
/// Returns spans covering `Closes: #NNN, #NNN` and `LP: #NNN, #NNN`
/// regions.  When `continues_from_prev` is true, leading `#NNN` references
/// (continuation from a marker on the previous line) are also included.
///
/// # Example
///
/// ```
/// use debian_changelog::bugs::bug_ref_spans;
///
/// let line = "* Fix. (Closes: #111, #222)";
/// let spans = bug_ref_spans(line, false);
/// assert_eq!(spans.len(), 1);
/// assert_eq!(&line[spans[0].start..spans[0].end], "Closes: #111, #222");
/// ```
pub fn bug_ref_spans(detail_text: &str, continues_from_prev: bool) -> Vec<BugRefSpan> {
    let mut spans = Vec::new();

    if continues_from_prev {
        if let Some(span) = continuation_span(detail_text) {
            spans.push(span);
        }
    }

    for &(_, marker_idx, marker_len) in &find_markers(detail_text) {
        let after = &detail_text[marker_idx + marker_len..];
        let (raw_len, trimmed_len) = measure_bug_list(after);
        let content = &after[..raw_len];
        let has_digits = content.chars().any(|c| c.is_ascii_digit());
        let reaches_eol = marker_idx + marker_len + raw_len == detail_text.len();
        let continues = reaches_eol
            && (!has_digits || content.trim().ends_with(',') || content.trim().is_empty());

        if has_digits {
            spans.push(BugRefSpan {
                start: marker_idx,
                end: marker_idx + marker_len + trimmed_len,
                continues,
            });
        } else if continues {
            spans.push(BugRefSpan {
                start: marker_idx,
                end: marker_idx + marker_len,
                continues: true,
            });
        }
    }

    spans
}

/// Extract a continuation span from the start of a detail line (leading
/// `#NNN, #NNN` following a marker on the previous line).
fn continuation_span(detail_text: &str) -> Option<BugRefSpan> {
    let end = detail_text
        .find(|c: char| !is_bug_list_char(c))
        .unwrap_or(detail_text.len());
    let raw = &detail_text[..end];
    if !raw.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let trimmed = raw
        .trim()
        .trim_end_matches(|c: char| c == ',' || c.is_ascii_whitespace());
    if trimmed.is_empty() {
        return None;
    }
    let start = detail_text.find(trimmed)?;
    let reaches_eol = end == detail_text.len();
    let trailing_comma = raw.trim_end().ends_with(',');
    Some(BugRefSpan {
        start,
        end: start + trimmed.len(),
        continues: reaches_eol && trailing_comma,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_closes_single() {
        let line = "* Fix bug. (Closes: #123456)";
        assert_eq!(bug_at_offset(line, 0, 21), Some(Bug::Debian(123456)));
    }

    #[test]
    fn test_lp_single() {
        let line = "* Fix bug. (LP: #987654)";
        assert_eq!(bug_at_offset(line, 0, 18), Some(Bug::Launchpad(987654)));
    }

    #[test]
    fn test_closes_multiple_first() {
        let line = "* Fixed. (Closes: #111, #222)";
        assert_eq!(bug_at_offset(line, 0, 19), Some(Bug::Debian(111)));
    }

    #[test]
    fn test_closes_multiple_second() {
        let line = "* Fixed. (Closes: #111, #222)";
        assert_eq!(bug_at_offset(line, 0, 25), Some(Bug::Debian(222)));
    }

    #[test]
    fn test_no_bug_ref() {
        assert_eq!(bug_at_offset("* Just a regular change.", 0, 10), None);
    }

    #[test]
    fn test_with_nonzero_start() {
        assert_eq!(
            bug_at_offset("* Fix. (Closes: #42)", 100, 117),
            Some(Bug::Debian(42))
        );
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(
            bug_at_offset("* Fix. (closes: #42)", 0, 17),
            Some(Bug::Debian(42))
        );
    }

    #[test]
    fn test_prefix_closes() {
        assert_eq!(
            bug_prefix_at_offset("* Fix. (Closes: #12", 0, 19),
            Some((BugTracker::Debian, "12".to_string()))
        );
    }

    #[test]
    fn test_prefix_lp() {
        assert_eq!(
            bug_prefix_at_offset("* Fix. (LP: #9", 0, 14),
            Some((BugTracker::Launchpad, "9".to_string()))
        );
    }

    #[test]
    fn test_prefix_empty() {
        assert_eq!(
            bug_prefix_at_offset("* Fix. (Closes: #", 0, 17),
            Some((BugTracker::Debian, "".to_string()))
        );
    }

    #[test]
    fn test_prefix_not_in_context() {
        assert_eq!(bug_prefix_at_offset("* Just regular text", 0, 10), None);
    }

    #[test]
    fn test_bug_url() {
        assert_eq!(Bug::Debian(123).url(), "https://bugs.debian.org/123");
        assert_eq!(
            Bug::Launchpad(456).url(),
            "https://bugs.launchpad.net/bugs/456"
        );
    }

    #[test]
    fn test_bug_id() {
        assert_eq!(Bug::Debian(123).id(), 123);
        assert_eq!(Bug::Launchpad(456).id(), 456);
    }

    #[test]
    fn test_bug_tracker() {
        assert_eq!(Bug::Debian(1).tracker(), BugTracker::Debian);
        assert_eq!(Bug::Launchpad(1).tracker(), BugTracker::Launchpad);
    }

    #[test]
    fn test_spans_inline() {
        let line = "* Fix. (Closes: #111, #222)";
        let spans = bug_ref_spans(line, false);
        assert_eq!(spans.len(), 1);
        assert_eq!(&line[spans[0].start..spans[0].end], "Closes: #111, #222");
        assert!(!spans[0].continues);
    }

    #[test]
    fn test_spans_lp() {
        let line = "* Fix. (LP: #987654)";
        let spans = bug_ref_spans(line, false);
        assert_eq!(spans.len(), 1);
        assert_eq!(&line[spans[0].start..spans[0].end], "LP: #987654");
    }

    #[test]
    fn test_spans_no_bugs() {
        assert_eq!(bug_ref_spans("* Regular change.", false).len(), 0);
    }

    #[test]
    fn test_spans_eol_comma_continues() {
        let line = "* Fix. Closes: #111,";
        let spans = bug_ref_spans(line, false);
        assert_eq!(spans.len(), 1);
        assert_eq!(&line[spans[0].start..spans[0].end], "Closes: #111");
        assert!(spans[0].continues);
    }

    #[test]
    fn test_spans_marker_only_continues() {
        let spans = bug_ref_spans("* Fix. Closes:", false);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].continues);
    }

    #[test]
    fn test_spans_continuation_line() {
        let line = "    #222, #333";
        let spans = bug_ref_spans(line, true);
        assert_eq!(spans.len(), 1);
        assert_eq!(&line[spans[0].start..spans[0].end], "#222, #333");
    }

    #[test]
    fn test_spans_continuation_not_active() {
        assert_eq!(bug_ref_spans("    #222", false).len(), 0);
    }
}
