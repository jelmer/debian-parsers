//! Three-way merge of Debian changelog files.
//!
//! This serves the same purpose as dpkg's `dpkg-mergechangelogs`: given a
//! common ancestor (`old`) and two derived changelogs (`new_a` and `new_b`),
//! it produces a single merged changelog. Unlike dpkg, it takes advantage of
//! the structured parse rather than merging raw text.
//!
//! Each entry is identified by its version number, and entries are assumed not
//! to conflict across versions: they are merged in decreasing version order.
//! When the same version appears in both `new_a` and `new_b` with differing
//! content, the entry's parts are merged individually. The change body is
//! merged *structurally*: it is parsed into author sections (`[ Name ]`) of
//! bullets, where each bullet (a `*` line plus its continuation/sub-bullet
//! lines) is treated as an atomic unit. Bullets are merged per section with
//! the [`merge3`] crate at bullet granularity, so:
//!
//! - bullets added independently on each side are unioned (deduplicating an
//!   identical bullet added on both sides) rather than reported as a conflict,
//!   which is the common case dpkg gets wrong;
//! - a new author section added on either side is preserved;
//! - a conflict is only emitted (`<<<<<<<` / `=======` / `>>>>>>>`) when the
//!   same original bullet was edited differently on both sides.
//!
//! This module is gated behind the `merge` feature.

use crate::{ChangeLog, Entry};
use debversion::Version;

/// Options controlling how entries are matched up during a merge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeOptions {
    /// Drop the part of the version after the last tilde when comparing
    /// versions, so that e.g. `1.0-1~exp1` and `1.0-1~exp5` are treated as the
    /// same entry.
    pub merge_prereleases: bool,

    /// Treat two entries as the same when both are marked `UNRELEASED`,
    /// ignoring their version numbers.
    pub merge_unreleased: bool,
}

impl MergeOptions {
    /// Create a new set of options with all flags disabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable merging of pre-releases (see [`MergeOptions::merge_prereleases`]).
    pub fn merge_prereleases(mut self, value: bool) -> Self {
        self.merge_prereleases = value;
        self
    }

    /// Enable merging of unreleased entries (see
    /// [`MergeOptions::merge_unreleased`]).
    pub fn merge_unreleased(mut self, value: bool) -> Self {
        self.merge_unreleased = value;
        self
    }
}

/// The outcome of a three-way merge.
#[derive(Debug, Clone)]
pub struct MergeResult {
    /// The merged changelog.
    pub changelog: ChangeLog,

    /// Whether any conflicts were encountered. When true, the merged changelog
    /// contains conflict markers and needs manual resolution.
    pub conflicts: bool,
}

/// A view of an entry's constituent parts, derived from the parsed entry.
///
/// The merge operates on these parsed parts rather than raw text spans.
struct EntryParts {
    /// The header line, e.g. `pkg (1.0-1) unstable; urgency=low`.
    header: Option<String>,
    /// The change lines (the body), excluding leading/trailing blank lines.
    changes: Vec<String>,
    /// The trailer line, e.g. ` -- Maintainer <addr>  Date`.
    trailer: Option<String>,
}

impl EntryParts {
    fn from_entry(entry: &Entry) -> Self {
        if entry.is_old_style() {
            // Old-style entries are not broken down into structured tokens, so
            // we treat the whole verbatim text as the change body.
            let text = entry.to_string();
            let changes = text.trim_end_matches('\n').split('\n').map(String::from);
            return EntryParts {
                header: None,
                changes: changes.collect(),
                trailer: None,
            };
        }
        let header_raw = entry.header().map(|h| h.to_string());
        let footer_raw = entry.footer().map(|f| f.to_string());
        let header = header_raw
            .as_deref()
            .map(|h| h.trim_end_matches('\n').to_string());
        let trailer = footer_raw
            .as_deref()
            .map(|f| f.trim_end_matches('\n').to_string());

        // Extract the raw body lines (with their original indentation) by
        // stripping the verbatim header and footer text off the full entry
        // text, then trimming the surrounding blank lines that the parser
        // treats as separators.
        let full = entry.to_string();
        let mut body = full.as_str();
        if let Some(ref h) = header_raw {
            body = body.strip_prefix(h.as_str()).unwrap_or(body);
        }
        if let Some(ref f) = footer_raw {
            body = body.strip_suffix(f.as_str()).unwrap_or(body);
        }
        let mut lines: Vec<String> = body
            .strip_suffix('\n')
            .unwrap_or(body)
            .split('\n')
            .map(String::from)
            .collect();
        // The body always carries a trailing blank from the split when it ends
        // in a newline boundary; drop empties at both ends to mirror dpkg's
        // separate handling of the blank-line separators.
        while lines.first().is_some_and(|l| l.trim().is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }

        EntryParts {
            header,
            changes: lines,
            trailer,
        }
    }
}

/// Compute the comparison version for an entry, applying the prerelease option.
fn merge_version(entry: &Entry, opts: &MergeOptions) -> Option<Version> {
    let version = entry.version()?;
    if opts.merge_prereleases {
        // Drop everything after the last tilde, matching `s/~[^~]*$//` on the
        // full version string.
        let s = version.to_string();
        if let Some(idx) = s.rfind('~') {
            return s[..idx].parse().ok();
        }
    }
    Some(version)
}

/// Compare two optional entries for ordering by version.
///
/// Returns `Ordering` such that lower versions sort first (matching the
/// reversed, ascending-version iteration in dpkg-mergechangelogs). A missing
/// entry sorts after a present one.
fn compare_entries(
    a: Option<&Entry>,
    b: Option<&Entry>,
    opts: &MergeOptions,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        // A present entry sorts before a missing one (it is "smaller").
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(a), Some(b)) => {
            if opts.merge_unreleased
                && a.is_unreleased() == Some(true)
                && b.is_unreleased() == Some(true)
            {
                return Ordering::Equal;
            }
            match (merge_version(a, opts), merge_version(b, opts)) {
                (Some(av), Some(bv)) => av.cmp(&bv),
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (None, None) => Ordering::Equal,
            }
        }
    }
}

/// Perform a three-way merge of changelog files.
///
/// `old` is the common ancestor; `new_a` and `new_b` are the two derived
/// versions. Returns the merged changelog along with a flag indicating whether
/// any conflicts were encountered.
pub fn merge_changelogs(
    old: &ChangeLog,
    new_a: &ChangeLog,
    new_b: &ChangeLog,
    opts: &MergeOptions,
) -> MergeResult {
    // dpkg processes entries in ascending version order (it reverses the
    // changelogs, which are stored newest-first), prepending merged output. We
    // collect entries newest-first and walk them oldest-first.
    let o: Vec<Entry> = old.iter().collect();
    let a: Vec<Entry> = new_a.iter().collect();
    let b: Vec<Entry> = new_b.iter().collect();

    let mut merger = Merger {
        opts: *opts,
        // Indices walk from the end (oldest) towards the front (newest).
        o,
        a,
        b,
        oi: 0,
        ai: 0,
        bi: 0,
        // Output is built oldest-first, then reversed into newest-first.
        blocks: Vec::new(),
        oldest_was_entry_merge: false,
        conflicts: false,
    };
    merger.run();

    // `blocks` holds output blocks oldest-first (each was conceptually
    // prepended). Reverse to newest-first and join with a blank line between
    // blocks, matching changelog formatting.
    let oldest_is_entry_merge = merger.oldest_was_entry_merge;
    merger.blocks.reverse();
    let text = if merger.blocks.is_empty() {
        String::new()
    } else {
        let joined = merger.blocks.join("\n\n");
        // dpkg always emits a blank line after an entry whose parts were merged
        // individually. Between blocks the join supplies that blank; if such an
        // entry is the oldest (bottom-most) block there is nothing below it, so
        // add the trailing blank explicitly to match.
        if oldest_is_entry_merge {
            format!("{}\n\n", joined)
        } else {
            format!("{}\n", joined)
        }
    };

    MergeResult {
        changelog: ChangeLog::parse_relaxed(&text),
        conflicts: merger.conflicts,
    }
}

struct Merger {
    opts: MergeOptions,
    o: Vec<Entry>,
    a: Vec<Entry>,
    b: Vec<Entry>,
    oi: usize,
    ai: usize,
    bi: usize,
    /// Output blocks, in oldest-first order (each is prepended, conceptually).
    /// Blocks are joined with a blank line on output.
    blocks: Vec<String>,
    /// Whether the first-pushed (i.e. oldest, bottom-most) block came from a
    /// per-part entry merge, which dpkg always trails with a blank line.
    oldest_was_entry_merge: bool,
    conflicts: bool,
}

impl Merger {
    fn run(&mut self) {
        loop {
            let (o, a, b) = self.next_items();
            if o.is_none() && a.is_none() && b.is_none() {
                break;
            }
            if self.merge_block(o.as_ref(), a.as_ref(), b.as_ref()) {
                continue;
            }
            // Only the conflicting cases are left.
            match (&a, &b) {
                (Some(a), Some(b)) => {
                    // Same version present on both sides with differing
                    // content: merge the entry parts individually.
                    self.merge_entries(o.as_ref(), a, b);
                }
                _ => {
                    // Present on one side, changed/absent on the other.
                    let strip = |e: &Entry| e.to_string().trim_end_matches('\n').to_string();
                    self.merge_conflict(a.as_ref().map(strip), b.as_ref().map(strip));
                }
            }
        }
    }

    /// Return the next entries to merge from each side. All returned entries
    /// share the same minimal version; a side whose next entry is larger is
    /// left in place (returned as `None`).
    fn next_items(&mut self) -> (Option<Entry>, Option<Entry>, Option<Entry>) {
        // Peek the next item on each side (oldest remaining).
        let peek = |v: &[Entry], i: usize| -> Option<Entry> {
            if i < v.len() {
                Some(v[v.len() - 1 - i].clone())
            } else {
                None
            }
        };
        let items = [
            peek(&self.o, self.oi),
            peek(&self.a, self.ai),
            peek(&self.b, self.bi),
        ];

        // Find the minimal version among the three.
        let mut min: Option<&Entry> = None;
        for item in items.iter().flatten() {
            min = match min {
                None => Some(item),
                Some(cur) => {
                    if compare_entries(Some(item), Some(cur), &self.opts)
                        == std::cmp::Ordering::Less
                    {
                        Some(item)
                    } else {
                        Some(cur)
                    }
                }
            };
        }

        // Take only the items equal to the minimum; advance their cursors.
        let mut out: [Option<Entry>; 3] = [None, None, None];
        for (idx, item) in items.iter().enumerate() {
            if let Some(item) = item {
                if min.is_some()
                    && compare_entries(Some(item), min, &self.opts) == std::cmp::Ordering::Equal
                {
                    out[idx] = Some(item.clone());
                    match idx {
                        0 => self.oi += 1,
                        1 => self.ai += 1,
                        _ => self.bi += 1,
                    }
                }
            }
        }
        let [o, a, b] = out;
        (o, a, b)
    }

    /// Try to merge the obvious cases. Returns true on success.
    ///
    /// ```text
    /// O A B => ?
    /// - x x => x
    /// o o b => b
    /// - - b => b
    /// o a o => a
    /// - a - => a
    /// ```
    fn merge_block(&mut self, o: Option<&Entry>, a: Option<&Entry>, b: Option<&Entry>) -> bool {
        // Entry text carries a trailing newline; strip it so blocks are joined
        // with a single blank-line separator on output.
        let strip = |e: &Entry| e.to_string().trim_end_matches('\n').to_string();
        let o = o.map(strip);
        let a = a.map(strip);
        let b = b.map(strip);
        match Self::resolve_block(o.as_deref(), a.as_deref(), b.as_deref()) {
            BlockResolution::Resolved(Some(text)) => {
                self.push_block(text);
                true
            }
            BlockResolution::Resolved(None) => true,
            BlockResolution::Conflict => false,
        }
    }

    /// Resolve one of the obvious merge cases without emitting anything.
    ///
    /// ```text
    /// O A B => ?
    /// - x x => x
    /// o o b => b
    /// - - b => b
    /// o a o => a
    /// - a - => a
    /// ```
    ///
    /// Returns the chosen text (`None` meaning "nothing", e.g. both sides
    /// dropped the part) or `Conflict` when the case is not obvious.
    fn resolve_block(o: Option<&str>, a: Option<&str>, b: Option<&str>) -> BlockResolution {
        if a.is_none() && b.is_none() {
            return BlockResolution::Resolved(None);
        }
        if let (Some(av), Some(bv)) = (a, b) {
            if av == bv {
                return BlockResolution::Resolved(Some(av.to_string()));
            }
        }
        if a == o {
            return BlockResolution::Resolved(b.map(str::to_string));
        }
        if b == o {
            return BlockResolution::Resolved(a.map(str::to_string));
        }
        BlockResolution::Conflict
    }

    /// Merge changelog entries by merging their parts individually, for a
    /// nicer result than a single conflict block. Produces one output block.
    fn merge_entries(&mut self, o: Option<&Entry>, a: &Entry, b: &Entry) {
        let op = o.map(EntryParts::from_entry);
        let ap = EntryParts::from_entry(a);
        let bp = EntryParts::from_entry(b);

        let mut lines: Vec<String> = Vec::new();

        // Header.
        match Self::resolve_block(
            op.as_ref().and_then(|p| p.header.as_deref()),
            ap.header.as_deref(),
            bp.header.as_deref(),
        ) {
            BlockResolution::Resolved(text) => lines.extend(text),
            BlockResolution::Conflict => {
                lines.extend(self.conflict_lines(ap.header.clone(), bp.header.clone()))
            }
        }

        // Blank line between header and changes.
        lines.push(String::new());

        // Changes: a structure-aware three-way merge that understands author
        // sections and treats each bullet as an atomic unit.
        lines.extend(self.merge_changes(
            op.as_ref().map(|p| p.changes.as_slice()).unwrap_or(&[]),
            &ap.changes,
            &bp.changes,
        ));

        // Blank line between changes and trailer.
        lines.push(String::new());

        // Trailer.
        match Self::resolve_block(
            op.as_ref().and_then(|p| p.trailer.as_deref()),
            ap.trailer.as_deref(),
            bp.trailer.as_deref(),
        ) {
            BlockResolution::Resolved(text) => lines.extend(text),
            BlockResolution::Conflict => {
                lines.extend(self.conflict_lines(ap.trailer.clone(), bp.trailer.clone()))
            }
        }

        let is_first = self.blocks.is_empty();
        self.push_block(lines.join("\n"));
        if is_first {
            self.oldest_was_entry_merge = true;
        }
    }

    /// Structure-aware three-way merge of the change bodies.
    ///
    /// The body is parsed into author sections, each a list of bullets (a
    /// `* ...` line plus its continuation/sub-bullet lines). Sections are
    /// matched across the three inputs by author title, and the bullets within
    /// each are merged with [`merge3`] at bullet granularity, so each bullet is
    /// an atomic unit rather than a sequence of lines.
    ///
    /// This is strictly better than a line-based merge for the common case
    /// where each side adds a different new bullet: they are unioned rather
    /// than reported as a conflict. A conflict is only emitted when the same
    /// original bullet was edited differently on both sides.
    fn merge_changes(&mut self, o: &[String], a: &[String], b: &[String]) -> Vec<String> {
        let os = parse_change_sections(o);
        let as_ = parse_change_sections(a);
        let bs = parse_change_sections(b);

        // Determine the order in which to emit sections: by title, in order of
        // first appearance across a, then b, then o. An untitled (leading)
        // section, if any, always comes first.
        let mut order: Vec<Option<String>> = Vec::new();
        let mut seen: std::collections::HashSet<Option<String>> = std::collections::HashSet::new();
        for sections in [&as_, &bs, &os] {
            for s in sections {
                if seen.insert(s.title.clone()) {
                    order.push(s.title.clone());
                }
            }
        }
        // Keep an untitled section first if present.
        order.sort_by_key(|t| t.is_some());

        let find = |sections: &[ChangeSection], title: &Option<String>| -> Vec<Bullet> {
            sections
                .iter()
                .find(|s| &s.title == title)
                .map(|s| s.bullets.clone())
                .unwrap_or_default()
        };

        // Keys of bullets present in the base. A base bullet that both sides
        // re-homed into different author sections would otherwise be emitted
        // once per section; dedup it so an unchanged base bullet appears once.
        let base_keys: std::collections::HashSet<String> = os
            .iter()
            .flat_map(|s| &s.bullets)
            .map(|lines| lines.join("\n"))
            .collect();
        let mut emitted_base: std::collections::HashSet<String> = std::collections::HashSet::new();

        let mut out: Vec<String> = Vec::new();
        let mut emitted_section = false;
        for title in &order {
            let merged =
                self.merge_bullets(&find(&os, title), &find(&as_, title), &find(&bs, title));
            // Drop base bullets already emitted in an earlier section.
            let merged: Vec<Bullet> = merged
                .into_iter()
                .filter(|bullet| {
                    let k = bullet.join("\n");
                    if base_keys.contains(&k) {
                        emitted_base.insert(k)
                    } else {
                        true
                    }
                })
                .collect();
            if merged.is_empty() {
                continue;
            }
            // Separate sections with a blank line, and emit the author header.
            if emitted_section {
                out.push(String::new());
            }
            if let Some(title) = title {
                out.push(crate::changes::format_section_title(title));
            }
            for bullet in merged {
                out.extend(bullet);
            }
            emitted_section = true;
        }
        out
    }

    /// Three-way merge of a single section's bullets, each treated as an atomic
    /// unit. Returns the merged list of bullets.
    fn merge_bullets(&mut self, o: &[Bullet], a: &[Bullet], b: &[Bullet]) -> Vec<Bullet> {
        // Key each bullet by its joined text so merge3 compares whole bullets.
        let key = |bullets: &[Bullet]| -> Vec<String> {
            bullets.iter().map(|lines| lines.join("\n")).collect()
        };
        let ok = key(o);
        let ak = key(a);
        let bk = key(b);
        let base: Vec<&str> = ok.iter().map(String::as_str).collect();
        let ours: Vec<&str> = ak.iter().map(String::as_str).collect();
        let theirs: Vec<&str> = bk.iter().map(String::as_str).collect();

        // Look a bullet's lines back up from its key, per side.
        let lookup = |k: &str, side_keys: &[String], side: &[Bullet]| -> Bullet {
            side.iter()
                .zip(side_keys)
                .find(|(_, sk)| sk.as_str() == k)
                .map(|(lines, _)| lines.clone())
                .unwrap_or_else(|| k.split('\n').map(String::from).collect())
        };

        let m = merge3::Merge3::new(&base, &ours, &theirs);
        let mut out: Vec<Bullet> = Vec::new();
        for group in m.merge_groups() {
            match group {
                merge3::MergeGroup::Unchanged(keys) | merge3::MergeGroup::Same(keys) => {
                    for k in keys {
                        out.push(lookup(k, &ok, o));
                    }
                }
                merge3::MergeGroup::A(keys) => {
                    for k in keys {
                        out.push(lookup(k, &ak, a));
                    }
                }
                merge3::MergeGroup::B(keys) => {
                    for k in keys {
                        out.push(lookup(k, &bk, b));
                    }
                }
                merge3::MergeGroup::Conflict(base_keys, a_keys, b_keys) => {
                    if base_keys.map(|s| s.is_empty()).unwrap_or(true) {
                        // Purely additive: both sides inserted new bullets at
                        // the same position. Union them, dropping any bullet
                        // that both sides added identically.
                        let mut taken: std::collections::HashSet<&str> =
                            std::collections::HashSet::new();
                        for k in a_keys {
                            taken.insert(k);
                            out.push(lookup(k, &ak, a));
                        }
                        for k in b_keys {
                            if taken.insert(k) {
                                out.push(lookup(k, &bk, b));
                            }
                        }
                    } else {
                        // An existing bullet was edited differently on each
                        // side: a genuine conflict, marked around the bullets.
                        self.conflicts = true;
                        let mut bullet = vec!["<<<<<<<".to_string()];
                        for k in a_keys {
                            bullet.extend(lookup(k, &ak, a));
                        }
                        bullet.push("=======".to_string());
                        for k in b_keys {
                            bullet.extend(lookup(k, &bk, b));
                        }
                        bullet.push(">>>>>>>".to_string());
                        out.push(bullet);
                    }
                }
            }
        }
        out
    }

    /// A conflict between two single parts, as a whole output block.
    fn merge_conflict(&mut self, a: Option<String>, b: Option<String>) {
        let lines = self.conflict_lines(a, b);
        self.push_block(lines.join("\n"));
    }

    /// Build the lines of a conflict block and record that a conflict occurred.
    fn conflict_lines(&mut self, a: Option<String>, b: Option<String>) -> Vec<String> {
        self.conflicts = true;
        let mut lines = vec!["<<<<<<<".to_string()];
        lines.extend(a);
        lines.push("=======".to_string());
        lines.extend(b);
        lines.push(">>>>>>>".to_string());
        lines
    }

    fn push_block(&mut self, block: String) {
        self.blocks.push(block);
    }
}

/// The outcome of attempting to resolve a block via the obvious merge cases.
enum BlockResolution {
    /// Resolved cleanly to the given text (`None` means "emit nothing").
    Resolved(Option<String>),
    /// Could not be resolved without a conflict.
    Conflict,
}

/// One logical change: a `* ...` bullet plus any continuation or sub-bullet
/// lines that belong to it. Stored as its raw lines, with indentation.
type Bullet = Vec<String>;

/// A section of an entry's change body, optionally introduced by an author
/// header like `[ Alice ]`.
#[derive(Debug, Default, PartialEq, Eq)]
struct ChangeSection {
    /// The author title, or `None` for an untitled (leading) section.
    title: Option<String>,
    /// The bullets in this section, in order.
    bullets: Vec<Bullet>,
}

/// Parse change-body lines into author sections of bullets.
///
/// A line of the form `[ Name ]` starts a new author section. Within a
/// section, a `* ` line starts a new bullet; other non-empty lines (e.g.
/// `  + sub-bullet` or wrapped continuations) attach to the current bullet.
/// Blank lines act as separators and are not retained.
fn parse_change_sections(lines: &[String]) -> Vec<ChangeSection> {
    let mut sections: Vec<ChangeSection> = Vec::new();
    let mut current = ChangeSection::default();
    let mut bullet: Bullet = Vec::new();

    let flush_bullet = |bullet: &mut Bullet, section: &mut ChangeSection| {
        if !bullet.is_empty() {
            section.bullets.push(std::mem::take(bullet));
        }
    };

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(author) = crate::changes::extract_author_name(line) {
            flush_bullet(&mut bullet, &mut current);
            if !current.bullets.is_empty() || current.title.is_some() {
                sections.push(std::mem::take(&mut current));
            }
            current.title = Some(author.to_string());
        } else if line.trim_start().starts_with("* ") {
            flush_bullet(&mut bullet, &mut current);
            bullet.push(line.clone());
        } else {
            // Continuation or sub-bullet of the current bullet. If there is no
            // open bullet (e.g. stray leading text), start one anyway so the
            // line is not lost.
            bullet.push(line.clone());
        }
    }
    flush_bullet(&mut bullet, &mut current);
    if !current.bullets.is_empty() || current.title.is_some() {
        sections.push(current);
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge(old: &str, a: &str, b: &str, opts: MergeOptions) -> (String, bool) {
        let result = merge_changelogs(
            &ChangeLog::parse_relaxed(old),
            &ChangeLog::parse_relaxed(a),
            &ChangeLog::parse_relaxed(b),
            &opts,
        );
        (result.changelog.to_string(), result.conflicts)
    }

    const OLD: &str = "\
foo (1.0-1) unstable; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";

    #[test]
    fn test_identical() {
        let (out, conflicts) = merge(OLD, OLD, OLD, MergeOptions::new());
        assert_eq!(out, OLD);
        assert!(!conflicts);
    }

    #[test]
    fn test_distinct_new_entries_interleave_by_version() {
        let a = "\
foo (1.1-1) unstable; urgency=low

  * Feature A.

 -- Alice <alice@debian.org>  Tue, 02 Jan 2024 00:00:00 +0000

foo (1.0-1) unstable; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let b = "\
foo (1.0.1-1) unstable; urgency=low

  * Feature B.

 -- Bob <bob@debian.org>  Wed, 03 Jan 2024 00:00:00 +0000

foo (1.0-1) unstable; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let (out, conflicts) = merge(OLD, a, b, MergeOptions::new());
        assert_eq!(
            out,
            "\
foo (1.1-1) unstable; urgency=low

  * Feature A.

 -- Alice <alice@debian.org>  Tue, 02 Jan 2024 00:00:00 +0000

foo (1.0.1-1) unstable; urgency=low

  * Feature B.

 -- Bob <bob@debian.org>  Wed, 03 Jan 2024 00:00:00 +0000

foo (1.0-1) unstable; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
"
        );
        assert!(!conflicts);
    }

    #[test]
    fn test_clean_three_way_change_merge() {
        let old = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.
  * Second point.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let a = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Added at top by A.
  * Initial release.
  * Second point.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let b = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.
  * Second point.
  * Added at bottom by B.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let (out, conflicts) = merge(old, a, b, MergeOptions::new());
        assert_eq!(
            out,
            "\
foo (1.0-1) UNRELEASED; urgency=low

  * Added at top by A.
  * Initial release.
  * Second point.
  * Added at bottom by B.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000

"
        );
        assert!(!conflicts);
    }

    #[test]
    fn test_added_bullets_on_both_sides_are_unioned() {
        // Unlike dpkg-mergechangelogs, two new bullets added to the same entry
        // are unioned rather than reported as a conflict.
        let old = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let a = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.
  * Added thing A.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let b = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.
  * Added thing B.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let (out, conflicts) = merge(old, a, b, MergeOptions::new());
        assert_eq!(
            out,
            "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.
  * Added thing A.
  * Added thing B.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000

"
        );
        assert!(!conflicts);
    }

    #[test]
    fn test_same_bullet_edited_differently_conflicts() {
        // When the same original bullet is edited differently on both sides,
        // that single bullet is a conflict.
        let old = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Fix the bug.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let a = old.replace("Fix the bug.", "Fix the bug properly.");
        let b = old.replace("Fix the bug.", "Fix the bug quickly.");
        let (out, conflicts) = merge(old, &a, &b, MergeOptions::new());
        assert_eq!(
            out,
            "\
foo (1.0-1) UNRELEASED; urgency=low

<<<<<<<
  * Fix the bug properly.
=======
  * Fix the bug quickly.
>>>>>>>

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000

"
        );
        assert!(conflicts);
    }

    #[test]
    fn test_header_conflict() {
        let old = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let a = old.replace("urgency=low", "urgency=medium");
        let b = old.replace("urgency=low", "urgency=high");
        let (out, conflicts) = merge(old, &a, &b, MergeOptions::new());
        assert_eq!(
            out,
            "\
<<<<<<<
foo (1.0-1) UNRELEASED; urgency=medium
=======
foo (1.0-1) UNRELEASED; urgency=high
>>>>>>>

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000

"
        );
        assert!(conflicts);
    }

    #[test]
    fn test_merge_prereleases() {
        let old = "\
foo (1.0-1~exp1) experimental; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        // A bumps the prerelease and adds a change; with -m the two are the
        // same entry, so the change is taken over.
        let a = "\
foo (1.0-1~exp2) experimental; urgency=low

  * Initial release.
  * More work.

 -- Jelmer <jelmer@debian.org>  Tue, 02 Jan 2024 00:00:00 +0000
";
        let b = old;
        let opts = MergeOptions::new().merge_prereleases(true);
        let (out, conflicts) = merge(old, a, b, opts);
        // exp1 and exp2 normalize to the same version; b matches old, so a is
        // taken verbatim via the obvious-case merge (no per-part merge, hence
        // no trailing blank line).
        assert_eq!(
            out,
            "\
foo (1.0-1~exp2) experimental; urgency=low

  * Initial release.
  * More work.

 -- Jelmer <jelmer@debian.org>  Tue, 02 Jan 2024 00:00:00 +0000
"
        );
        assert!(!conflicts);
    }

    #[test]
    fn test_merge_unreleased_conflict() {
        let old = "\
foo (2.1-1) unstable; urgency=low

  * Released.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let a = "\
foo (2.2-1) UNRELEASED; urgency=low

  * Work for 2.2.

 -- Jelmer <jelmer@debian.org>  Tue, 02 Jan 2024 00:00:00 +0000

foo (2.1-1) unstable; urgency=low

  * Released.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let b = "\
foo (2.3-1) UNRELEASED; urgency=low

  * Work for 2.3.

 -- Jelmer <jelmer@debian.org>  Wed, 03 Jan 2024 00:00:00 +0000

foo (2.1-1) unstable; urgency=low

  * Released.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let opts = MergeOptions::new().merge_unreleased(true);
        let (out, conflicts) = merge(old, a, b, opts);
        // The two UNRELEASED entries are matched. Their header and trailer
        // differ and conflict, but their bullets are independent additions and
        // so are unioned rather than conflicting. The released entry below is
        // shared.
        assert_eq!(
            out,
            "\
<<<<<<<
foo (2.2-1) UNRELEASED; urgency=low
=======
foo (2.3-1) UNRELEASED; urgency=low
>>>>>>>

  * Work for 2.2.
  * Work for 2.3.

<<<<<<<
 -- Jelmer <jelmer@debian.org>  Tue, 02 Jan 2024 00:00:00 +0000
=======
 -- Jelmer <jelmer@debian.org>  Wed, 03 Jan 2024 00:00:00 +0000
>>>>>>>

foo (2.1-1) unstable; urgency=low

  * Released.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
"
        );
        assert!(conflicts);
    }

    #[test]
    fn test_entry_added_on_one_side() {
        let a = "\
foo (1.1-1) unstable; urgency=low

  * New.

 -- A <a@d.org>  Tue, 02 Jan 2024 00:00:00 +0000

foo (1.0-1) unstable; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let (out, conflicts) = merge(OLD, a, OLD, MergeOptions::new());
        assert_eq!(out, a);
        assert!(!conflicts);
    }

    #[test]
    fn test_nonoverlapping_change_edits_merge_cleanly() {
        // A edits the first bullet, B edits the third; the shared middle line
        // anchors a clean three-way merge.
        let old = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Line one.
  * Shared line.
  * Line three.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let a = old.replace("Line one.", "Line one A.");
        let b = old.replace("Line three.", "Line three B.");
        let (out, conflicts) = merge(old, &a, &b, MergeOptions::new());
        assert_eq!(
            out,
            "\
foo (1.0-1) UNRELEASED; urgency=low

  * Line one A.
  * Shared line.
  * Line three B.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000

"
        );
        assert!(!conflicts);
    }

    #[test]
    fn test_new_author_sections_from_both_sides() {
        // Each side appends a new author section; both are kept.
        let old = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let a = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.

  [ Alice ]
  * Alice change.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let b = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.

  [ Bob ]
  * Bob change.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let (out, conflicts) = merge(old, a, b, MergeOptions::new());
        assert_eq!(
            out,
            "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.

[ Alice ]
  * Alice change.

[ Bob ]
  * Bob change.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000

"
        );
        assert!(!conflicts);
    }

    #[test]
    fn test_multiline_bullet_is_atomic() {
        // A bullet with sub-items is treated as one unit; independent new
        // bullets on each side are unioned without disturbing it.
        let old = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Big change:
    + part one
    + part two

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let a = old.replace("    + part two\n", "    + part two\n  * A new bullet.\n");
        let b = old.replace("    + part two\n", "    + part two\n  * B new bullet.\n");
        let (out, conflicts) = merge(old, &a, &b, MergeOptions::new());
        assert_eq!(
            out,
            "\
foo (1.0-1) UNRELEASED; urgency=low

  * Big change:
    + part one
    + part two
  * A new bullet.
  * B new bullet.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000

"
        );
        assert!(!conflicts);
    }

    #[test]
    fn test_identical_bullet_added_both_sides_dedups() {
        let old = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let both = "\
foo (1.0-1) UNRELEASED; urgency=low

  * Initial release.
  * Same new bullet.

 -- Jelmer <jelmer@debian.org>  Mon, 01 Jan 2024 00:00:00 +0000
";
        let (out, conflicts) = merge(old, both, both, MergeOptions::new());
        assert_eq!(out, both);
        assert!(!conflicts);
    }

    #[test]
    fn test_parse_change_sections() {
        let lines: Vec<String> = [
            "  * First bullet.",
            "  [ Alice ]",
            "  * Alice one.",
            "    + sub item",
            "  * Alice two.",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let sections = parse_change_sections(&lines);
        assert_eq!(
            sections,
            vec![
                ChangeSection {
                    title: None,
                    bullets: vec![vec!["  * First bullet.".to_string()]],
                },
                ChangeSection {
                    title: Some("Alice".to_string()),
                    bullets: vec![
                        vec!["  * Alice one.".to_string(), "    + sub item".to_string()],
                        vec!["  * Alice two.".to_string()],
                    ],
                },
            ]
        );
    }
}
