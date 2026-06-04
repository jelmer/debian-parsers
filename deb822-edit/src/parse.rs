//! Parse wrapper type following rust-analyzer's pattern for thread-safe storage in Salsa.

use crate::lossless::{Deb822, ParseError, PositionedParseError};
use rowan::ast::AstNode;
use rowan::{GreenNode, SyntaxNode};
use std::marker::PhantomData;

/// The result of parsing: a syntax tree and a collection of errors.
///
/// This type is designed to be stored in Salsa databases as it contains
/// the thread-safe `GreenNode` instead of the non-thread-safe `SyntaxNode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse<T> {
    green: GreenNode,
    errors: Vec<String>,
    positioned_errors: Vec<PositionedParseError>,
    _ty: PhantomData<T>,
}

impl<T> Parse<T> {
    /// Create a new Parse result from a GreenNode and errors
    pub fn new(green: GreenNode, errors: Vec<String>) -> Self {
        Parse {
            green,
            errors,
            positioned_errors: Vec::new(),
            _ty: PhantomData,
        }
    }

    /// Create a new Parse result from a GreenNode, errors, and positioned errors
    pub fn new_with_positioned_errors(
        green: GreenNode,
        errors: Vec<String>,
        positioned_errors: Vec<PositionedParseError>,
    ) -> Self {
        Parse {
            green,
            errors,
            positioned_errors,
            _ty: PhantomData,
        }
    }

    /// Get the green node (thread-safe representation)
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// Get the syntax errors
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Get parse errors with position information
    pub fn positioned_errors(&self) -> &[PositionedParseError] {
        &self.positioned_errors
    }

    /// Get parse errors as strings (for backward compatibility if needed)
    pub fn error_messages(&self) -> Vec<String> {
        self.positioned_errors
            .iter()
            .map(|e| e.message.clone())
            .collect()
    }

    /// Check if there are any errors
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Convert to a Result, returning the tree if there are no errors
    pub fn to_result(self) -> Result<T, ParseError>
    where
        T: AstNode<Language = crate::lossless::Lang>,
    {
        if self.errors.is_empty() {
            let node = SyntaxNode::new_root_mut(self.green);
            Ok(T::cast(node).expect("root node has wrong type"))
        } else {
            Err(ParseError(self.errors))
        }
    }

    /// Get the parsed syntax tree
    ///
    /// Returns the syntax tree even if there are parse errors.
    /// Use `errors()` or `positioned_errors()` to check for errors if needed.
    /// This allows for error-resilient tooling that can work with partial/invalid input.
    pub fn tree(&self) -> T
    where
        T: AstNode<Language = crate::lossless::Lang>,
    {
        let node = SyntaxNode::new_root_mut(self.green.clone());
        T::cast(node).expect("root node has wrong type")
    }

    /// Get the syntax node
    pub fn syntax_node(&self) -> SyntaxNode<crate::lossless::Lang> {
        SyntaxNode::new_root(self.green.clone())
    }
}

// Implement Send + Sync since GreenNode is thread-safe
unsafe impl<T> Send for Parse<T> {}
unsafe impl<T> Sync for Parse<T> {}

impl Parse<Deb822> {
    /// Parse deb822 text, returning a Parse result
    pub fn parse_deb822(text: &str) -> Self {
        let parsed = crate::lossless::parse(text);
        Parse::new_with_positioned_errors(
            parsed.green_node,
            parsed.errors,
            parsed.positioned_errors,
        )
    }

    /// Incrementally reparse after a text edit.
    ///
    /// Given the new full text and the range that was edited (in the *new* text
    /// coordinates after the edit has been applied), this tries to reuse
    /// unchanged paragraphs from the previous parse and only reparse the
    /// affected region.
    ///
    /// Falls back to a full reparse if the edit spans the entire file or if
    /// incremental reparsing is not beneficial.
    pub fn reparse(&self, new_text: &str, edit: rowan::TextRange) -> Self {
        use rowan::TextSize;

        let root = &self.green;

        // Collect children with their text ranges
        let mut children: Vec<(
            rowan::NodeOrToken<&rowan::GreenNodeData, &rowan::GreenTokenData>,
            TextSize,
            TextSize,
        )> = Vec::new();
        let mut offset = TextSize::from(0);
        for child in root.children() {
            let len = match &child {
                rowan::NodeOrToken::Node(n) => n.text_len(),
                rowan::NodeOrToken::Token(t) => t.text_len(),
            };
            children.push((child, offset, offset + len));
            offset += len;
        }

        let old_len = offset;

        // If there are very few children, just do a full reparse
        if children.len() <= 2 {
            return Self::parse_deb822(new_text);
        }

        // Find the range of children affected by the edit.
        // The edit range is in terms of the *new* text. We need to figure out
        // which old children overlap. The edit replaces old text
        // [edit.start .. edit.start + old_len - new_len + edit.len()] with
        // new text [edit.start .. edit.end].
        let new_len = TextSize::of(new_text);
        let len_delta: i64 = i64::from(u32::from(new_len)) - i64::from(u32::from(old_len));

        // In old-text coordinates, the edit covered:
        let old_edit_start = edit.start();
        let old_edit_end = TextSize::from((i64::from(u32::from(edit.end())) - len_delta) as u32);

        // Find first and last affected child indices.
        // Use >= for the first child to catch inserts at child boundaries.
        let first_affected = children
            .iter()
            .position(|(_, _, end)| *end >= old_edit_start);
        let last_affected = children
            .iter()
            .rposition(|(_, start, _)| *start <= old_edit_end);

        let (first_affected, last_affected) = match (first_affected, last_affected) {
            (Some(f), Some(l)) => (f, l),
            _ => return Self::parse_deb822(new_text),
        };

        // Expand to paragraph boundaries: find the text region in the *new*
        // text that covers all affected children.  We use the old children
        // offsets for the prefix (unchanged) and derive the suffix.
        let reparse_start = children[first_affected].1;
        let reparse_old_end = children[last_affected].2;

        // In new-text coordinates, the end of the affected region is shifted
        // by the length delta.
        let reparse_new_end =
            TextSize::from((i64::from(u32::from(reparse_old_end)) + len_delta) as u32);

        // Bounds check
        if u32::from(reparse_start) > u32::from(new_len)
            || u32::from(reparse_new_end) > u32::from(new_len)
        {
            return Self::parse_deb822(new_text);
        }

        let reparse_slice = &new_text[usize::from(reparse_start)..usize::from(reparse_new_end)];

        // Parse just the affected region
        let reparsed = crate::lossless::parse(reparse_slice);
        let reparsed_root = reparsed.green_node;

        // Build the new root by splicing: prefix children + reparsed children + suffix children
        let to_owned = |c: &rowan::NodeOrToken<&rowan::GreenNodeData, &rowan::GreenTokenData>| -> rowan::NodeOrToken<GreenNode, rowan::GreenToken> {
            match c {
                rowan::NodeOrToken::Node(n) => rowan::NodeOrToken::Node((*n).to_owned()),
                rowan::NodeOrToken::Token(t) => rowan::NodeOrToken::Token((*t).to_owned()),
            }
        };
        let mut new_root_children = Vec::new();
        for (c, _, _) in &children[..first_affected] {
            new_root_children.push(to_owned(c));
        }
        for c in reparsed_root.children() {
            new_root_children.push(c.to_owned());
        }
        for (c, _, _) in &children[last_affected + 1..] {
            new_root_children.push(to_owned(c));
        }

        let new_green = GreenNode::new(
            rowan::SyntaxKind(crate::lex::SyntaxKind::ROOT as u16),
            new_root_children,
        );

        // Collect errors: errors from unchanged prefix/suffix are tricky to
        // preserve with correct offsets, so we re-collect from scratch.
        // For a full solution we'd need to offset-shift the old errors, but
        // since most files are error-free, a full error re-scan is acceptable.
        // We use the reparsed errors (offset-shifted) plus any errors from
        // children outside the reparsed region.
        // For simplicity, combine positioned errors from the reparse
        // (shifted to absolute positions) with a note that prefix/suffix
        // errors are dropped. TODO: preserve prefix/suffix errors with shifted offsets.
        let positioned_errors: Vec<_> = reparsed
            .positioned_errors
            .iter()
            .map(|e| PositionedParseError {
                message: e.message.clone(),
                range: rowan::TextRange::new(
                    e.range.start() + reparse_start,
                    e.range.end() + reparse_start,
                ),
                code: e.code.clone(),
            })
            .collect();
        let errors: Vec<_> = positioned_errors
            .iter()
            .map(|e| e.message.clone())
            .collect();

        Parse::new_with_positioned_errors(new_green, errors, positioned_errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_positioned_errors_api() {
        let input = "Invalid field without colon\nBroken: field: extra colon\n";
        let parsed = Parse::<Deb822>::parse_deb822(input);

        // Should have positioned errors
        let positioned_errors = parsed.positioned_errors();
        assert!(!positioned_errors.is_empty());

        // Should still have string errors for backward compatibility
        let string_errors = parsed.errors();
        assert!(!string_errors.is_empty());

        // Should be able to get error messages
        let error_messages = parsed.error_messages();
        assert_eq!(error_messages.len(), positioned_errors.len());

        for (i, positioned_error) in positioned_errors.iter().enumerate() {
            assert_eq!(positioned_error.message, error_messages[i]);
            assert!(positioned_error.range.start() <= positioned_error.range.end());
        }
    }

    #[test]
    fn test_positioned_errors_example() {
        // Example from the requirements document
        let input = "Invalid: field\nBroken field without colon";
        let parsed = Parse::<Deb822>::parse_deb822(input);

        let positioned_errors = parsed.positioned_errors();
        assert!(!positioned_errors.is_empty());

        // Example usage like in a language server
        for error in positioned_errors {
            let start_offset: u32 = error.range.start().into();
            let end_offset: u32 = error.range.end().into();
            println!(
                "Error at {:?} ({}..{}): {}",
                error.range, start_offset, end_offset, error.message
            );

            // Verify we can extract the problematic text
            if end_offset <= input.len() as u32 {
                let error_text = &input[start_offset as usize..end_offset as usize];
                assert!(!error_text.is_empty());
            }
        }
    }

    /// Helper: parse old text, apply edit to get new text, reparse incrementally,
    /// and verify the result matches a full parse of the new text.
    fn assert_incremental_matches_full(
        old_text: &str,
        edit_start: u32,
        edit_end: u32,
        replacement: &str,
    ) {
        let parsed = Parse::<Deb822>::parse_deb822(old_text);
        let mut new_text = old_text.to_string();
        new_text.replace_range(edit_start as usize..edit_end as usize, replacement);

        // The edit range in new-text coordinates
        let new_edit_end = edit_start + replacement.len() as u32;
        let edit_range = rowan::TextRange::new(
            rowan::TextSize::from(edit_start),
            rowan::TextSize::from(new_edit_end),
        );

        let incremental = parsed.reparse(&new_text, edit_range);
        let full = Parse::<Deb822>::parse_deb822(&new_text);

        // The trees must produce identical text
        let inc_tree = incremental.tree();
        let full_tree = full.tree();
        assert_eq!(
            inc_tree.syntax().text().to_string(),
            full_tree.syntax().text().to_string(),
            "tree text mismatch for edit [{edit_start}..{edit_end}] -> {replacement:?}"
        );

        // Green nodes should be structurally equal
        assert_eq!(
            incremental.green(),
            full.green(),
            "green node mismatch for edit [{edit_start}..{edit_end}] -> {replacement:?}"
        );
    }

    #[test]
    fn test_reparse_edit_within_first_paragraph() {
        let old = "Source: foo\nMaintainer: Alice\n\nPackage: bar\nArchitecture: all\n";
        // Change "foo" to "baz" (bytes 8..11)
        assert_incremental_matches_full(old, 8, 11, "baz");
    }

    #[test]
    fn test_reparse_edit_within_second_paragraph() {
        let old = "Source: foo\nMaintainer: Alice\n\nPackage: bar\nArchitecture: all\n";
        // Change "bar" to "qux" (bytes 38..41)
        assert_incremental_matches_full(old, 38, 41, "qux");
    }

    #[test]
    fn test_reparse_insert_field_in_paragraph() {
        let old = "Source: foo\n\nPackage: bar\nArchitecture: all\n";
        // Insert "Section: net\n" after "Source: foo\n" (at byte 12)
        assert_incremental_matches_full(old, 12, 12, "Section: net\n");
    }

    #[test]
    fn test_reparse_delete_field() {
        let old = "Source: foo\nSection: net\nMaintainer: Alice\n\nPackage: bar\n";
        // Delete "Section: net\n" (bytes 12..25)
        assert_incremental_matches_full(old, 12, 25, "");
    }

    #[test]
    fn test_reparse_edit_spanning_paragraph_boundary() {
        let old = "Source: foo\n\nPackage: bar\n\nPackage: baz\n";
        // Delete the blank line and merge paragraphs (bytes 11..13)
        assert_incremental_matches_full(old, 11, 13, "\n");
    }

    #[test]
    fn test_reparse_single_paragraph() {
        // Only one paragraph — should fall back to full reparse
        let old = "Source: foo\nMaintainer: Alice\n";
        assert_incremental_matches_full(old, 8, 11, "bar");
    }

    #[test]
    fn test_reparse_add_new_paragraph() {
        let old = "Source: foo\n\nPackage: bar\n";
        // Add a new paragraph at the end
        assert_incremental_matches_full(old, 25, 25, "\nPackage: baz\nArchitecture: any\n");
    }

    #[test]
    fn test_tree_with_errors_does_not_panic() {
        // Verify that tree() returns a tree even when there are parse errors,
        // enabling error-resilient tooling
        let input = "Invalid field without colon\nBroken: field: extra colon\n";
        let parsed = Parse::<Deb822>::parse_deb822(input);

        // Verify there are errors
        assert!(!parsed.errors().is_empty());

        // tree() should not panic despite errors
        let tree = parsed.tree();

        // Verify we got a valid syntax tree
        assert!(tree.syntax().text().to_string() == input);
    }
}
