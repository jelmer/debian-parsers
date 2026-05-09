use crate::lex::lex;
use crate::SyntaxKind;
use crate::SyntaxKind::*;
#[cfg(feature = "chrono")]
use chrono::{DateTime, FixedOffset};
use debversion::Version;
use rowan::ast::AstNode;
use std::str::FromStr;

/// Trait for types that can be converted to a timestamp string
///
/// This trait allows both chrono DateTime types and plain strings to be used
/// as timestamps in the changelog API.
pub trait IntoTimestamp {
    /// Convert this value into a timestamp string in Debian changelog format
    fn into_timestamp(self) -> String;
}

impl IntoTimestamp for String {
    fn into_timestamp(self) -> String {
        self
    }
}

impl IntoTimestamp for &str {
    fn into_timestamp(self) -> String {
        self.to_string()
    }
}

#[cfg(feature = "chrono")]
impl<Tz: chrono::TimeZone> IntoTimestamp for DateTime<Tz>
where
    Tz::Offset: std::fmt::Display,
{
    fn into_timestamp(self) -> String {
        const CHANGELOG_TIME_FORMAT: &str = "%a, %d %b %Y %H:%M:%S %z";
        self.format(CHANGELOG_TIME_FORMAT).to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
/// Urgency of the changes in the changelog entry
pub enum Urgency {
    /// Low urgency
    Low,

    #[default]
    /// Medium urgency
    Medium,

    /// High urgency
    High,

    /// Emergency urgency
    Emergency,

    /// Critical urgency
    Critical,
}

impl std::fmt::Display for Urgency {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Urgency::Low => f.write_str("low"),
            Urgency::Medium => f.write_str("medium"),
            Urgency::High => f.write_str("high"),
            Urgency::Emergency => f.write_str("emergency"),
            Urgency::Critical => f.write_str("critical"),
        }
    }
}

impl FromStr for Urgency {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(Urgency::Low),
            "medium" => Ok(Urgency::Medium),
            "high" => Ok(Urgency::High),
            "emergency" => Ok(Urgency::Emergency),
            "critical" => Ok(Urgency::Critical),
            _ => Err(ParseError(vec![format!("invalid urgency: {}", s)])),
        }
    }
}

#[derive(Debug)]
/// Error while reading a changelog file.
pub enum Error {
    /// I/O Error
    Io(std::io::Error),

    /// Parsing error
    Parse(ParseError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self {
            Error::Io(e) => write!(f, "IO error: {}", e),
            Error::Parse(e) => write!(f, "Parse error: {}", e),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Error while parsing
pub struct ParseError(Vec<String>);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for err in &self.0 {
            writeln!(f, "{}", err)?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

impl From<ParseError> for Error {
    fn from(e: ParseError) -> Self {
        Error::Parse(e)
    }
}

/// Second, implementing the `Language` trait teaches rowan to convert between
/// these two SyntaxKind types, allowing for a nicer SyntaxNode API where
/// "kinds" are values from our `enum SyntaxKind`, instead of plain u16 values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {}
impl rowan::Language for Lang {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }
    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

/// GreenNode is an immutable tree, which is cheap to change,
/// but doesn't contain offsets and parent pointers.
use rowan::{GreenNode, GreenToken};

/// You can construct GreenNodes by hand, but a builder
/// is helpful for top-down parsers: it maintains a stack
/// of currently in-progress nodes
use rowan::GreenNodeBuilder;

/// The result of parsing: a syntax tree and a collection of errors.
///
/// This type is designed to be stored in Salsa databases as it contains
/// the thread-safe `GreenNode` instead of the non-thread-safe `SyntaxNode`.
#[derive(Debug)]
pub struct Parse<T> {
    green: GreenNode,
    errors: Vec<(String, rowan::TextSize)>,
    _ty: std::marker::PhantomData<T>,
}

// The T parameter is only used as a phantom type, so we can implement Clone and PartialEq
// without requiring T to implement them
impl<T> Clone for Parse<T> {
    fn clone(&self) -> Self {
        Parse {
            green: self.green.clone(),
            errors: self.errors.clone(),
            _ty: std::marker::PhantomData,
        }
    }
}

impl<T> PartialEq for Parse<T> {
    fn eq(&self, other: &Self) -> bool {
        self.green == other.green && self.errors == other.errors
    }
}

impl<T> Eq for Parse<T> {}

// Implement Send + Sync since GreenNode is thread-safe
unsafe impl<T> Send for Parse<T> {}
unsafe impl<T> Sync for Parse<T> {}

impl<T> Parse<T> {
    /// Create a new Parse result from a GreenNode and errors
    pub fn new(green: GreenNode, errors: Vec<(String, rowan::TextSize)>) -> Self {
        Parse {
            green,
            errors,
            _ty: std::marker::PhantomData,
        }
    }

    /// Get the green node (thread-safe representation)
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// Get the syntax error messages.
    pub fn errors(&self) -> Vec<&str> {
        self.errors.iter().map(|(msg, _)| msg.as_str()).collect()
    }

    /// Get the syntax errors with their byte offsets.
    pub fn errors_with_offsets(&self) -> &[(String, rowan::TextSize)] {
        &self.errors
    }

    /// Check if there are any errors
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Convert to a Result, returning the tree if there are no errors
    pub fn to_result(self) -> Result<T, ParseError>
    where
        T: AstNode<Language = Lang>,
    {
        if self.errors.is_empty() {
            let node = SyntaxNode::new_root(self.green);
            Ok(T::cast(node).expect("root node has wrong type"))
        } else {
            Err(ParseError(
                self.errors.into_iter().map(|(msg, _)| msg).collect(),
            ))
        }
    }

    /// Convert to a Result, returning a mutable tree if there are no errors
    pub fn to_mut_result(self) -> Result<T, ParseError>
    where
        T: AstNode<Language = Lang>,
    {
        if self.errors.is_empty() {
            let node = SyntaxNode::new_root_mut(self.green);
            Ok(T::cast(node).expect("root node has wrong type"))
        } else {
            Err(ParseError(
                self.errors.into_iter().map(|(msg, _)| msg).collect(),
            ))
        }
    }

    /// Get the parsed syntax tree.
    ///
    /// The tree is returned even when there are parse errors, since the parser
    /// always produces a valid (possibly incomplete) green tree. Callers that
    /// need to distinguish error-free parses should check [`errors()`] first.
    pub fn tree(&self) -> T
    where
        T: AstNode<Language = Lang>,
    {
        let node = SyntaxNode::new_root(self.green.clone());
        T::cast(node).expect("root node has wrong type")
    }

    /// Get the syntax node
    pub fn syntax_node(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// Get a mutable parsed syntax tree.
    ///
    /// The tree is returned even when there are parse errors, since the parser
    /// always produces a valid (possibly incomplete) green tree. Callers that
    /// need to distinguish error-free parses should check [`errors()`] first.
    pub fn tree_mut(&self) -> T
    where
        T: AstNode<Language = Lang>,
    {
        let node = SyntaxNode::new_root_mut(self.green.clone());
        T::cast(node).expect("root node has wrong type")
    }
}

impl Parse<ChangeLog> {
    /// Incrementally reparse after a text edit.
    ///
    /// Given the new full text and the range that was edited (in the *new* text
    /// coordinates after the edit has been applied), this tries to reuse
    /// unchanged entries from the previous parse and only reparse the
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
            return parse(new_text);
        }

        let new_len = TextSize::of(new_text);
        let len_delta: i64 = i64::from(u32::from(new_len)) - i64::from(u32::from(old_len));

        // In old-text coordinates, the edit covered:
        let old_edit_start = edit.start();
        let old_edit_end = TextSize::from((i64::from(u32::from(edit.end())) - len_delta) as u32);

        // Find first and last affected child indices.
        // Use >= / <= to catch inserts at child boundaries.
        let first_affected = children
            .iter()
            .position(|(_, _, end)| *end >= old_edit_start);
        let last_affected = children
            .iter()
            .rposition(|(_, start, _)| *start <= old_edit_end);

        let (first_affected, last_affected) = match (first_affected, last_affected) {
            (Some(f), Some(l)) => (f, l),
            _ => return parse(new_text),
        };

        let reparse_start = children[first_affected].1;
        let reparse_old_end = children[last_affected].2;

        // In new-text coordinates, the end of the affected region is shifted
        let reparse_new_end =
            TextSize::from((i64::from(u32::from(reparse_old_end)) + len_delta) as u32);

        // Bounds check
        if u32::from(reparse_start) > u32::from(new_len)
            || u32::from(reparse_new_end) > u32::from(new_len)
        {
            return parse(new_text);
        }

        let reparse_slice = &new_text[usize::from(reparse_start)..usize::from(reparse_new_end)];

        // Parse just the affected region
        let reparsed = parse(reparse_slice);
        let reparsed_root = reparsed.green;

        // Build the new root by splicing
        let to_owned = |c: &rowan::NodeOrToken<
            &rowan::GreenNodeData,
            &rowan::GreenTokenData,
        >|
         -> rowan::NodeOrToken<GreenNode, rowan::GreenToken> {
            match c {
                rowan::NodeOrToken::Node(n) => {
                    rowan::NodeOrToken::Node((*n).to_owned())
                }
                rowan::NodeOrToken::Token(t) => {
                    rowan::NodeOrToken::Token((*t).to_owned())
                }
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

        let new_green = GreenNode::new(rowan::SyntaxKind(ROOT as u16), new_root_children);

        // Build the error list by combining:
        // 1. Prefix errors: original errors before the affected region (unchanged)
        // 2. Reparsed errors: shifted to absolute positions by adding reparse_start
        // 3. Suffix errors: original errors after the affected region, shifted by len_delta
        let mut errors = Vec::new();

        // Prefix errors (before the affected region, offsets unchanged)
        for (msg, offset) in &self.errors {
            if *offset < reparse_start {
                errors.push((msg.clone(), *offset));
            }
        }

        // Reparsed errors (shift from slice-local to absolute positions)
        for (msg, offset) in reparsed.errors {
            errors.push((msg, offset + reparse_start));
        }

        // Suffix errors (after the affected region, shift by len_delta)
        for (msg, offset) in &self.errors {
            if *offset >= reparse_old_end {
                let new_offset = TextSize::from((i64::from(u32::from(*offset)) + len_delta) as u32);
                errors.push((msg.clone(), new_offset));
            }
        }

        Parse::new(new_green, errors)
    }
}

fn parse(text: &str) -> Parse<ChangeLog> {
    struct Parser {
        /// input tokens, including whitespace,
        /// in *reverse* order.
        tokens: Vec<(SyntaxKind, String)>,
        /// the in-progress tree.
        builder: GreenNodeBuilder<'static>,
        /// the list of syntax errors we've accumulated
        /// so far, with their byte offsets.
        errors: Vec<(String, rowan::TextSize)>,
        /// current byte offset in the input text.
        text_offset: rowan::TextSize,
    }

    impl Parser {
        fn error(&mut self, msg: String) {
            let offset = self.text_offset;
            self.builder.start_node(ERROR.into());
            if self.current().is_some() {
                self.bump();
            }
            self.errors.push((msg, offset));
            self.builder.finish_node();
        }

        fn parse_entry_header(&mut self) {
            self.builder.start_node(ENTRY_HEADER.into());
            self.expect(IDENTIFIER);

            self.skip_ws();

            if self.current() == Some(NEWLINE) {
                self.bump();
                self.builder.finish_node();
                return;
            }

            self.expect(VERSION);

            self.skip_ws();

            self.builder.start_node(DISTRIBUTIONS.into());
            loop {
                match self.current() {
                    Some(IDENTIFIER) => self.bump(),
                    Some(NEWLINE) => {
                        self.bump();
                        self.builder.finish_node();
                        self.builder.finish_node();
                        return;
                    }
                    Some(SEMICOLON) => {
                        break;
                    }
                    _ => {
                        self.error("expected distribution or semicolon".to_string());
                        break;
                    }
                }
                self.skip_ws();
            }
            self.builder.finish_node();

            self.skip_ws();

            self.builder.start_node(METADATA.into());
            if self.current() == Some(SEMICOLON) {
                self.bump();
                loop {
                    self.skip_ws();

                    if self.current() == Some(NEWLINE) {
                        break;
                    }

                    self.builder.start_node(METADATA_ENTRY.into());
                    if self.current() == Some(IDENTIFIER) {
                        self.builder.start_node(METADATA_KEY.into());
                        self.bump();
                        self.builder.finish_node();
                    } else {
                        self.error("expected metadata key".to_string());
                        self.builder.finish_node();
                        break;
                    }

                    if self.current() == Some(EQUALS) {
                        self.bump();
                    } else {
                        self.error("expected equals".to_string());
                        self.builder.finish_node();
                        break;
                    }

                    if self.current() == Some(IDENTIFIER) {
                        self.builder.start_node(METADATA_VALUE.into());
                        self.bump();
                        // Handle old-style metadata values that may contain spaces and multiple tokens
                        // e.g., "closes=53715 56047 56607"
                        loop {
                            match (self.current(), self.next()) {
                                // Stop if we see a new key=value pattern (IDENTIFIER followed by EQUALS)
                                (Some(WHITESPACE), Some(IDENTIFIER)) => {
                                    // Look further ahead to see if there's an EQUALS after the identifier
                                    // If there is, this is a new metadata entry, so stop here
                                    // Otherwise, consume the whitespace and identifier as part of the value
                                    if self.tokens.len() >= 3 {
                                        if let Some((kind, _)) =
                                            self.tokens.get(self.tokens.len() - 3)
                                        {
                                            if *kind == EQUALS {
                                                break; // Next token starts a new metadata entry
                                            }
                                        }
                                    }
                                    self.bump(); // consume whitespace
                                }
                                (Some(WHITESPACE), _) => self.bump(),
                                (Some(IDENTIFIER), _) => self.bump(),
                                _ => break,
                            }
                        }
                        self.builder.finish_node();
                    } else {
                        self.error("expected metadata value".to_string());
                        self.builder.finish_node();
                        break;
                    }
                    self.builder.finish_node();

                    // Skip comma separators (old-style format)
                    self.skip_ws();
                    if self.current() == Some(ERROR) {
                        // Peek at the token text to see if it's a comma
                        if let Some((_, text)) = self.tokens.last() {
                            if text == "," {
                                self.bump(); // consume the comma
                                continue;
                            }
                        }
                    }
                }
            } else if self.current() == Some(NEWLINE) {
            } else {
                self.error("expected semicolon or newline".to_string());
            }
            self.builder.finish_node();

            self.expect(NEWLINE);
            self.builder.finish_node();
        }

        fn parse_entry(&mut self) {
            self.builder.start_node(ENTRY.into());
            self.parse_entry_header();
            loop {
                match self
                    .tokens
                    .last()
                    .map(|(kind, token)| (kind, token.as_str()))
                {
                    None => {
                        // End of file - entry without footer is valid
                        break;
                    }
                    // empty line
                    Some((NEWLINE, _)) => {
                        self.builder.start_node(EMPTY_LINE.into());
                        self.bump();
                        self.builder.finish_node();
                    }
                    // details
                    Some((INDENT, "  ")) => {
                        self.parse_entry_detail();
                    }
                    // footer
                    Some((INDENT, " -- ")) => {
                        self.parse_entry_footer();
                        break;
                    }
                    _ => break,
                }
            }

            self.builder.finish_node();
        }

        pub fn parse_entry_detail(&mut self) {
            self.builder.start_node(ENTRY_BODY.into());
            self.expect(INDENT);

            match self.current() {
                Some(DETAIL) => {
                    self.bump();
                }
                Some(NEWLINE) => {}
                _ => {
                    self.error("expected detail".to_string());
                }
            }

            self.expect(NEWLINE);
            self.builder.finish_node();
        }

        pub fn parse_entry_footer(&mut self) {
            self.builder.start_node(ENTRY_FOOTER.into());

            if self.current() != Some(INDENT) {
                self.error("expected indent".to_string());
            } else {
                let dashes = &self.tokens.last().unwrap().1;
                if dashes != " -- " {
                    self.error("expected --".to_string());
                } else {
                    self.bump();
                }
            }

            self.builder.start_node(MAINTAINER.into());
            while self.current() == Some(TEXT)
                || (self.current() == Some(WHITESPACE) && self.next() != Some(EMAIL))
            {
                self.bump();
            }
            self.builder.finish_node();

            if self.current().is_some() && self.current() != Some(NEWLINE) {
                self.expect(WHITESPACE);
            }

            if self.current().is_some() && self.current() != Some(NEWLINE) {
                self.expect(EMAIL);
            }

            if self.tokens.last().map(|(k, t)| (*k, t.as_str())) == Some((WHITESPACE, "  ")) {
                self.bump();
            } else if self.current() == Some(WHITESPACE) {
                self.error("expected two spaces".to_string());
            } else if self.current() == Some(NEWLINE) {
                self.bump();
                self.builder.finish_node();
                return;
            } else {
                self.error(format!("expected whitespace, got {:?}", self.current()));
            }

            self.builder.start_node(TIMESTAMP.into());

            loop {
                if self.current() != Some(TEXT) && self.current() != Some(WHITESPACE) {
                    break;
                }
                self.bump();
            }
            self.builder.finish_node();

            self.expect(NEWLINE);
            self.builder.finish_node();
        }

        fn parse(mut self) -> Parse<ChangeLog> {
            self.builder.start_node(ROOT.into());
            loop {
                match self.current() {
                    None => break,
                    Some(NEWLINE) => {
                        self.builder.start_node(EMPTY_LINE.into());
                        self.bump();
                        self.builder.finish_node();
                    }
                    Some(COMMENT) => {
                        self.bump();
                    }
                    Some(IDENTIFIER) => {
                        self.parse_entry();
                    }
                    t => {
                        self.error(format!("unexpected token {:?}", t));
                        self.bump();
                    }
                }
            }
            // Close the root node.
            self.builder.finish_node();

            // Turn the builder into a GreenNode
            Parse::new(self.builder.finish(), self.errors)
        }
        /// Advance one token, adding it to the current branch of the tree builder.
        fn bump(&mut self) {
            let (kind, text) = self.tokens.pop().unwrap();
            self.text_offset += rowan::TextSize::of(text.as_str());
            self.builder.token(kind.into(), text.as_str());
        }
        /// Peek at the first unprocessed token
        fn current(&self) -> Option<SyntaxKind> {
            self.tokens.last().map(|(kind, _)| *kind)
        }

        fn next(&self) -> Option<SyntaxKind> {
            self.tokens
                .get(self.tokens.len() - 2)
                .map(|(kind, _)| *kind)
        }

        fn expect(&mut self, expected: SyntaxKind) {
            if self.current() != Some(expected) {
                self.error(format!("expected {:?}, got {:?}", expected, self.current()));
            } else {
                self.bump();
            }
        }
        fn skip_ws(&mut self) {
            while self.current() == Some(WHITESPACE) {
                self.bump()
            }
        }
    }

    let mut tokens = lex(text);
    tokens.reverse();
    Parser {
        tokens,
        builder: GreenNodeBuilder::new(),
        errors: Vec::new(),
        text_offset: rowan::TextSize::from(0),
    }
    .parse()
}

// To work with the parse results we need a view into the
// green tree - the Syntax tree.
// It is also immutable, like a GreenNode,
// but it contains parent pointers, offsets, and
// has identity semantics.

pub type SyntaxNode = rowan::SyntaxNode<Lang>;
#[allow(unused)]
pub type SyntaxToken = rowan::SyntaxToken<Lang>;
#[allow(unused)]
type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

/// Calculate line and column (both 0-indexed) for the given offset in the tree.
/// Column is measured in bytes from the start of the line.
pub(crate) fn line_col_at_offset(node: &SyntaxNode, offset: rowan::TextSize) -> (usize, usize) {
    let root = node.ancestors().last().unwrap_or_else(|| node.clone());
    let mut line = 0;
    let mut last_newline_offset = rowan::TextSize::from(0);

    for element in root.preorder_with_tokens() {
        if let rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(token)) = element {
            if token.text_range().start() >= offset {
                break;
            }

            // Count newlines and track position of last one
            for (idx, _) in token.text().match_indices('\n') {
                line += 1;
                last_newline_offset =
                    token.text_range().start() + rowan::TextSize::from((idx + 1) as u32);
            }
        }
    }

    let column: usize = (offset - last_newline_offset).into();
    (line, column)
}

macro_rules! ast_node {
    ($ast:ident, $kind:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        /// A node in the changelog syntax tree.
        pub struct $ast(SyntaxNode);

        impl AstNode for $ast {
            type Language = Lang;

            fn can_cast(kind: SyntaxKind) -> bool {
                kind == $kind
            }

            fn cast(syntax: SyntaxNode) -> Option<Self> {
                if Self::can_cast(syntax.kind()) {
                    Some(Self(syntax))
                } else {
                    None
                }
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }

        impl $ast {
            #[allow(dead_code)]
            fn replace_root(&mut self, new_root: SyntaxNode) {
                self.0 = Self::cast(new_root).unwrap().0;
            }

            /// Get the line number (0-indexed) where this node starts.
            pub fn line(&self) -> usize {
                line_col_at_offset(&self.0, self.0.text_range().start()).0
            }

            /// Get the column number (0-indexed, in bytes) where this node starts.
            pub fn column(&self) -> usize {
                line_col_at_offset(&self.0, self.0.text_range().start()).1
            }

            /// Get both line and column (0-indexed) where this node starts.
            /// Returns (line, column) where column is measured in bytes from the start of the line.
            pub fn line_col(&self) -> (usize, usize) {
                line_col_at_offset(&self.0, self.0.text_range().start())
            }
        }

        impl std::fmt::Display for $ast {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(self.0.text().to_string().as_str())
            }
        }
    };
}

ast_node!(ChangeLog, ROOT);
ast_node!(Entry, ENTRY);
ast_node!(EntryHeader, ENTRY_HEADER);
ast_node!(EntryBody, ENTRY_BODY);
ast_node!(EntryFooter, ENTRY_FOOTER);
ast_node!(Maintainer, MAINTAINER);
ast_node!(Timestamp, TIMESTAMP);
ast_node!(MetadataEntry, METADATA_ENTRY);
ast_node!(MetadataKey, METADATA_KEY);
ast_node!(MetadataValue, METADATA_VALUE);

impl MetadataEntry {
    /// Returns the key of the metadata entry.
    pub fn key(&self) -> Option<String> {
        self.0
            .children()
            .find_map(MetadataKey::cast)
            .map(|k| k.to_string())
    }

    /// Returns the value of the metadata entry.
    pub fn value(&self) -> Option<String> {
        self.0
            .children()
            .find_map(MetadataValue::cast)
            .map(|k| k.to_string())
    }

    /// Sets the value of the metadata entry.
    pub fn set_value(&mut self, value: &str) {
        let node = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == METADATA_VALUE);
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(METADATA_VALUE.into());
        builder.token(IDENTIFIER.into(), value);
        builder.finish_node();
        let root = SyntaxNode::new_root_mut(builder.finish());

        let range = if let Some(node) = node {
            node.index()..node.index() + 1
        } else {
            let count = self.0.children().count();
            count..count
        };

        self.0.splice_children(range, vec![root.into()]);
    }
}

/// A builder for a changelog entry.
pub struct EntryBuilder {
    root: SyntaxNode,
    package: Option<String>,
    version: Option<Version>,
    distributions: Option<Vec<String>>,
    urgency: Option<Urgency>,
    maintainer: Option<(String, String)>,
    timestamp_string: Option<String>,
    change_lines: Vec<String>,
}

impl EntryBuilder {
    /// Set the package name
    #[must_use]
    pub fn package(mut self, package: String) -> Self {
        self.package = Some(package);
        self
    }

    /// Set the package version
    #[must_use]
    pub fn version(mut self, version: Version) -> Self {
        self.version = Some(version);
        self
    }

    /// Set the distribution(s)
    #[must_use]
    pub fn distributions(mut self, distributions: Vec<String>) -> Self {
        self.distributions = Some(distributions);
        self
    }

    #[must_use]
    pub fn distribution(mut self, distribution: String) -> Self {
        self.distributions
            .get_or_insert_with(Vec::new)
            .push(distribution);
        self
    }

    #[must_use]
    pub fn urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = Some(urgency);
        self
    }

    #[must_use]
    pub fn maintainer(mut self, maintainer: (String, String)) -> Self {
        self.maintainer = Some(maintainer);
        self
    }

    /// Set the timestamp (accepts chrono::DateTime or String)
    #[must_use]
    pub fn datetime(mut self, timestamp: impl IntoTimestamp) -> Self {
        self.timestamp_string = Some(timestamp.into_timestamp());
        self
    }

    #[must_use]
    pub fn change_line(mut self, line: String) -> Self {
        self.change_lines.push(line);
        self
    }

    pub fn verify(&self) -> Result<(), String> {
        if self.package.is_none() {
            return Err("package is required".to_string());
        }
        if self.version.is_none() {
            return Err("version is required".to_string());
        }
        match self.distributions {
            None => {
                return Err("at least one distribution is required".to_string());
            }
            Some(ref distributions) => {
                if distributions.is_empty() {
                    return Err("at least one distribution is required".to_string());
                }
            }
        }
        if self.change_lines.is_empty() {
            return Err("at least one change line is required".to_string());
        }
        Ok(())
    }

    fn metadata(&self) -> impl Iterator<Item = (String, String)> {
        let mut ret = vec![];
        if let Some(urgency) = self.urgency.as_ref() {
            ret.push(("urgency".to_string(), urgency.to_string()));
        }
        ret.into_iter()
    }

    pub fn finish(self) -> Entry {
        if self.root.children().find_map(Entry::cast).is_some() {
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(EMPTY_LINE.into());
            builder.token(NEWLINE.into(), "\n");
            builder.finish_node();
            let syntax = SyntaxNode::new_root_mut(builder.finish());
            self.root.splice_children(0..0, vec![syntax.into()]);
        }

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ENTRY.into());
        builder.start_node(ENTRY_HEADER.into());
        if let Some(package) = self.package.as_ref() {
            builder.token(IDENTIFIER.into(), package.as_str());
        }
        if let Some(version) = self.version.as_ref() {
            builder.token(WHITESPACE.into(), " ");
            builder.token(VERSION.into(), format!("({})", version).as_str());
        }
        if let Some(distributions) = self.distributions.as_ref() {
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(DISTRIBUTIONS.into());
            let mut it = distributions.iter().peekable();
            while it.peek().is_some() {
                builder.token(IDENTIFIER.into(), it.next().unwrap());
                if it.peek().is_some() {
                    builder.token(WHITESPACE.into(), " ");
                }
            }
            builder.finish_node(); // DISTRIBUTIONS
        }
        let mut metadata = self.metadata().peekable();
        if metadata.peek().is_some() {
            builder.token(SEMICOLON.into(), ";");
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(METADATA.into());
            for (key, value) in metadata {
                builder.start_node(METADATA_ENTRY.into());
                builder.start_node(METADATA_KEY.into());
                builder.token(IDENTIFIER.into(), key.as_str());
                builder.finish_node(); // METADATA_KEY
                builder.token(EQUALS.into(), "=");
                builder.start_node(METADATA_VALUE.into());
                builder.token(METADATA_VALUE.into(), value.as_str());
                builder.finish_node(); // METADATA_VALUE
                builder.finish_node(); // METADATA_ENTRY
            }
            builder.finish_node(); // METADATA
        }
        builder.token(NEWLINE.into(), "\n");
        builder.finish_node(); // ENTRY_HEADER

        builder.start_node(EMPTY_LINE.into());
        builder.token(NEWLINE.into(), "\n");
        builder.finish_node(); // EMPTY_LINE

        for line in self.change_lines {
            builder.start_node(ENTRY_BODY.into());
            builder.token(INDENT.into(), "  ");
            builder.token(DETAIL.into(), line.as_str());
            builder.token(NEWLINE.into(), "\n");
            builder.finish_node(); // ENTRY_BODY
        }

        builder.start_node(EMPTY_LINE.into());
        builder.token(NEWLINE.into(), "\n");
        builder.finish_node(); // EMPTY_LINE

        builder.start_node(ENTRY_FOOTER.into());
        builder.token(INDENT.into(), " -- ");
        if let Some(maintainer) = self.maintainer.as_ref() {
            builder.start_node(MAINTAINER.into());
            let mut it = maintainer.0.split(' ').peekable();
            while let Some(p) = it.next() {
                builder.token(TEXT.into(), p);
                if it.peek().is_some() {
                    builder.token(WHITESPACE.into(), " ");
                }
            }
            builder.finish_node(); // MAINTAINER
        }

        if let Some(maintainer) = self.maintainer.as_ref() {
            builder.token(WHITESPACE.into(), " ");
            builder.token(EMAIL.into(), format!("<{}>", maintainer.1).as_str());
        }

        if let Some(timestamp) = self.timestamp_string.as_ref() {
            builder.token(WHITESPACE.into(), "  ");

            builder.start_node(TIMESTAMP.into());
            let mut it = timestamp.split(' ').peekable();
            while let Some(p) = it.next() {
                builder.token(TEXT.into(), p);
                if it.peek().is_some() {
                    builder.token(WHITESPACE.into(), " ");
                }
            }
            builder.finish_node(); // TIMESTAMP
        }
        builder.token(NEWLINE.into(), "\n");
        builder.finish_node(); // ENTRY_FOOTER

        builder.finish_node(); // ENTRY
        let syntax = SyntaxNode::new_root_mut(builder.finish());
        self.root.splice_children(0..0, vec![syntax.clone().into()]);
        Entry(syntax)
    }
}

impl IntoIterator for ChangeLog {
    type Item = Entry;
    type IntoIter = std::vec::IntoIter<Entry>;

    fn into_iter(self) -> Self::IntoIter {
        // TODO: This is inefficient
        self.iter().collect::<Vec<_>>().into_iter()
    }
}

fn replay(builder: &mut GreenNodeBuilder, node: SyntaxNode) {
    builder.start_node(node.kind().into());
    for child in node.children_with_tokens() {
        match child {
            SyntaxElement::Node(n) => replay(builder, n),
            SyntaxElement::Token(t) => {
                builder.token(t.kind().into(), t.text());
            }
        }
    }
    builder.finish_node();
}

impl FromIterator<Entry> for ChangeLog {
    fn from_iter<T: IntoIterator<Item = Entry>>(iter: T) -> Self {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ROOT.into());
        for entry in iter {
            replay(&mut builder, entry.0.clone());
        }
        builder.finish_node();
        ChangeLog(SyntaxNode::new_root_mut(builder.finish()))
    }
}

impl ChangeLog {
    /// Create a new, empty changelog.
    pub fn new() -> ChangeLog {
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(ROOT.into());
        builder.finish_node();

        let syntax = SyntaxNode::new_root_mut(builder.finish());
        ChangeLog(syntax)
    }

    /// Parse changelog text, returning a Parse result
    pub fn parse(text: &str) -> Parse<ChangeLog> {
        parse(text)
    }

    /// Returns an iterator over all entries in the changelog file.
    pub fn iter(&self) -> impl Iterator<Item = Entry> + '_ {
        self.0.children().filter_map(Entry::cast)
    }

    /// Returns an iterator over all entries in the changelog file.
    #[deprecated(since = "0.2.0", note = "use `iter` instead")]
    pub fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        self.iter()
    }

    /// Find the entry that contains the given text position.
    ///
    /// This uses the CST to efficiently locate the entry containing the position
    /// without iterating through all entries (O(log n) complexity).
    ///
    /// # Arguments
    /// * `offset` - The text offset (byte position) to search for
    ///
    /// # Returns
    /// The Entry containing the position, or None if the position is outside all entries
    pub fn entry_at_offset(&self, offset: rowan::TextSize) -> Option<Entry> {
        // Use rowan's token_at_offset for efficient O(log n) lookup
        let token = self.0.token_at_offset(offset).right_biased()?;

        // Walk up the tree to find the Entry node
        token.parent_ancestors().find_map(Entry::cast)
    }

    /// Find all entries that intersect with the given text range.
    ///
    /// This uses the sequential nature of entries and early termination to efficiently
    /// find only the entries that intersect with the range, without scanning the entire file.
    ///
    /// # Arguments
    /// * `range` - The text range to search within
    ///
    /// # Returns
    /// An iterator over entries that intersect with the range
    pub fn entries_in_range(&self, range: rowan::TextRange) -> impl Iterator<Item = Entry> + '_ {
        // Entries are direct children of the root and are sequential
        // We can use early termination: skip entries before the range, yield intersecting entries,
        // and stop once we pass the range
        self.0
            .children()
            .filter_map(Entry::cast)
            .skip_while(move |entry| {
                // Skip entries that end before our range starts
                entry.syntax().text_range().end() <= range.start()
            })
            .take_while(move |entry| {
                // Stop once we find an entry that starts after our range ends
                entry.syntax().text_range().start() < range.end()
            })
    }

    /// Create a new, empty entry.
    pub fn new_empty_entry(&mut self) -> EntryBuilder {
        EntryBuilder {
            root: self.0.clone(),
            package: None,
            version: None,
            distributions: None,
            urgency: None,
            maintainer: None,
            timestamp_string: None,
            change_lines: vec![],
        }
    }

    fn first_valid_entry(&self) -> Option<Entry> {
        self.iter().find(|entry| {
            entry.package().is_some() && entry.header().is_some() && entry.footer().is_some()
        })
    }

    /// Return a builder for a new entry.
    pub fn new_entry(&mut self) -> EntryBuilder {
        let base_entry = self.first_valid_entry();
        let package = base_entry
            .as_ref()
            .and_then(|first_entry| first_entry.package());
        let mut version = base_entry
            .as_ref()
            .and_then(|first_entry| first_entry.version());
        if let Some(version) = version.as_mut() {
            version.increment_debian();
        }
        EntryBuilder {
            root: if self.0.is_mutable() {
                self.0.clone()
            } else {
                self.0.clone_for_update()
            },
            package,
            version,
            distributions: Some(vec![crate::UNRELEASED.into()]),
            urgency: Some(Urgency::default()),
            maintainer: crate::get_maintainer(),
            #[cfg(feature = "chrono")]
            timestamp_string: Some(chrono::Utc::now().into_timestamp()),
            #[cfg(not(feature = "chrono"))]
            timestamp_string: None,
            change_lines: vec![],
        }
    }

    /// Add a change to the changelog.
    ///
    /// This will update the current changelog entry if it is considered
    /// unreleased. Otherwise, a new entry will be created.
    ///
    /// If there is an existing entry, the change will be added to the end of
    /// the entry. If the previous change was attributed to another author,
    /// a new section line ("[ Author Name ]") will be added as well.
    ///
    /// # Arguments
    /// * `change` - The change to add, e.g. &["* Fix a bug"]
    /// * `author` - The author of the change, e.g. ("John Doe", "john@example")
    ///
    /// # Errors
    ///
    /// Returns an error if text rewrapping fails.
    pub fn try_auto_add_change(
        &mut self,
        change: &[&str],
        author: (String, String),
        datetime: Option<impl IntoTimestamp>,
        urgency: Option<Urgency>,
    ) -> Result<Entry, crate::textwrap::Error> {
        match self.first_valid_entry() {
            Some(entry) if entry.is_unreleased() != Some(false) => {
                // Add to existing entry
                entry.try_add_change_for_author(change, author)?;
                // TODO: set timestamp to std::cmp::max(entry.timestamp(), datetime)
                // TODO: set urgency to std::cmp::max(entry.urgency(), urgency)
                Ok(entry)
            }
            Some(_entry) => {
                // Create new entry
                let mut builder = self.new_entry();
                builder = builder.maintainer(author);
                if let Some(datetime) = datetime {
                    builder = builder.datetime(datetime);
                }
                if let Some(urgency) = urgency {
                    builder = builder.urgency(urgency);
                }
                for change in change {
                    builder = builder.change_line(change.to_string());
                }
                Ok(builder.finish())
            }
            None => {
                panic!("No existing entries found in changelog");
            }
        }
    }

    /// Automatically add a change to the changelog
    ///
    /// If there is an existing entry, the change will be added to the end of
    /// the entry. If the previous change was attributed to another author,
    /// a new section line ("[ Author Name ]") will be added as well.
    ///
    /// # Deprecated
    ///
    /// This function panics on errors. Use [`ChangeLog::try_auto_add_change`] instead for proper error handling.
    ///
    /// # Panics
    ///
    /// Panics if text rewrapping fails.
    ///
    /// # Arguments
    /// * `change` - The change to add, e.g. &["* Fix a bug"]
    /// * `author` - The author of the change, e.g. ("John Doe", "john@example")
    #[cfg(feature = "chrono")]
    #[deprecated(
        since = "0.2.10",
        note = "Use try_auto_add_change for proper error handling"
    )]
    pub fn auto_add_change(
        &mut self,
        change: &[&str],
        author: (String, String),
        datetime: Option<chrono::DateTime<chrono::FixedOffset>>,
        urgency: Option<Urgency>,
    ) -> Entry {
        self.try_auto_add_change(change, author, datetime, urgency)
            .unwrap()
    }

    /// Pop the first entry from the changelog.
    pub fn pop_first(&mut self) -> Option<Entry> {
        let mut it = self.iter();
        if let Some(entry) = it.next() {
            // Drop trailing newlines
            while let Some(sibling) = entry.0.next_sibling() {
                if sibling.kind() == EMPTY_LINE {
                    sibling.detach();
                } else {
                    break;
                }
            }
            entry.0.detach();
            Some(entry)
        } else {
            None
        }
    }

    /// Read a changelog file from a path
    pub fn read_path(path: impl AsRef<std::path::Path>) -> Result<ChangeLog, Error> {
        let mut file = std::fs::File::open(path)?;
        Self::read(&mut file)
    }

    /// Read a changelog file from a reader
    pub fn read<R: std::io::Read>(mut r: R) -> Result<ChangeLog, Error> {
        let mut buf = String::new();
        r.read_to_string(&mut buf)?;
        Ok(buf.parse()?)
    }

    /// Parse changelog text, returning a ChangeLog, ignoring any errors.
    pub fn parse_relaxed(text: &str) -> ChangeLog {
        let parsed = parse(text);
        let node = SyntaxNode::new_root_mut(parsed.green);
        ChangeLog::cast(node).unwrap_or_else(ChangeLog::new)
    }

    /// Read a changelog file from a reader, allowing for syntax errors
    pub fn read_relaxed<R: std::io::Read>(mut r: R) -> Result<ChangeLog, Error> {
        let mut buf = String::new();
        r.read_to_string(&mut buf)?;

        Ok(Self::parse_relaxed(&buf))
    }

    /// Write the changelog to a writer
    pub fn write<W: std::io::Write>(&self, mut w: W) -> Result<(), Error> {
        let buf = self.to_string();
        w.write_all(buf.as_bytes())?;
        Ok(())
    }

    /// Write the changelog to a path
    pub fn write_to_path(&self, p: &std::path::Path) -> Result<(), Error> {
        let f = std::fs::File::create(p)?;
        self.write(f)?;
        Ok(())
    }

    /// Iterator over entries grouped by their maintainer (author).
    ///
    /// Returns an iterator over tuples of (maintainer_name, maintainer_email, Vec<Entry>)
    /// where entries with the same maintainer are grouped together.
    pub fn iter_by_author(&self) -> impl Iterator<Item = (String, String, Vec<Entry>)> + '_ {
        crate::iter_entries_by_author(self)
    }

    /// Get all unique authors across all entries in the changelog.
    ///
    /// This includes both maintainers from entry footers and authors from [ Author Name ] sections.
    pub fn get_all_authors(&self) -> std::collections::HashSet<crate::Identity> {
        let mut authors = std::collections::HashSet::new();

        // Add maintainers from entry footers
        for entry in self.iter() {
            if let Some(identity) = entry.get_maintainer_identity() {
                authors.insert(identity);
            }
        }

        // Add authors from change sections
        for entry in self.iter() {
            for author_name in entry.get_authors() {
                // Create identity with empty email since we only have names from change sections
                authors.insert(crate::Identity::new(author_name, "".to_string()));
            }
        }

        authors
    }
}

impl Default for ChangeLog {
    fn default() -> Self {
        Self::new()
    }
}

impl FromStr for ChangeLog {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ChangeLog::parse(s).to_mut_result()
    }
}

impl FromStr for Entry {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cl: ChangeLog = s.parse()?;
        let mut entries = cl.iter();
        let entry = entries
            .next()
            .ok_or_else(|| ParseError(vec!["no entries found".to_string()]))?;
        if entries.next().is_some() {
            return Err(ParseError(vec!["multiple entries found".to_string()]));
        }
        Ok(entry)
    }
}

impl EntryHeader {
    /// Returns the version of the entry, returning an error if the version string is invalid.
    ///
    /// Returns:
    /// - `Some(Ok(version))` if a valid version is found
    /// - `Some(Err(err))` if a version token exists but cannot be parsed
    /// - `None` if no version token is present
    pub fn try_version(&self) -> Option<Result<Version, debversion::ParseError>> {
        self.0.children_with_tokens().find_map(|it| {
            if let Some(token) = it.as_token() {
                if token.kind() == VERSION {
                    let text = token.text()[1..token.text().len() - 1].to_string();
                    return Some(text.parse());
                }
            }
            None
        })
    }

    /// Returns the version of the entry.
    ///
    /// Note: This method silently returns `None` if the version string is invalid.
    /// Consider using [`try_version`](Self::try_version) instead to handle parsing errors properly.
    pub fn version(&self) -> Option<Version> {
        self.try_version().and_then(|r| r.ok())
    }

    /// Returns the package name of the entry.
    pub fn package(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|it| {
            if let Some(token) = it.as_token() {
                if token.kind() == IDENTIFIER {
                    return Some(token.text().to_string());
                }
            }
            None
        })
    }

    /// Returns the distributions of the entry.
    pub fn distributions(&self) -> Option<Vec<String>> {
        let node = self.0.children().find(|it| it.kind() == DISTRIBUTIONS);

        node.map(|node| {
            node.children_with_tokens()
                .filter_map(|it| {
                    if let Some(token) = it.as_token() {
                        if token.kind() == IDENTIFIER {
                            return Some(token.text().to_string());
                        }
                    }
                    None
                })
                .collect::<Vec<_>>()
        })
    }

    /// Set distributions for the entry.
    pub fn set_distributions(&mut self, _distributions: Vec<String>) {
        let node = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == DISTRIBUTIONS);
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(DISTRIBUTIONS.into());
        for (i, distribution) in _distributions.iter().enumerate() {
            if i > 0 {
                builder.token(WHITESPACE.into(), " ");
            }
            builder.token(IDENTIFIER.into(), distribution);
        }
        builder.finish_node();

        let (range, green) = if let Some(node) = node {
            (
                node.index()..node.index() + 1,
                vec![builder.finish().into()],
            )
        } else if let Some(version) = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == VERSION)
        {
            (
                version.index()..version.index() + 1,
                vec![
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                    builder.finish().into(),
                ],
            )
        } else if let Some(metadata) = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == METADATA)
        {
            (
                metadata.index() - 1..metadata.index() - 1,
                vec![
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                    builder.finish().into(),
                ],
            )
        } else {
            (
                self.0.children().count()..self.0.children().count(),
                vec![
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                    builder.finish().into(),
                ],
            )
        };

        let new_root = SyntaxNode::new_root_mut(self.0.green().splice_children(range, green));
        self.replace_root(new_root);
    }

    /// Set the version for the entry.
    pub fn set_version(&mut self, version: &Version) {
        // Find the version token
        let node = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == VERSION);
        let (range, green) = if let Some(token) = node {
            (
                token.index()..token.index() + 1,
                vec![GreenToken::new(VERSION.into(), &format!("({})", version)).into()],
            )
        } else {
            let index = self
                .0
                .children_with_tokens()
                .position(|it| it.kind() == IDENTIFIER)
                .unwrap_or(0);
            (
                index + 1..index + 1,
                vec![
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                    GreenToken::new(VERSION.into(), &format!("({})", version)).into(),
                ],
            )
        };
        let new_root = SyntaxNode::new_root_mut(self.0.green().splice_children(range, green));

        self.replace_root(new_root);
    }

    /// Set the package name for the entry.
    pub fn set_package(&mut self, package: String) {
        let node = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == IDENTIFIER);

        let new_root = if let Some(token) = node {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                token.index()..token.index() + 1,
                vec![GreenToken::new(IDENTIFIER.into(), &package).into()],
            ))
        } else {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                0..0,
                vec![
                    GreenToken::new(IDENTIFIER.into(), &package).into(),
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                ],
            ))
        };

        self.replace_root(new_root);
    }

    /// Set extra metadata for the entry.
    pub fn set_metadata(&mut self, key: &str, value: &str) {
        // Find the appropriate metadata node
        if let Some(mut node) = self
            .metadata_nodes()
            .find(|it| it.key().map(|k| k == key).unwrap_or(false))
        {
            node.set_value(value);
        } else if let Some(metadata) = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == METADATA)
        {
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(METADATA_ENTRY.into());
            builder.start_node(METADATA_KEY.into());
            builder.token(IDENTIFIER.into(), key);
            builder.finish_node();
            builder.token(EQUALS.into(), "=");
            builder.start_node(METADATA_VALUE.into());
            builder.token(IDENTIFIER.into(), value);
            builder.finish_node();
            builder.finish_node();

            let metadata = metadata.as_node().unwrap();

            let count = metadata.children_with_tokens().count();
            self.0.splice_children(
                metadata.index()..metadata.index() + 1,
                vec![SyntaxNode::new_root_mut(metadata.green().splice_children(
                    count..count,
                    vec![
                        GreenToken::new(WHITESPACE.into(), " ").into(),
                        builder.finish().into(),
                    ],
                ))
                .into()],
            );
        } else {
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(METADATA.into());
            builder.token(SEMICOLON.into(), ";");
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(METADATA_ENTRY.into());
            builder.start_node(METADATA_KEY.into());
            builder.token(IDENTIFIER.into(), key);
            builder.finish_node();
            builder.token(EQUALS.into(), "=");
            builder.start_node(METADATA_VALUE.into());
            builder.token(IDENTIFIER.into(), value);
            builder.finish_node();
            builder.finish_node();

            let new_root = SyntaxNode::new_root_mut(builder.finish());

            // Add either just after DISTRIBUTIONS
            if let Some(distributions) = self
                .0
                .children_with_tokens()
                .find(|it| it.kind() == DISTRIBUTIONS)
            {
                self.0.splice_children(
                    distributions.index() + 1..distributions.index() + 1,
                    vec![new_root.into()],
                );
            } else if let Some(nl) = self
                .0
                .children_with_tokens()
                .find(|it| it.kind() == NEWLINE)
            {
                // Just before the newline
                self.0
                    .splice_children(nl.index()..nl.index(), vec![new_root.into()]);
            } else {
                let count = self.0.children_with_tokens().count();
                self.0.splice_children(count..count, vec![new_root.into()]);
            }
        }
    }

    /// Returns an iterator over the metadata entry AST nodes.
    pub fn metadata_nodes(&self) -> impl Iterator<Item = MetadataEntry> + '_ {
        let node = self.0.children().find(|it| it.kind() == METADATA);

        node.into_iter().flat_map(|node| {
            node.children_with_tokens()
                .filter_map(|it| MetadataEntry::cast(it.into_node()?))
        })
    }

    /// Returns an iterator over the metadata key-value pairs.
    pub fn metadata(&self) -> impl Iterator<Item = (String, String)> + '_ {
        self.metadata_nodes().filter_map(|entry| {
            if let (Some(key), Some(value)) = (entry.key(), entry.value()) {
                Some((key, value))
            } else {
                None
            }
        })
    }

    /// Returns the urgency of the entry.3
    pub fn urgency(&self) -> Option<Urgency> {
        for (key, value) in self.metadata() {
            if key.as_str() == "urgency" {
                return Some(value.parse().unwrap());
            }
        }
        None
    }
}

impl EntryFooter {
    /// Returns the email address of the maintainer from the footer.
    pub fn email(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|it| {
            if let Some(token) = it.as_token() {
                let text = token.text();
                if token.kind() == EMAIL {
                    return Some(text[1..text.len() - 1].to_string());
                }
            }
            None
        })
    }

    /// Returns the maintainer name from the footer.
    pub fn maintainer(&self) -> Option<String> {
        self.0
            .children()
            .find_map(Maintainer::cast)
            .map(|m| m.text())
            .filter(|s| !s.is_empty())
    }

    /// Set the maintainer for the entry.
    pub fn set_maintainer(&mut self, maintainer: String) {
        let node = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == MAINTAINER);
        let new_root = if let Some(node) = node {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                node.index()..node.index() + 1,
                vec![GreenToken::new(MAINTAINER.into(), &maintainer).into()],
            ))
        } else if let Some(node) = self.0.children_with_tokens().find(|it| it.kind() == INDENT) {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                node.index() + 1..node.index() + 1,
                vec![GreenToken::new(MAINTAINER.into(), &maintainer).into()],
            ))
        } else {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                0..0,
                vec![
                    GreenToken::new(INDENT.into(), " -- ").into(),
                    GreenToken::new(MAINTAINER.into(), &maintainer).into(),
                ],
            ))
        };

        self.replace_root(new_root);
    }

    /// Set email for the entry.
    pub fn set_email(&mut self, _email: String) {
        let node = self.0.children_with_tokens().find(|it| it.kind() == EMAIL);
        let new_root = if let Some(node) = node {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                node.index()..node.index() + 1,
                vec![GreenToken::new(EMAIL.into(), &format!("<{}>", _email)).into()],
            ))
        } else if let Some(node) = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == MAINTAINER)
        {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                node.index() + 1..node.index() + 1,
                vec![GreenToken::new(EMAIL.into(), &format!("<{}>", _email)).into()],
            ))
        } else if let Some(node) = self.0.children_with_tokens().find(|it| it.kind() == INDENT) {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                node.index() + 1..node.index() + 1,
                vec![
                    GreenToken::new(MAINTAINER.into(), "").into(),
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                    GreenToken::new(EMAIL.into(), &format!("<{}>", _email)).into(),
                ],
            ))
        } else {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                0..0,
                vec![
                    GreenToken::new(INDENT.into(), " -- ").into(),
                    GreenToken::new(MAINTAINER.into(), "").into(),
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                    GreenToken::new(EMAIL.into(), &format!("<{}>", _email)).into(),
                ],
            ))
        };

        self.replace_root(new_root);
    }

    /// Returns the timestamp from the footer.
    pub fn timestamp(&self) -> Option<String> {
        self.0
            .children()
            .find_map(Timestamp::cast)
            .map(|m| m.text())
    }

    /// Set timestamp for the entry.
    pub fn set_timestamp(&mut self, timestamp: String) {
        let node = self
            .0
            .children_with_tokens()
            .find(|it| it.kind() == TIMESTAMP);
        let new_root = if let Some(node) = node {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                node.index()..node.index() + 1,
                vec![GreenToken::new(TIMESTAMP.into(), &timestamp).into()],
            ))
        } else if let Some(node) = self.0.children_with_tokens().find(|it| it.kind() == INDENT) {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                node.index() + 1..node.index() + 1,
                vec![GreenToken::new(TIMESTAMP.into(), &timestamp).into()],
            ))
        } else if let Some(node) = self.0.children_with_tokens().find(|it| it.kind() == EMAIL) {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                node.index() + 1..node.index() + 1,
                vec![GreenToken::new(TIMESTAMP.into(), &timestamp).into()],
            ))
        } else {
            SyntaxNode::new_root_mut(self.0.green().splice_children(
                0..0,
                vec![
                    GreenToken::new(INDENT.into(), " -- ").into(),
                    GreenToken::new(TIMESTAMP.into(), &timestamp).into(),
                ],
            ))
        };

        self.replace_root(new_root);
    }
}

impl EntryBody {
    fn text(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|it| {
                if let Some(token) = it.as_token() {
                    if token.kind() == DETAIL {
                        return Some(token.text().to_string());
                    }
                }
                None
            })
            .collect::<Vec<_>>()
            .concat()
    }
}

impl Timestamp {
    fn text(&self) -> String {
        self.0.text().to_string()
    }
}

impl Maintainer {
    fn text(&self) -> String {
        self.0.text().to_string()
    }
}

impl Entry {
    /// Returns the header AST node of the entry.
    pub fn header(&self) -> Option<EntryHeader> {
        self.0.children().find_map(EntryHeader::cast)
    }

    /// Returns the body AST node of the entry.
    pub fn body(&self) -> Option<EntryBody> {
        self.0.children().find_map(EntryBody::cast)
    }

    /// Returns the footer AST node of the entry.
    pub fn footer(&self) -> Option<EntryFooter> {
        self.0.children().find_map(EntryFooter::cast)
    }

    /// Return the package name of the entry.
    pub fn package(&self) -> Option<String> {
        self.header().and_then(|h| h.package())
    }

    /// Set the package name of the entry.
    pub fn set_package(&mut self, package: String) {
        if let Some(mut header) = self.header() {
            let header_index = header.0.index();
            header.set_package(package);
            self.0
                .splice_children(header_index..header_index + 1, vec![header.0.into()]);
        } else {
            self.create_header().set_package(package);
        }
    }

    /// Returns the version of the entry, returning an error if the version string is invalid.
    ///
    /// Returns:
    /// - `Some(Ok(version))` if a valid version is found
    /// - `Some(Err(err))` if a version token exists but cannot be parsed
    /// - `None` if no version token is present or no header exists
    pub fn try_version(&self) -> Option<Result<Version, debversion::ParseError>> {
        self.header().and_then(|h| h.try_version())
    }

    /// Returns the version of the entry.
    ///
    /// Note: This method silently returns `None` if the version string is invalid.
    /// Consider using [`try_version`](Self::try_version) instead to handle parsing errors properly.
    pub fn version(&self) -> Option<Version> {
        self.try_version().and_then(|r| r.ok())
    }

    /// Set the version of the entry.
    pub fn set_version(&mut self, version: &Version) {
        if let Some(mut header) = self.header() {
            let header_index = header.0.index();
            header.set_version(version);
            self.0
                .splice_children(header_index..header_index + 1, vec![header.0.into()]);
        } else {
            self.create_header().set_version(version);
        }
    }

    /// Return the distributions of the entry.
    pub fn distributions(&self) -> Option<Vec<String>> {
        self.header().and_then(|h| h.distributions())
    }

    /// Set the distributions for the entry
    pub fn set_distributions(&mut self, distributions: Vec<String>) {
        if let Some(mut header) = self.header() {
            let header_index = header.0.index();
            header.set_distributions(distributions);
            self.0
                .splice_children(header_index..header_index + 1, vec![header.0.into()]);
        } else {
            self.create_header().set_distributions(distributions);
        }
    }

    /// Returns the email address of the maintainer.
    pub fn email(&self) -> Option<String> {
        self.footer().and_then(|f| f.email())
    }

    /// Returns the maintainer AST node.
    pub fn maintainer_node(&self) -> Option<Maintainer> {
        self.footer()
            .and_then(|f| f.0.children().find_map(Maintainer::cast))
    }

    /// Returns the name of the maintainer.
    pub fn maintainer(&self) -> Option<String> {
        self.footer().and_then(|f| f.maintainer())
    }

    /// Set the maintainer of the entry.
    pub fn set_maintainer(&mut self, maintainer: (String, String)) {
        if let Some(mut footer) = self.footer() {
            let footer_index = footer.0.index();
            footer.set_maintainer(maintainer.0);
            footer.set_email(maintainer.1);
            self.0
                .splice_children(footer_index..footer_index + 1, vec![footer.0.into()]);
        } else {
            let mut footer = self.create_footer();
            footer.set_maintainer(maintainer.0);
            footer.set_email(maintainer.1);
        }
    }

    /// Returns the timestamp AST node.
    pub fn timestamp_node(&self) -> Option<Timestamp> {
        self.footer()
            .and_then(|f| f.0.children().find_map(Timestamp::cast))
    }

    /// Returns the timestamp of the entry, as the raw string.
    pub fn timestamp(&self) -> Option<String> {
        self.footer().and_then(|f| f.timestamp())
    }

    /// Set the timestamp of the entry.
    pub fn set_timestamp(&mut self, timestamp: String) {
        if let Some(mut footer) = self.footer() {
            let footer_index = footer.0.index();
            footer.set_timestamp(timestamp);
            self.0
                .splice_children(footer_index..footer_index + 1, vec![footer.0.into()]);
        } else {
            self.create_footer().set_timestamp(timestamp);
        }
    }

    /// Set the datetime of the entry.
    #[cfg(feature = "chrono")]
    pub fn set_datetime(&mut self, datetime: DateTime<FixedOffset>) {
        self.set_timestamp(format!("{}", datetime.format("%a, %d %b %Y %H:%M:%S %z")));
    }

    /// Returns the datetime of the entry.
    #[cfg(feature = "chrono")]
    pub fn datetime(&self) -> Option<DateTime<FixedOffset>> {
        self.timestamp().and_then(|ts| parse_time_string(&ts).ok())
    }

    /// Returns the urgency of the entry.
    pub fn urgency(&self) -> Option<Urgency> {
        self.header().and_then(|h| h.urgency())
    }

    fn create_header(&self) -> EntryHeader {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ENTRY_HEADER.into());
        builder.token(NEWLINE.into(), "\n");
        builder.finish_node();
        let syntax = SyntaxNode::new_root_mut(builder.finish());
        self.0.splice_children(0..0, vec![syntax.into()]);
        EntryHeader(self.0.children().next().unwrap().clone_for_update())
    }

    fn create_footer(&self) -> EntryFooter {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ENTRY_FOOTER.into());
        builder.token(NEWLINE.into(), "\n");
        builder.finish_node();
        let syntax = SyntaxNode::new_root_mut(builder.finish());
        let count = self.0.children().count();
        self.0.splice_children(count..count, vec![syntax.into()]);
        EntryFooter(self.0.children().last().unwrap().clone_for_update())
    }

    /// Set the urgency of the entry.
    pub fn set_urgency(&mut self, urgency: Urgency) {
        self.set_metadata("urgency", urgency.to_string().as_str());
    }

    /// Set a metadata key-value pair for the entry.
    pub fn set_metadata(&mut self, key: &str, value: &str) {
        if let Some(mut header) = self.header() {
            let header_index = header.0.index();
            header.set_metadata(key, value);
            self.0
                .splice_children(header_index..header_index + 1, vec![header.0.into()]);
        } else {
            self.create_header().set_metadata(key, value);
        }
    }

    /// Add a change for the specified author
    ///
    /// If the author is not the same as the current maintainer, a new
    /// section will be created for the author in the entry (e.g. "[ John Doe ]").
    ///
    /// Returns an error if text rewrapping fails.
    pub fn try_add_change_for_author(
        &self,
        change: &[&str],
        author: (String, String),
    ) -> Result<(), crate::textwrap::Error> {
        let changes_lines = self.change_lines().collect::<Vec<_>>();
        let by_author = crate::changes::changes_by_author(changes_lines.iter().map(|s| s.as_str()))
            .collect::<Vec<_>>();

        // There are no per author sections yet, so attribute current changes to changelog entry author
        if by_author.iter().all(|(a, _, _)| a.is_none()) {
            if let Some(maintainer_name) = self.maintainer() {
                if author.0 != maintainer_name {
                    self.prepend_change_line(
                        crate::changes::format_section_title(maintainer_name.as_str()).as_str(),
                    );
                    if !self.change_lines().last().unwrap().is_empty() {
                        self.append_change_line("");
                    }
                    self.append_change_line(
                        crate::changes::format_section_title(author.0.as_str()).as_str(),
                    );
                }
            }
        } else if let Some(last_section) = by_author.last().as_ref() {
            if last_section.0 != Some(author.0.as_str()) {
                self.append_change_line("");
                self.append_change_line(
                    crate::changes::format_section_title(author.0.as_str()).as_str(),
                );
            }
        }

        if let Some(last) = self.change_lines().last() {
            if last.trim().is_empty() {
                self.pop_change_line();
            }
        }

        for line in crate::textwrap::try_rewrap_changes(change.iter().copied())? {
            self.append_change_line(line.as_ref());
        }
        Ok(())
    }

    /// Add a change for the specified author
    ///
    /// If the author is not the same as the current maintainer, a new
    /// section will be created for the author in the entry (e.g. "[ John Doe ]").
    ///
    /// # Deprecated
    ///
    /// This function panics on errors. Use [`Entry::try_add_change_for_author`] instead for proper error handling.
    ///
    /// # Panics
    ///
    /// Panics if text rewrapping fails.
    #[deprecated(
        since = "0.2.10",
        note = "Use try_add_change_for_author for proper error handling"
    )]
    pub fn add_change_for_author(&self, change: &[&str], author: (String, String)) {
        self.try_add_change_for_author(change, author).unwrap()
    }

    /// Prepend a change line to the entry
    pub fn prepend_change_line(&self, line: &str) {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ENTRY_BODY.into());
        if !line.is_empty() {
            builder.token(INDENT.into(), "  ");
            builder.token(DETAIL.into(), line);
        }
        builder.token(NEWLINE.into(), "\n");
        builder.finish_node();

        // Insert just after the header
        let mut it = self.0.children();
        let header = it.find(|n| n.kind() == ENTRY_HEADER);

        let previous_line = it.find(|n| n.kind() == EMPTY_LINE).or(header);

        let index = previous_line.map_or(0, |l| l.index() + 1);

        let syntax = SyntaxNode::new_root_mut(builder.finish());

        self.0.splice_children(index..index, vec![syntax.into()]);
    }

    /// Pop the last change line from the entry
    pub fn pop_change_line(&self) -> Option<String> {
        // Find the last child of type ENTRY_BODY
        let last_child = self.0.children().filter(|n| n.kind() == ENTRY_BODY).last();

        if let Some(last_child) = last_child {
            let text = last_child.children_with_tokens().find_map(|it| {
                if let Some(token) = it.as_token() {
                    if token.kind() == DETAIL {
                        return Some(token.text().to_string());
                    }
                }
                None
            });
            self.0
                .splice_children(last_child.index()..last_child.index() + 1, vec![]);
            text
        } else {
            None
        }
    }

    /// Append a line to the changelog entry
    pub fn append_change_line(&self, line: &str) {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ENTRY_BODY.into());
        if !line.is_empty() {
            builder.token(INDENT.into(), "  ");
            builder.token(DETAIL.into(), line);
        }
        builder.token(NEWLINE.into(), "\n");
        builder.finish_node();

        // Find the last child of type ENTRY_BODY
        let last_child = self
            .0
            .children()
            .filter(|n| n.kind() == ENTRY_BODY)
            .last()
            .unwrap_or_else(|| {
                // No ENTRY_BODY nodes exist. Insert after the EMPTY_LINE that follows
                // the ENTRY_HEADER (if it exists), to preserve required blank line.
                let children: Vec<_> = self.0.children().collect();
                if children.len() >= 2
                    && children[0].kind() == ENTRY_HEADER
                    && children[1].kind() == EMPTY_LINE
                {
                    children[1].clone()
                } else {
                    children[0].clone()
                }
            });

        let syntax = SyntaxNode::new_root_mut(builder.finish()).into();
        self.0
            .splice_children(last_child.index() + 1..last_child.index() + 1, vec![syntax]);
    }

    /// Add a bullet point to the changelog entry.
    ///
    /// This is a convenience method that appends a bullet point line to the entry.
    /// Always prepends "* " to the text and wraps the text to 78 columns if needed.
    ///
    /// # Arguments
    /// * `text` - The text of the bullet point (without the "* " prefix)
    ///
    /// # Examples
    /// ```
    /// use debian_changelog::ChangeLog;
    ///
    /// let mut changelog = ChangeLog::new();
    /// let entry = changelog.new_entry()
    ///     .maintainer(("Author".into(), "author@example.com".into()))
    ///     .distribution("unstable".to_string())
    ///     .version("1.0.0".parse().unwrap())
    ///     .finish();
    ///
    /// entry.add_bullet("First change");
    /// entry.add_bullet("Second change");
    ///
    /// let lines: Vec<_> = entry.change_lines().collect();
    /// assert_eq!(lines[0], "* First change");
    /// assert_eq!(lines[1], "* Second change");
    /// ```
    pub fn add_bullet(&self, text: &str) {
        // Wrap the text with "* " prefix
        let wrapped = crate::textwrap::textwrap(
            text,
            Some(crate::textwrap::DEFAULT_WIDTH),
            Some(crate::textwrap::INITIAL_INDENT),
            Some("  "),
        );

        // Append each wrapped line
        for line in wrapped {
            self.append_change_line(&line);
        }
    }

    /// Returns the changes of the entry.
    pub fn change_lines(&self) -> impl Iterator<Item = String> + '_ {
        let mut lines = self
            .0
            .children()
            .filter_map(|n| {
                if let Some(ref change) = EntryBody::cast(n.clone()) {
                    Some(change.text())
                } else if n.kind() == EMPTY_LINE {
                    Some("".to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        while let Some(last) = lines.last() {
            if last.is_empty() {
                lines.pop();
            } else {
                break;
            }
        }

        lines.into_iter().skip_while(|it| it.is_empty())
    }

    /// Ensure that the first line of the entry is the specified line
    ///
    /// If the first line is not the specified line, it will be prepended to the entry.
    pub fn ensure_first_line(&self, line: &str) {
        let first_line = self.change_lines().next().map(|it| it.trim().to_string());

        if first_line != Some(line.to_string()) {
            self.prepend_change_line(line);
        }
    }

    /// Return whether the entry is marked as being unreleased
    pub fn is_unreleased(&self) -> Option<bool> {
        let distro_is_unreleased = self.distributions().as_ref().map(|ds| {
            let ds = ds.iter().map(|d| d.as_str()).collect::<Vec<&str>>();
            crate::distributions_is_unreleased(ds.as_slice())
        });

        let footer_is_unreleased = if self.maintainer().is_none() && self.email().is_none() {
            Some(true)
        } else {
            None
        };

        match (distro_is_unreleased, footer_is_unreleased) {
            (Some(true), _) => Some(true),
            (_, Some(true)) => Some(true),
            (Some(false), _) => Some(false),
            (_, Some(false)) => Some(false),
            _ => None,
        }
    }

    /// Iterator over changes in this entry grouped by author.
    ///
    /// Returns a vector of tuples (author_name, line_numbers, change_lines)
    /// where author_name is Some for attributed changes or None for changes without attribution.
    pub fn iter_changes_by_author(&self) -> Vec<(Option<String>, Vec<usize>, Vec<String>)> {
        let changes: Vec<String> = self.change_lines().map(|s| s.to_string()).collect();
        crate::changes::changes_by_author(changes.iter().map(|s| s.as_str()))
            .map(|(author, linenos, lines)| {
                let author_name = author.map(|s| s.to_string());
                let change_lines = lines.into_iter().map(|s| s.to_string()).collect();
                (author_name, linenos, change_lines)
            })
            .collect()
    }

    /// Get all authors mentioned in this entry's changes.
    ///
    /// This includes authors from [ Author Name ] sections in the change text,
    /// but not the main maintainer/uploader from the entry footer.
    pub fn get_authors(&self) -> std::collections::HashSet<String> {
        let changes: Vec<String> = self.change_lines().map(|s| s.to_string()).collect();
        let change_strs: Vec<&str> = changes.iter().map(|s| s.as_str()).collect();
        crate::changes::find_extra_authors(&change_strs)
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Get the maintainer information as an Identity struct.
    ///
    /// Returns the maintainer name and email from the entry footer if available.
    pub fn get_maintainer_identity(&self) -> Option<crate::Identity> {
        if let (Some(name), Some(email)) = (self.maintainer(), self.email()) {
            Some(crate::Identity::new(name, email))
        } else {
            None
        }
    }

    /// Add changes for a specific author to this entry.
    ///
    /// This will add an author section (e.g., `[ Author Name ]`) if needed,
    /// and append the changes under that section. If this is the first attributed
    /// change and there are existing unattributed changes, they will be wrapped
    /// in the maintainer's section.
    ///
    /// # Arguments
    /// * `author_name` - The name of the author to attribute the changes to
    /// * `changes` - A list of change lines to add (e.g., `["* Fixed bug"]`)
    ///
    /// # Example
    /// ```
    /// use debian_changelog::Entry;
    /// let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low
    ///
    ///   * Existing change
    ///
    ///  -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
    /// "#.parse().unwrap();
    ///
    /// entry.try_add_changes_for_author("Alice", vec!["* New feature"]);
    /// ```
    pub fn try_add_changes_for_author(
        &self,
        author_name: &str,
        changes: Vec<&str>,
    ) -> Result<(), crate::textwrap::Error> {
        let mut change_lines: Vec<String> = self.change_lines().collect();
        let original_len = change_lines.len();
        let default_author = self.get_maintainer_identity().map(|id| (id.name, id.email));

        crate::changes::try_add_change_for_author(
            &mut change_lines,
            author_name,
            changes,
            default_author,
        )?;

        // The function modifies change_lines in place. We need to handle two cases:
        // 1. Lines were inserted at the beginning (when wrapping existing changes)
        // 2. Lines were appended at the end (normal case)

        if change_lines.len() > original_len {
            // New lines were added
            let original_changes: Vec<_> = self.change_lines().collect();

            // Check if lines were inserted at the start
            let inserted_at_start = original_len > 0 && change_lines[0] != original_changes[0];

            if inserted_at_start {
                // Lines were inserted at the beginning - we need to rebuild
                // This happens when converting unattributed changes to attributed ones
                while self.pop_change_line().is_some() {}
                for line in change_lines {
                    self.append_change_line(&line);
                }
            } else {
                // Lines were appended at the end - just append the new ones
                for line in change_lines.iter().skip(original_len) {
                    self.append_change_line(line);
                }
            }
        }
        Ok(())
    }

    /// Add changes for the specified author
    ///
    /// # Deprecated
    ///
    /// This function panics on errors. Use [`Entry::try_add_changes_for_author`] instead for proper error handling.
    ///
    /// # Panics
    ///
    /// Panics if text rewrapping fails.
    #[deprecated(
        since = "0.2.10",
        note = "Use try_add_changes_for_author for proper error handling"
    )]
    pub fn add_changes_for_author(&self, author_name: &str, changes: Vec<&str>) {
        self.try_add_changes_for_author(author_name, changes)
            .unwrap()
    }

    /// Find the bug reference at the given absolute byte offset.
    ///
    /// Returns `Some(Bug)` when the cursor sits on a bug number inside a
    /// `Closes: #...` or `LP: #...` pattern in a detail line.
    ///
    /// # Example
    ///
    /// ```
    /// use debian_changelog::changes::Bug;
    /// use rowan::ast::AstNode;
    ///
    /// let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Fix. (Closes: #42)\n\n -- T <t@t>  Mon, 01 Jan 2024 00:00:00 +0000\n";
    /// let parsed = debian_changelog::ChangeLog::parse(text);
    /// let changelog = parsed.tree();
    /// let offset = rowan::TextSize::from(text.find("#42").unwrap() as u32 + 1);
    /// let entry = changelog.entry_at_offset(offset).unwrap();
    /// assert_eq!(entry.bug_at_offset(offset), Some(Bug::Debian(42)));
    /// ```
    pub fn bug_at_offset(&self, offset: rowan::TextSize) -> Option<crate::bugs::Bug> {
        let detail = self.detail_token_at_offset(offset)?;
        let start: usize = detail.text_range().start().into();
        crate::bugs::bug_at_offset(detail.text(), start, offset.into())
    }

    /// Find the bug number prefix being typed at the given absolute byte offset.
    ///
    /// Returns the bug tracker variant and the digit prefix typed so far,
    /// useful for completion.  Returns `None` when the cursor is not in a
    /// bug-reference context.
    pub fn bug_prefix_at_offset(
        &self,
        offset: rowan::TextSize,
    ) -> Option<(crate::bugs::BugTracker, String)> {
        let detail = self.detail_token_at_offset(offset)?;
        let start: usize = detail.text_range().start().into();
        crate::bugs::bug_prefix_at_offset(detail.text(), start, offset.into())
    }

    /// Find the DETAIL token at the given offset within this entry.
    fn detail_token_at_offset(&self, offset: rowan::TextSize) -> Option<SyntaxToken> {
        let token = match self.syntax().token_at_offset(offset) {
            rowan::TokenAtOffset::Single(token) => Some(token),
            rowan::TokenAtOffset::Between(left, right) => {
                if left.kind() == DETAIL {
                    Some(left)
                } else if right.kind() == DETAIL {
                    Some(right)
                } else {
                    None
                }
            }
            rowan::TokenAtOffset::None => None,
        }?;
        if token.kind() == DETAIL {
            Some(token)
        } else {
            None
        }
    }
}

#[cfg(feature = "chrono")]
const CHANGELOG_TIME_FORMAT: &str = "%a, %d %b %Y %H:%M:%S %z";

#[cfg(feature = "chrono")]
fn parse_time_string(time_str: &str) -> Result<DateTime<FixedOffset>, chrono::ParseError> {
    // First try parsing with day-of-week validation
    if let Ok(dt) = DateTime::parse_from_str(time_str, CHANGELOG_TIME_FORMAT) {
        return Ok(dt);
    }

    // If that fails, try parsing without day-of-week validation
    // This is more lenient for changelogs that have incorrect day-of-week values
    // Skip the day name (everything before the first comma and space)
    if let Some(after_comma) = time_str.split_once(", ") {
        DateTime::parse_from_str(after_comma.1, "%d %b %Y %H:%M:%S %z")
    } else {
        // If there's no comma, return the original error
        DateTime::parse_from_str(time_str, CHANGELOG_TIME_FORMAT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        const CHANGELOG: &str = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500

breezy (3.3.3-2) unstable; urgency=medium

  * Drop unnecessary dependency on python3-six. Closes: #1039011
  * Drop dependency on cython3-dbg. Closes: #1040544

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 24 Jun 2023 14:58:57 +0100

# Oh, and here is a comment
"#;
        let parsed = parse(CHANGELOG);
        assert!(parsed.errors().is_empty());
        let node = parsed.syntax_node();
        assert_eq!(
            format!("{:#?}", node),
            r###"ROOT@0..405
  ENTRY@0..140
    ENTRY_HEADER@0..39
      IDENTIFIER@0..6 "breezy"
      WHITESPACE@6..7 " "
      VERSION@7..16 "(3.3.4-1)"
      WHITESPACE@16..17 " "
      DISTRIBUTIONS@17..25
        IDENTIFIER@17..25 "unstable"
      METADATA@25..38
        SEMICOLON@25..26 ";"
        WHITESPACE@26..27 " "
        METADATA_ENTRY@27..38
          METADATA_KEY@27..34
            IDENTIFIER@27..34 "urgency"
          EQUALS@34..35 "="
          METADATA_VALUE@35..38
            IDENTIFIER@35..38 "low"
      NEWLINE@38..39 "\n"
    EMPTY_LINE@39..40
      NEWLINE@39..40 "\n"
    ENTRY_BODY@40..66
      INDENT@40..42 "  "
      DETAIL@42..65 "* New upstream release."
      NEWLINE@65..66 "\n"
    EMPTY_LINE@66..67
      NEWLINE@66..67 "\n"
    ENTRY_FOOTER@67..140
      INDENT@67..71 " -- "
      MAINTAINER@71..86
        TEXT@71..77 "Jelmer"
        WHITESPACE@77..78 " "
        TEXT@78..86 "Vernooĳ"
      WHITESPACE@86..87 " "
      EMAIL@87..106 "<jelmer@debian.org>"
      WHITESPACE@106..108 "  "
      TIMESTAMP@108..139
        TEXT@108..112 "Mon,"
        WHITESPACE@112..113 " "
        TEXT@113..115 "04"
        WHITESPACE@115..116 " "
        TEXT@116..119 "Sep"
        WHITESPACE@119..120 " "
        TEXT@120..124 "2023"
        WHITESPACE@124..125 " "
        TEXT@125..133 "18:13:45"
        WHITESPACE@133..134 " "
        TEXT@134..139 "-0500"
      NEWLINE@139..140 "\n"
  EMPTY_LINE@140..141
    NEWLINE@140..141 "\n"
  ENTRY@141..376
    ENTRY_HEADER@141..183
      IDENTIFIER@141..147 "breezy"
      WHITESPACE@147..148 " "
      VERSION@148..157 "(3.3.3-2)"
      WHITESPACE@157..158 " "
      DISTRIBUTIONS@158..166
        IDENTIFIER@158..166 "unstable"
      METADATA@166..182
        SEMICOLON@166..167 ";"
        WHITESPACE@167..168 " "
        METADATA_ENTRY@168..182
          METADATA_KEY@168..175
            IDENTIFIER@168..175 "urgency"
          EQUALS@175..176 "="
          METADATA_VALUE@176..182
            IDENTIFIER@176..182 "medium"
      NEWLINE@182..183 "\n"
    EMPTY_LINE@183..184
      NEWLINE@183..184 "\n"
    ENTRY_BODY@184..249
      INDENT@184..186 "  "
      DETAIL@186..248 "* Drop unnecessary de ..."
      NEWLINE@248..249 "\n"
    ENTRY_BODY@249..302
      INDENT@249..251 "  "
      DETAIL@251..301 "* Drop dependency on  ..."
      NEWLINE@301..302 "\n"
    EMPTY_LINE@302..303
      NEWLINE@302..303 "\n"
    ENTRY_FOOTER@303..376
      INDENT@303..307 " -- "
      MAINTAINER@307..322
        TEXT@307..313 "Jelmer"
        WHITESPACE@313..314 " "
        TEXT@314..322 "Vernooĳ"
      WHITESPACE@322..323 " "
      EMAIL@323..342 "<jelmer@debian.org>"
      WHITESPACE@342..344 "  "
      TIMESTAMP@344..375
        TEXT@344..348 "Sat,"
        WHITESPACE@348..349 " "
        TEXT@349..351 "24"
        WHITESPACE@351..352 " "
        TEXT@352..355 "Jun"
        WHITESPACE@355..356 " "
        TEXT@356..360 "2023"
        WHITESPACE@360..361 " "
        TEXT@361..369 "14:58:57"
        WHITESPACE@369..370 " "
        TEXT@370..375 "+0100"
      NEWLINE@375..376 "\n"
  EMPTY_LINE@376..377
    NEWLINE@376..377 "\n"
  COMMENT@377..405 "# Oh, and here is a c ..."
"###
        );

        let mut root = parsed.tree_mut();
        let entries: Vec<_> = root.iter().collect();
        assert_eq!(entries.len(), 2);
        let entry = &entries[0];
        assert_eq!(entry.package(), Some("breezy".into()));
        assert_eq!(entry.version(), Some("3.3.4-1".parse().unwrap()));
        assert_eq!(entry.distributions(), Some(vec!["unstable".into()]));
        assert_eq!(entry.urgency(), Some(Urgency::Low));
        assert_eq!(entry.maintainer(), Some("Jelmer Vernooĳ".into()));
        assert_eq!(entry.email(), Some("jelmer@debian.org".into()));
        assert_eq!(
            entry.timestamp(),
            Some("Mon, 04 Sep 2023 18:13:45 -0500".into())
        );
        #[cfg(feature = "chrono")]
        assert_eq!(
            entry.datetime(),
            Some("2023-09-04T18:13:45-05:00".parse().unwrap())
        );
        let changes_lines: Vec<_> = entry.change_lines().collect();
        assert_eq!(changes_lines, vec!["* New upstream release.".to_string()]);

        assert_eq!(node.text(), CHANGELOG);

        let first = root.pop_first().unwrap();
        assert_eq!(first.version(), Some("3.3.4-1".parse().unwrap()));
        assert_eq!(
            root.to_string(),
            r#"breezy (3.3.3-2) unstable; urgency=medium

  * Drop unnecessary dependency on python3-six. Closes: #1039011
  * Drop dependency on cython3-dbg. Closes: #1040544

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 24 Jun 2023 14:58:57 +0100

# Oh, and here is a comment
"#
        );
    }

    #[test]
    fn test_from_io_read() {
        let changelog = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#;

        let input = changelog.as_bytes();
        let input = Box::new(std::io::Cursor::new(input)) as Box<dyn std::io::Read>;
        let parsed = ChangeLog::read(input).unwrap();
        assert_eq!(parsed.to_string(), changelog);
    }

    #[test]
    #[cfg(feature = "chrono")]
    fn test_new_entry() {
        let mut cl = ChangeLog::new();
        cl.new_entry()
            .package("breezy".into())
            .version("3.3.4-1".parse().unwrap())
            .distributions(vec!["unstable".into()])
            .urgency(Urgency::Low)
            .maintainer(("Jelmer Vernooĳ".into(), "jelmer@debian.org".into()))
            .change_line("* A change.".into())
            .datetime("Mon, 04 Sep 2023 18:13:45 -0500")
            .finish();
        assert_eq!(
            r###"breezy (3.3.4-1) unstable; urgency=low

  * A change.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"###,
            cl.to_string()
        );

        assert!(!cl.iter().next().unwrap().is_unreleased().unwrap());
    }

    #[test]
    #[cfg(feature = "chrono")]
    fn test_new_empty_default() {
        let mut cl = ChangeLog::new();
        cl.new_entry()
            .package("breezy".into())
            .version("3.3.4-1".parse().unwrap())
            .maintainer(("Jelmer Vernooĳ".into(), "jelmer@debian.org".into()))
            .change_line("* A change.".into())
            .datetime("Mon, 04 Sep 2023 18:13:45 -0500")
            .finish();
        assert_eq!(
            r###"breezy (3.3.4-1) UNRELEASED; urgency=medium

  * A change.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"###,
            cl.to_string()
        );
    }

    #[test]
    fn test_new_empty_entry() {
        let mut cl = ChangeLog::new();
        cl.new_empty_entry()
            .change_line("* A change.".into())
            .finish();
        assert_eq!(
            r###"

  * A change.

 -- 
"###,
            cl.to_string()
        );
        assert_eq!(cl.iter().next().unwrap().is_unreleased(), Some(true));
    }

    #[test]
    fn test_parse_invalid_line() {
        let text = r#"THIS IS NOT A PARSEABLE LINE
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
"#;
        let cl = ChangeLog::read_relaxed(text.as_bytes()).unwrap();
        let entry = cl.iter().nth(1).unwrap();
        assert_eq!(entry.package(), Some("lintian-brush".into()));
        assert_eq!(entry.version(), Some("0.35".parse().unwrap()));
        assert_eq!(entry.urgency(), Some(Urgency::Medium));
        assert_eq!(entry.maintainer(), Some("Joe Example".into()));
        assert_eq!(entry.email(), Some("joe@example.com".into()));
        assert_eq!(entry.distributions(), Some(vec!["UNRELEASED".into()]));
        #[cfg(feature = "chrono")]
        assert_eq!(
            entry.datetime(),
            Some("2019-10-04T02:36:13+00:00".parse().unwrap())
        );
    }

    #[cfg(test)]
    mod entry_manipulate_tests {
        use super::*;

        #[test]
        fn test_append_change_line() {
            let mut cl = ChangeLog::new();

            let entry = cl
                .new_empty_entry()
                .change_line("* A change.".into())
                .finish();

            entry.append_change_line("* Another change.");

            assert_eq!(
                r###"

  * A change.
  * Another change.

 -- 
"###,
                cl.to_string()
            );
        }

        #[test]
        fn test_prepend_change_line() {
            let mut cl = ChangeLog::new();

            let entry = cl
                .new_empty_entry()
                .change_line("* A change.".into())
                .finish();

            entry.prepend_change_line("* Another change.");

            assert_eq!(
                r###"

  * Another change.
  * A change.

 -- 
"###,
                cl.to_string()
            );

            assert_eq!(entry.maintainer(), None);
            assert_eq!(entry.email(), None);
            assert_eq!(entry.timestamp(), None);
            assert_eq!(entry.package(), None);
            assert_eq!(entry.version(), None);
        }
    }

    #[cfg(test)]
    mod auto_add_change_tests {
        #[test]
        fn test_unreleased_existing() {
            let text = r#"lintian-brush (0.35) unstable; urgency=medium

  * This line already existed.

  [ Jane Example ]
  * And this one has an existing author.

 -- 
"#;
            let mut cl = super::ChangeLog::read(text.as_bytes()).unwrap();

            let entry = cl.iter().next().unwrap();
            assert_eq!(entry.package(), Some("lintian-brush".into()));
            assert_eq!(entry.is_unreleased(), Some(true));

            let entry = cl
                .try_auto_add_change(
                    &["* And this one is new."],
                    ("Joe Example".to_string(), "joe@example.com".to_string()),
                    None::<String>,
                    None,
                )
                .unwrap();

            assert_eq!(cl.iter().count(), 1);

            assert_eq!(entry.package(), Some("lintian-brush".into()));
            assert_eq!(entry.is_unreleased(), Some(true));
            assert_eq!(
                entry.change_lines().collect::<Vec<_>>(),
                &[
                    "* This line already existed.",
                    "",
                    "[ Jane Example ]",
                    "* And this one has an existing author.",
                    "",
                    "[ Joe Example ]",
                    "* And this one is new.",
                ]
            );
        }
    }

    #[test]
    fn test_ensure_first_line() {
        let text = r#"lintian-brush (0.35) unstable; urgency=medium

  * This line already existed.

  [ Jane Example ]
  * And this one has an existing author.

 -- 
"#;
        let cl = ChangeLog::read(text.as_bytes()).unwrap();

        let entry = cl.iter().next().unwrap();
        assert_eq!(entry.package(), Some("lintian-brush".into()));

        entry.ensure_first_line("* QA upload.");
        entry.ensure_first_line("* QA upload.");

        assert_eq!(
            r#"lintian-brush (0.35) unstable; urgency=medium

  * QA upload.
  * This line already existed.

  [ Jane Example ]
  * And this one has an existing author.

 -- 
"#,
            cl.to_string()
        );
    }

    #[test]
    fn test_set_version() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_version(&"3.3.5-1".parse().unwrap());

        assert_eq!(
            r#"breezy (3.3.5-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    fn test_set_package() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_package("bzr".into());

        assert_eq!(
            r#"bzr (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    fn test_set_distributions() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_distributions(vec!["unstable".into(), "experimental".into()]);

        assert_eq!(
            r#"breezy (3.3.4-1) unstable experimental; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    fn test_set_distributions_no_existing() {
        let mut entry: Entry = r#"breezy (3.3.4-1); urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_distributions(vec!["unstable".into()]);

        assert!(entry.to_string().contains("unstable"));
    }

    #[test]
    fn test_set_maintainer() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_maintainer(("Joe Example".into(), "joe@example.com".into()));

        assert_eq!(
            r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Joe Example <joe@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    fn test_set_timestamp() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_timestamp("Mon, 04 Sep 2023 18:13:46 -0500".into());

        assert_eq!(
            r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:46 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    #[cfg(feature = "chrono")]
    fn test_set_datetime() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <joe@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_datetime("2023-09-04T18:13:46-05:00".parse().unwrap());

        assert_eq!(
            r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <joe@example.com>  Mon, 04 Sep 2023 18:13:46 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    fn test_set_urgency() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_urgency(Urgency::Medium);

        assert_eq!(
            r#"breezy (3.3.4-1) unstable; urgency=medium

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    fn test_set_metadata() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_metadata("foo", "bar");

        assert_eq!(
            r#"breezy (3.3.4-1) unstable; urgency=low foo=bar

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    fn test_set_metadata_replace_existing() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low foo=old

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_metadata("foo", "new");

        assert_eq!(
            r#"breezy (3.3.4-1) unstable; urgency=low foo=new

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    fn test_set_metadata_after_distributions() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable experimental; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_metadata("foo", "bar");

        assert_eq!(
            r#"breezy (3.3.4-1) unstable experimental; urgency=low foo=bar

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#,
            entry.to_string()
        );
    }

    #[test]
    fn test_add_change_for_author() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

  [ Jelmer Vernooĳ ]
  * A change by the maintainer.

 -- Joe Example <joe@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry
            .try_add_change_for_author(
                &["A change by the maintainer."],
                ("Jelmer Vernooĳ".into(), "jelmer@debian.org".into()),
            )
            .unwrap();
    }

    #[test]
    fn test_changelog_from_entry_iter() {
        let text = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@jelmer.uk>  Mon, 04 Sep 2023 18:13:45 -0500
"#;

        let entry: Entry = text.parse().unwrap();

        let cl = std::iter::once(entry).collect::<ChangeLog>();

        assert_eq!(cl.to_string(), text);
    }

    #[test]
    fn test_pop_change_line() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.
  * Fixed bug #123.
  * Added new feature.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        // Test popping existing lines
        assert_eq!(
            entry.pop_change_line(),
            Some("* Added new feature.".to_string())
        );
        assert_eq!(
            entry.pop_change_line(),
            Some("* Fixed bug #123.".to_string())
        );
        assert_eq!(
            entry.pop_change_line(),
            Some("* New upstream release.".to_string())
        );

        // Test popping from empty entry
        assert_eq!(entry.pop_change_line(), None);
    }

    #[test]
    fn test_pop_change_line_empty_entry() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        assert_eq!(entry.pop_change_line(), None);
    }

    #[test]
    fn test_pop_change_line_empty_string() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * Something

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.pop_change_line();
        entry.append_change_line("");
        // Empty lines don't have DETAIL tokens, so pop_change_line returns None
        assert_eq!(entry.pop_change_line(), None);
    }

    #[test]
    fn test_append_change_line() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.append_change_line("* Fixed bug #456.");

        assert_eq!(
            entry.to_string(),
            r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.
  * Fixed bug #456.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        );
    }

    #[test]
    fn test_append_change_line_empty() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.append_change_line("");

        let lines: Vec<String> = entry.change_lines().collect();
        // Empty lines are not returned by change_lines()
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "* New upstream release.".to_string());
    }

    #[test]
    fn test_changelog_write_to_path() {
        use tempfile::NamedTempFile;

        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        changelog.write_to_path(&path).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, changelog.to_string());
    }

    #[test]
    fn test_changelog_into_iter() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500

breezy (3.3.3-1) unstable; urgency=low

  * Previous release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 03 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let entries: Vec<Entry> = changelog.into_iter().collect();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_set_version_no_existing() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry.set_version(&"1.0.0".parse().unwrap());

        assert!(entry.to_string().contains("(1.0.0)"));
    }

    #[test]
    fn test_entry_footer_set_email_edge_cases() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        // Test checking email through entry
        assert_eq!(entry.email(), Some("jelmer@debian.org".to_string()));
    }

    #[test]
    fn test_entry_footer_set_maintainer_edge_cases() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        // Test setting maintainer
        entry.set_maintainer(("New Maintainer".into(), "new@example.com".into()));

        assert!(entry
            .to_string()
            .contains("New Maintainer <new@example.com>"));
    }

    #[test]
    fn test_entry_footer_set_timestamp_edge_cases() {
        let mut entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  
"#
        .parse()
        .unwrap();

        // Test setting timestamp when it's missing
        entry.set_timestamp("Mon, 04 Sep 2023 18:13:45 -0500".into());

        assert!(entry
            .to_string()
            .contains("Mon, 04 Sep 2023 18:13:45 -0500"));
    }

    #[test]
    fn test_parse_multiple_distributions_frozen_unstable() {
        // Test case for https://github.com/jelmer/debian-changelog-rs/issues/93
        // The "at" package has entries with "frozen unstable" distributions from 1998
        const CHANGELOG: &str = r#"at (3.1.8-10) frozen unstable; urgency=high

  * Suidunregister /usr/bin (closes: Bug#59421).

 -- Siggy Brentrup <bsb@winnegan.de>  Mon,  3 Apr 2000 13:56:47 +0200
"#;

        let parsed = parse(CHANGELOG);
        assert!(parsed.errors().is_empty());

        let root = parsed.tree();
        let entries: Vec<_> = root.iter().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.package(), Some("at".into()));
        assert_eq!(entry.version(), Some("3.1.8-10".parse().unwrap()));
        assert_eq!(
            entry.distributions(),
            Some(vec!["frozen".into(), "unstable".into()])
        );
    }

    #[test]
    fn test_parse_old_metadata_format_with_comma() {
        // Test case for https://github.com/jelmer/debian-changelog-rs/issues/93
        // The "at" package has old-style metadata with comma-separated values
        const CHANGELOG: &str = r#"at (3.1.8-9) frozen unstable; urgency=low, closes=53715 56047 56607 55560 55514

  * Added SIGCHLD handler to release zombies (closes 53715 56047 56607)

 -- Siggy Brentrup <bsb@winnegan.de>  Sun, 30 Jan 2000 22:00:46 +0100
"#;

        let parsed = parse(CHANGELOG);

        // This old format currently fails to parse
        if !parsed.errors().is_empty() {
            eprintln!("Parse errors: {:?}", parsed.errors());
        }
        assert!(parsed.errors().is_empty());

        let root = parsed.tree();
        let entries: Vec<_> = root.iter().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.package(), Some("at".into()));
        assert_eq!(entry.version(), Some("3.1.8-9".parse().unwrap()));
        assert_eq!(
            entry.distributions(),
            Some(vec!["frozen".into(), "unstable".into()])
        );
        assert_eq!(entry.urgency(), Some(Urgency::Low));

        // Verify we can access the "closes" metadata
        let header = entry.header().unwrap();
        let metadata: Vec<(String, String)> = header.metadata().collect();

        // Should have both urgency and closes
        assert_eq!(metadata.len(), 2);
        assert!(metadata.iter().any(|(k, v)| k == "urgency" && v == "low"));

        // Get the closes value and verify exact match
        let closes_value = metadata
            .iter()
            .find(|(k, _)| k == "closes")
            .map(|(_, v)| v)
            .expect("closes metadata should exist");

        assert_eq!(closes_value, "53715 56047 56607 55560 55514");
    }

    #[test]
    fn test_entry_iter_changes_by_author() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Author 1 ]
  * Change by Author 1

  [ Author 2 ]  
  * Change by Author 2
  * Another change by Author 2

  * Unattributed change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let changes = entry.iter_changes_by_author();

        assert_eq!(changes.len(), 3);

        assert_eq!(changes[0].0, Some("Author 1".to_string()));
        assert_eq!(changes[0].2, vec!["* Change by Author 1".to_string()]);

        assert_eq!(changes[1].0, Some("Author 2".to_string()));
        assert_eq!(
            changes[1].2,
            vec![
                "* Change by Author 2".to_string(),
                "* Another change by Author 2".to_string()
            ]
        );

        assert_eq!(changes[2].0, None);
        assert_eq!(changes[2].2, vec!["* Unattributed change".to_string()]);
    }

    #[test]
    fn test_entry_get_authors() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Author 1 ]
  * Change by Author 1

  [ Author 2 ]  
  * Change by Author 2

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let authors = entry.get_authors();

        assert_eq!(authors.len(), 2);
        assert!(authors.contains("Author 1"));
        assert!(authors.contains("Author 2"));
        // Maintainer should not be in the authors from change sections
        assert!(!authors.contains("Jelmer Vernooĳ"));
    }

    #[test]
    fn test_entry_get_maintainer_identity() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let identity = entry.get_maintainer_identity().unwrap();

        assert_eq!(identity.name, "Jelmer Vernooĳ");
        assert_eq!(identity.email, "jelmer@debian.org");
    }

    #[test]
    fn test_entry_get_maintainer_identity_missing() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

"#
        .parse()
        .unwrap();

        let identity = entry.get_maintainer_identity();

        assert!(identity.is_none());
    }

    #[test]
    fn test_changelog_iter_by_author() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500

breezy (3.3.3-1) unstable; urgency=low

  * Bug fix release.

 -- Jane Doe <jane@example.com>  Sun, 03 Sep 2023 17:12:30 -0500

breezy (3.3.2-1) unstable; urgency=low

  * Another release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 02 Sep 2023 16:11:15 -0500
"#
        .parse()
        .unwrap();

        let authors: Vec<(String, String, Vec<Entry>)> = changelog.iter_by_author().collect();

        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0].0, "Jane Doe");
        assert_eq!(authors[0].1, "jane@example.com");
        assert_eq!(authors[0].2.len(), 1);
        assert_eq!(authors[1].0, "Jelmer Vernooĳ");
        assert_eq!(authors[1].1, "jelmer@debian.org");
        assert_eq!(authors[1].2.len(), 2);
    }

    #[test]
    fn test_changelog_get_all_authors() {
        let changelog: ChangeLog = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Contributor 1 ]
  * Contribution

  * Main change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500

breezy (3.3.3-1) unstable; urgency=low

  * Bug fix release.

 -- Jane Doe <jane@example.com>  Sun, 03 Sep 2023 17:12:30 -0500
"#
        .parse()
        .unwrap();

        let authors = changelog.get_all_authors();

        assert_eq!(authors.len(), 3);

        let author_names: std::collections::HashSet<String> = authors
            .iter()
            .map(|identity| identity.name.clone())
            .collect();

        assert!(author_names.contains("Jelmer Vernooĳ"));
        assert!(author_names.contains("Jane Doe"));
        assert!(author_names.contains("Contributor 1"));
    }

    #[test]
    fn test_add_changes_for_author_no_existing_sections() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  * Existing change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry
            .try_add_changes_for_author("Alice", vec!["* Alice's change"])
            .unwrap();

        let lines: Vec<_> = entry.change_lines().collect();

        // Should have wrapped existing changes in maintainer's section
        assert!(lines.iter().any(|l| l.contains("[ Jelmer Vernooĳ ]")));
        // Should have added Alice's section
        assert!(lines.iter().any(|l| l.contains("[ Alice ]")));
        // Should have both changes
        assert!(lines.iter().any(|l| l.contains("Existing change")));
        assert!(lines.iter().any(|l| l.contains("Alice's change")));
    }

    #[test]
    fn test_add_changes_for_author_with_existing_sections() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Author 1 ]
  * Change by Author 1

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry
            .try_add_changes_for_author("Alice", vec!["* Alice's new change"])
            .unwrap();

        let lines: Vec<_> = entry.change_lines().collect();

        // Should have Author 1's section
        assert!(lines.iter().any(|l| l.contains("[ Author 1 ]")));
        // Should have added Alice's section
        assert!(lines.iter().any(|l| l.contains("[ Alice ]")));
        // Should have both changes
        assert!(lines.iter().any(|l| l.contains("Change by Author 1")));
        assert!(lines.iter().any(|l| l.contains("Alice's new change")));
    }

    #[test]
    fn test_add_changes_for_author_same_author() {
        let entry: Entry = r#"breezy (3.3.4-1) unstable; urgency=low

  [ Alice ]
  * First change

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        entry
            .try_add_changes_for_author("Alice", vec!["* Second change"])
            .unwrap();

        let lines: Vec<_> = entry.change_lines().collect();

        // Should only have one Alice section (not duplicated)
        let alice_count = lines.iter().filter(|l| l.contains("[ Alice ]")).count();
        assert_eq!(alice_count, 1);

        // Should have both changes
        assert!(lines.iter().any(|l| l.contains("First change")));
        assert!(lines.iter().any(|l| l.contains("Second change")));
    }

    #[test]
    fn test_datetime_with_incorrect_day_of_week() {
        // Test for bug: datetime() should parse leniently even when day-of-week doesn't match
        // This changelog entry has "Mon, 22 Mar 2011" but Mar 22, 2011 was actually Tuesday
        let entry: Entry = r#"blah (0.1-2) UNRELEASED; urgency=medium

  * New release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 22 Mar 2011 16:47:42 +0000
"#
        .parse()
        .unwrap();

        // timestamp() should return just the date portion
        assert_eq!(
            entry.timestamp(),
            Some("Mon, 22 Mar 2011 16:47:42 +0000".into())
        );

        // datetime() should successfully parse the timestamp despite incorrect day-of-week
        let datetime = entry.datetime();
        assert!(
            datetime.is_some(),
            "datetime() should not return None for timestamp with incorrect day-of-week"
        );
        assert_eq!(datetime.unwrap().to_rfc3339(), "2011-03-22T16:47:42+00:00");
    }

    #[test]
    fn test_line_col() {
        let text = r#"foo (1.0-1) unstable; urgency=low

  * First change

 -- Maintainer <email@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

bar (2.0-1) experimental; urgency=high

  * Second change
  * Third change

 -- Another <another@example.com>  Tue, 02 Jan 2024 13:00:00 +0000
"#;
        let changelog = text.parse::<ChangeLog>().unwrap();

        // Test changelog root position
        assert_eq!(changelog.line(), 0);
        assert_eq!(changelog.column(), 0);
        assert_eq!(changelog.line_col(), (0, 0));

        // Test entry line numbers
        let entries: Vec<_> = changelog.iter().collect();
        assert_eq!(entries.len(), 2);

        // First entry starts at line 0
        assert_eq!(entries[0].line(), 0);
        assert_eq!(entries[0].column(), 0);
        assert_eq!(entries[0].line_col(), (0, 0));

        // Second entry starts at line 6 (after first entry and empty line)
        assert_eq!(entries[1].line(), 6);
        assert_eq!(entries[1].column(), 0);
        assert_eq!(entries[1].line_col(), (6, 0));

        // Test entry components
        let header = entries[0].header().unwrap();
        assert_eq!(header.line(), 0);
        assert_eq!(header.column(), 0);

        let body = entries[0].body().unwrap();
        assert_eq!(body.line(), 2); // Body starts at first change line

        let footer = entries[0].footer().unwrap();
        assert_eq!(footer.line(), 4); // Footer line

        // Test maintainer and timestamp nodes
        let maintainer = entries[0].maintainer_node().unwrap();
        assert_eq!(maintainer.line(), 4); // On footer line

        let timestamp = entries[0].timestamp_node().unwrap();
        assert_eq!(timestamp.line(), 4); // On footer line

        // Verify second entry components
        let header2 = entries[1].header().unwrap();
        assert_eq!(header2.line(), 6);

        let footer2 = entries[1].footer().unwrap();
        assert_eq!(footer2.line(), 11);
    }

    #[test]
    fn test_entry_at_offset() {
        let text = r#"foo (1.0-1) unstable; urgency=low

  * First change

 -- Maintainer <email@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

bar (2.0-1) experimental; urgency=high

  * Second change
  * Third change

 -- Another <another@example.com>  Tue, 02 Jan 2024 13:00:00 +0000
"#;
        let changelog = text.parse::<ChangeLog>().unwrap();

        // Test offset at the start of first entry
        let entry = changelog.entry_at_offset(0.into()).unwrap();
        assert_eq!(entry.package(), Some("foo".to_string()));

        // Test offset in the middle of first entry (e.g., at "First change")
        let first_change_offset = text.find("First change").unwrap() as u32;
        let entry = changelog
            .entry_at_offset(first_change_offset.into())
            .unwrap();
        assert_eq!(entry.package(), Some("foo".to_string()));

        // Test offset at the footer of first entry
        let first_footer_offset = text.find("Maintainer").unwrap() as u32;
        let entry = changelog
            .entry_at_offset(first_footer_offset.into())
            .unwrap();
        assert_eq!(entry.package(), Some("foo".to_string()));

        // Test offset at the start of second entry
        let second_entry_offset = text.find("bar (2.0-1)").unwrap() as u32;
        let entry = changelog
            .entry_at_offset(second_entry_offset.into())
            .unwrap();
        assert_eq!(entry.package(), Some("bar".to_string()));

        // Test offset in the middle of second entry
        let second_change_offset = text.find("Second change").unwrap() as u32;
        let entry = changelog
            .entry_at_offset(second_change_offset.into())
            .unwrap();
        assert_eq!(entry.package(), Some("bar".to_string()));

        // Test offset at the very end of the changelog
        let end_offset = text.len() as u32;
        let entry = changelog.entry_at_offset(end_offset.into());
        // This should still find the last entry since the offset is at the end
        assert!(entry.is_some());
    }

    #[test]
    fn test_entry_at_offset_empty_changelog() {
        let text = "";
        let changelog = text.parse::<ChangeLog>().unwrap();

        // Test with offset 0 on empty changelog
        let entry = changelog.entry_at_offset(0.into());
        assert!(entry.is_none());
    }

    #[test]
    fn test_entries_in_range() {
        let text = r#"foo (1.0-1) unstable; urgency=low

  * First change

 -- Maintainer <email@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

bar (2.0-1) experimental; urgency=high

  * Second change
  * Third change

 -- Another <another@example.com>  Tue, 02 Jan 2024 13:00:00 +0000

baz (3.0-1) unstable; urgency=medium

  * Fourth change

 -- Third <third@example.com>  Wed, 03 Jan 2024 14:00:00 +0000
"#;
        let changelog = text.parse::<ChangeLog>().unwrap();

        // Test range covering just the first entry
        let first_entry_start = 0u32;
        let first_entry_end = text.find("bar (2.0-1)").unwrap() as u32;
        let range = rowan::TextRange::new(first_entry_start.into(), first_entry_end.into());
        let entries: Vec<_> = changelog.entries_in_range(range).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].package(), Some("foo".to_string()));

        // Test range covering the first two entries
        let second_entry_end = text.find("baz (3.0-1)").unwrap() as u32;
        let range = rowan::TextRange::new(first_entry_start.into(), second_entry_end.into());
        let entries: Vec<_> = changelog.entries_in_range(range).collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].package(), Some("foo".to_string()));
        assert_eq!(entries[1].package(), Some("bar".to_string()));

        // Test range covering all three entries
        let range = rowan::TextRange::new(0.into(), (text.len() as u32).into());
        let entries: Vec<_> = changelog.entries_in_range(range).collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].package(), Some("foo".to_string()));
        assert_eq!(entries[1].package(), Some("bar".to_string()));
        assert_eq!(entries[2].package(), Some("baz".to_string()));

        // Test range in the middle of the second entry
        let second_change_start = text.find("Second change").unwrap() as u32;
        let third_change_end = (text.find("Third change").unwrap() + "Third change".len()) as u32;
        let range = rowan::TextRange::new(second_change_start.into(), third_change_end.into());
        let entries: Vec<_> = changelog.entries_in_range(range).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].package(), Some("bar".to_string()));

        // Test range starting in the middle of first entry and ending in the middle of second
        let first_change_offset = text.find("First change").unwrap() as u32;
        let second_change_offset = text.find("Second change").unwrap() as u32;
        let range = rowan::TextRange::new(first_change_offset.into(), second_change_offset.into());
        let entries: Vec<_> = changelog.entries_in_range(range).collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].package(), Some("foo".to_string()));
        assert_eq!(entries[1].package(), Some("bar".to_string()));
    }

    #[test]
    fn test_entries_in_range_empty() {
        let text = r#"foo (1.0-1) unstable; urgency=low

  * First change

 -- Maintainer <email@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

bar (2.0-1) experimental; urgency=high

  * Second change

 -- Another <another@example.com>  Tue, 02 Jan 2024 13:00:00 +0000
"#;
        let changelog = text.parse::<ChangeLog>().unwrap();

        // Test with a range that's before the first entry (shouldn't happen in practice, but test it)
        // Since the first entry starts at 0, we can't have a range before it, so test an empty range
        let range = rowan::TextRange::new(0.into(), 0.into());
        let entries: Vec<_> = changelog.entries_in_range(range).collect();
        assert_eq!(entries.len(), 0);

        // Test with empty changelog
        let empty_text = "";
        let empty_changelog = empty_text.parse::<ChangeLog>().unwrap();
        let range = rowan::TextRange::new(0.into(), 0.into());
        let entries: Vec<_> = empty_changelog.entries_in_range(range).collect();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_entries_in_range_partial_overlap() {
        let text = r#"foo (1.0-1) unstable; urgency=low

  * First change

 -- Maintainer <email@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

bar (2.0-1) experimental; urgency=high

  * Second change

 -- Another <another@example.com>  Tue, 02 Jan 2024 13:00:00 +0000
"#;
        let changelog = text.parse::<ChangeLog>().unwrap();

        // Test with a range that starts before the first entry ends and goes into the second
        let first_change_offset = text.find("First change").unwrap() as u32;
        let bar_header_end = (text.find("bar (2.0-1)").unwrap() + "bar (2.0-1)".len()) as u32;
        let range = rowan::TextRange::new(first_change_offset.into(), bar_header_end.into());
        let entries: Vec<_> = changelog.entries_in_range(range).collect();
        // Should return both entries since the range intersects with both
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].package(), Some("foo".to_string()));
        assert_eq!(entries[1].package(), Some("bar".to_string()));
    }

    #[test]
    fn test_parse_entry_missing_footer() {
        // When a single entry is missing a footer, subsequent entries should still
        // be parsed correctly.
        let changelog: ChangeLog = r#"foo (1.0-1) unstable; urgency=low

  * First change

bar (2.0-1) unstable; urgency=low

  * Second change

 -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500
"#
        .parse()
        .unwrap();

        let entries: Vec<_> = changelog.iter().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].package(), Some("foo".to_string()));
        assert_eq!(entries[0].version(), Some("1.0-1".parse().unwrap()));
        assert!(entries[0].get_maintainer_identity().is_none());

        assert_eq!(entries[1].package(), Some("bar".to_string()));
        assert_eq!(entries[1].version(), Some("2.0-1".parse().unwrap()));
        let identity = entries[1].get_maintainer_identity().unwrap();
        assert_eq!(identity.name, "Maintainer");
        assert_eq!(identity.email, "maint@example.com");
    }

    #[test]
    fn test_parse_entry_missing_footer_no_blank_line() {
        // When a single entry is missing a footer and there's no blank line
        // before the next entry, subsequent entries should still be parsed.
        let changelog: ChangeLog = "foo (1.0-1) unstable; urgency=low\n\n  * First change\nbar (2.0-1) unstable; urgency=low\n\n  * Second change\n\n -- Maintainer <maint@example.com>  Mon, 04 Sep 2023 18:13:45 -0500\n"
        .parse()
        .unwrap();

        let entries: Vec<_> = changelog.iter().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].package(), Some("foo".to_string()));
        assert!(entries[0].get_maintainer_identity().is_none());

        assert_eq!(entries[1].package(), Some("bar".to_string()));
        let identity = entries[1].get_maintainer_identity().unwrap();
        assert_eq!(identity.name, "Maintainer");
        assert_eq!(identity.email, "maint@example.com");
    }

    #[test]
    fn test_parse_entry_trailing_space_instead_of_footer() {
        // Entry has " \n" (trailing space line) instead of a proper footer.
        // The parser should recover and still parse the second entry.
        let parsed = ChangeLog::parse("foo (42.0.5-1) unstable; urgency=medium\n\n  * Team upload.\n  * detail\n\n \n\nbar (41.0.7-5) unstable; urgency=medium\n\n  * AMAU\n\n -- Name <email@example.com>  Thu, 07 Mar 2024 13:42:35 +0100\n");
        let changelog: ChangeLog = parsed.tree();

        let entries: Vec<_> = changelog.iter().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].package(), Some("foo".to_string()));
        assert!(entries[0].get_maintainer_identity().is_none());

        assert_eq!(entries[1].package(), Some("bar".to_string()));
        let identity = entries[1].get_maintainer_identity().unwrap();
        assert_eq!(identity.name, "Name");
        assert_eq!(identity.email, "email@example.com");
    }

    #[test]
    fn test_parse_middle_entry_missing_footer() {
        // Three entries, middle one missing footer
        let changelog: ChangeLog = r#"foo (3.0-1) unstable; urgency=low

  * Third change

 -- Alice <alice@example.com>  Mon, 04 Sep 2023 18:13:45 -0500

bar (2.0-1) unstable; urgency=low

  * Second change (no footer)

baz (1.0-1) unstable; urgency=low

  * First change

 -- Bob <bob@example.com>  Sat, 02 Sep 2023 16:11:15 -0500
"#
        .parse()
        .unwrap();

        let entries: Vec<_> = changelog.iter().collect();
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].package(), Some("foo".to_string()));
        let identity = entries[0].get_maintainer_identity().unwrap();
        assert_eq!(identity.name, "Alice");

        assert_eq!(entries[1].package(), Some("bar".to_string()));
        assert!(entries[1].get_maintainer_identity().is_none());

        assert_eq!(entries[2].package(), Some("baz".to_string()));
        let identity = entries[2].get_maintainer_identity().unwrap();
        assert_eq!(identity.name, "Bob");
    }

    #[test]
    fn test_parse_real_world_malformed_footer() {
        // Real-world case: first entry has an empty bullet ("  * \n") and a
        // truncated footer line (" \n") instead of " -- maintainer <email>  date".
        // The second entry has a proper footer. Both should be parsed.
        let input = concat!(
            "python-cryptography (42.0.5-1) unstable; urgency=medium\n",
            "\n",
            "  * Team upload.\n",
            "  * Bump setuptools-rust\n",
            "\n",
            "  [ Andreas Tille ]\n",
            "  * New upstream version\n",
            "\n",
            "  [ Andrey Rakhmatullin ]\n",
            "  * Add myself to Uploaders.\n",
            "  * \n",
            "\n",
            "  [ Jérémy Lal ]\n",
            "  * Testsuite: autopkgtest-pkg-pybuild\n",
            "\n",
            " \n",
            "\n",
            "python-cryptography (41.0.7-5) unstable; urgency=medium\n",
            "\n",
            "  * AMAU, Closes: #1064979\n",
            "\n",
            "  [ Andreas Tille ]\n",
            "  * Enable building twice in a row\n",
            "\n",
            " -- Jérémy Lal <kapouer@melix.org>  Thu, 07 Mar 2024 13:42:35 +0100\n",
        );
        let parsed = ChangeLog::parse(input);
        assert!(!parsed.errors().is_empty());
        let changelog: ChangeLog = parsed.tree();

        let entries: Vec<_> = changelog.iter().collect();
        assert_eq!(entries.len(), 2);

        assert_eq!(
            entries[0].package(),
            Some("python-cryptography".to_string())
        );
        assert_eq!(entries[0].version(), Some("42.0.5-1".parse().unwrap()));
        assert!(entries[0].get_maintainer_identity().is_none());

        assert_eq!(
            entries[1].package(),
            Some("python-cryptography".to_string())
        );
        assert_eq!(entries[1].version(), Some("41.0.7-5".parse().unwrap()));
        let identity = entries[1].get_maintainer_identity().unwrap();
        assert_eq!(identity.name, "Jérémy Lal");
        assert_eq!(identity.email, "kapouer@melix.org");
    }

    /// Helper: parse old text, apply edit to get new text, reparse incrementally,
    /// and verify the result matches a full parse of the new text.
    fn assert_incremental_matches_full(
        old_text: &str,
        edit_start: u32,
        edit_end: u32,
        replacement: &str,
    ) {
        let parsed = parse(old_text);
        let mut new_text = old_text.to_string();
        new_text.replace_range(edit_start as usize..edit_end as usize, replacement);

        let new_edit_end = edit_start + replacement.len() as u32;
        let edit_range = rowan::TextRange::new(
            rowan::TextSize::from(edit_start),
            rowan::TextSize::from(new_edit_end),
        );

        let incremental = parsed.reparse(&new_text, edit_range);
        let full = parse(&new_text);

        let inc_tree = incremental.tree();
        let full_tree = full.tree();
        assert_eq!(
            inc_tree.syntax().text().to_string(),
            full_tree.syntax().text().to_string(),
            "tree text mismatch for edit [{edit_start}..{edit_end}] -> {replacement:?}"
        );

        assert_eq!(
            incremental.green(),
            full.green(),
            "green node mismatch for edit [{edit_start}..{edit_end}] -> {replacement:?}"
        );
    }

    const TWO_ENTRY_CHANGELOG: &str = "\
breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500

breezy (3.3.3-2) unstable; urgency=medium

  * Drop unnecessary dependency on python3-six. Closes: #1039011

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 24 Jun 2023 14:58:57 +0100
";

    #[test]
    fn test_reparse_edit_within_first_entry() {
        let old = TWO_ENTRY_CHANGELOG;
        // Change "low" to "high" in the first entry's urgency
        let start = old.find("low").unwrap() as u32;
        let end = start + 3;
        assert_incremental_matches_full(old, start, end, "high");
    }

    #[test]
    fn test_reparse_edit_within_second_entry() {
        let old = TWO_ENTRY_CHANGELOG;
        // Change "medium" to "high" in the second entry's urgency
        let start = old.rfind("medium").unwrap() as u32;
        let end = start + 6;
        assert_incremental_matches_full(old, start, end, "high");
    }

    #[test]
    fn test_reparse_add_detail_line() {
        let old = TWO_ENTRY_CHANGELOG;
        // Insert a new detail line after "New upstream release.\n"
        let insert_point =
            old.find("  * New upstream release.\n").unwrap() + "  * New upstream release.\n".len();
        assert_incremental_matches_full(
            old,
            insert_point as u32,
            insert_point as u32,
            "  * Fix important bug.\n",
        );
    }

    #[test]
    fn test_reparse_change_version() {
        let old = TWO_ENTRY_CHANGELOG;
        // Change version "3.3.4-1" to "3.3.5-1"
        let start = old.find("3.3.4-1").unwrap() as u32;
        let end = start + 7;
        assert_incremental_matches_full(old, start, end, "3.3.5-1");
    }

    #[test]
    fn test_reparse_single_entry_falls_back() {
        let old = "\
breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0500
";
        let start = old.find("low").unwrap() as u32;
        let end = start + 3;
        assert_incremental_matches_full(old, start, end, "high");
    }

    #[test]
    fn test_reparse_preserves_unchanged_entry() {
        let old = TWO_ENTRY_CHANGELOG;
        let parsed = parse(old);
        let old_green = parsed.green().clone();

        // Edit only in first entry: "low" -> "high"
        let start = old.find("low").unwrap() as u32;
        let end = start + 3;
        let mut new_text = old.to_string();
        new_text.replace_range(start as usize..end as usize, "high");
        let new_edit_end = start + 4; // "high" is 4 bytes
        let edit_range = rowan::TextRange::new(
            rowan::TextSize::from(start),
            rowan::TextSize::from(new_edit_end),
        );

        let reparsed = parsed.reparse(&new_text, edit_range);
        let new_green = reparsed.green().clone();

        let old_children: Vec<_> = old_green.children().collect();
        let new_children: Vec<_> = new_green.children().collect();

        // Should have the same number of children
        assert_eq!(old_children.len(), new_children.len());

        // The second entry (last child) should be structurally identical
        assert_eq!(
            old_children.last(),
            new_children.last(),
            "unchanged entry should be reused"
        );

        // The first entry should differ
        assert_ne!(old_children[0], new_children[0]);
    }
}
