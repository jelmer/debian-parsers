//! Parser for relationship fields like `Depends`, `Recommends`, etc.
//!
//! # Example
//! ```
//! use debian_control::edit::relations::{Relations, Relation};
//! use debian_control::relations::VersionConstraint;
//!
//! let mut relations: Relations = r"python3-dulwich (>= 0.19.0), python3-requests, python3-urllib3 (<< 1.26.0)".parse().unwrap();
//! assert_eq!(relations.to_string(), "python3-dulwich (>= 0.19.0), python3-requests, python3-urllib3 (<< 1.26.0)");
//! assert!(relations.satisfied_by(|name: &str| -> Option<debversion::Version> {
//!    match name {
//!    "python3-dulwich" => Some("0.19.0".parse().unwrap()),
//!    "python3-requests" => Some("2.25.1".parse().unwrap()),
//!    "python3-urllib3" => Some("1.25.11".parse().unwrap()),
//!    _ => None
//!    }}));
//! relations.remove_entry(1);
//! relations.get_entry(0).unwrap().get_relation(0).unwrap().set_archqual("amd64");
//! assert_eq!(relations.to_string(), "python3-dulwich:amd64 (>= 0.19.0), python3-urllib3 (<< 1.26.0)");
//! ```
use crate::relations::SyntaxKind::{self, *};
use crate::relations::{BuildProfile, VersionConstraint};
use debversion::Version;
use rowan::{Direction, NodeOrToken};
use std::collections::HashSet;

/// Error type for parsing relations fields
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

/// The parse results are stored as a "green tree".
/// We'll discuss working with the results later
struct Parse {
    green_node: GreenNode,
    #[allow(unused)]
    errors: Vec<String>,
}

fn parse(text: &str, allow_substvar: bool) -> Parse {
    struct Parser {
        /// input tokens, including whitespace,
        /// in *reverse* order.
        tokens: Vec<(SyntaxKind, String)>,
        /// the in-progress tree.
        builder: GreenNodeBuilder<'static>,
        /// the list of syntax errors we've accumulated
        /// so far.
        errors: Vec<String>,
        /// whether to allow substvars
        allow_substvar: bool,
    }

    impl Parser {
        fn parse_substvar(&mut self) {
            self.builder.start_node(SyntaxKind::SUBSTVAR.into());
            self.bump();
            if self.current() != Some(L_CURLY) {
                self.error(format!("expected {{ but got {:?}", self.current()).to_string());
            } else {
                self.bump();
            }
            loop {
                match self.current() {
                    Some(IDENT) | Some(COLON) => {
                        self.bump();
                    }
                    Some(R_CURLY) => {
                        break;
                    }
                    e => {
                        self.error(format!("expected identifier or : but got {:?}", e).to_string());
                        break;
                    }
                }
            }
            if self.current() != Some(R_CURLY) {
                self.error(format!("expected }} but got {:?}", self.current()).to_string());
            } else {
                self.bump();
            }
            self.builder.finish_node();
        }

        fn parse_entry(&mut self) {
            self.skip_ws();
            self.builder.start_node(SyntaxKind::ENTRY.into());
            loop {
                self.parse_relation();
                match self.peek_past_ws() {
                    Some(COMMA) => {
                        break;
                    }
                    Some(PIPE) => {
                        self.skip_ws();
                        self.bump();
                        self.skip_ws();
                    }
                    None => {
                        self.skip_ws();
                        break;
                    }
                    _ => {
                        self.skip_ws();
                        self.builder.start_node(SyntaxKind::ERROR.into());
                        match self.tokens.pop() {
                            Some((k, t)) => {
                                self.builder.token(k.into(), t.as_str());
                                self.errors
                                    .push(format!("Expected comma or pipe, not {:?}", (k, t)));
                            }
                            None => {
                                self.errors
                                    .push("Expected comma or pipe, got end of file".to_string());
                            }
                        }
                        self.builder.finish_node();
                    }
                }
            }
            self.builder.finish_node();
        }

        fn error(&mut self, error: String) {
            self.errors.push(error);
            self.builder.start_node(SyntaxKind::ERROR.into());
            if self.current().is_some() {
                self.bump();
            }
            self.builder.finish_node();
        }

        fn parse_relation(&mut self) {
            self.builder.start_node(SyntaxKind::RELATION.into());
            if self.current() == Some(IDENT) {
                self.bump();
            } else {
                self.error("Expected package name".to_string());
            }
            match self.peek_past_ws() {
                Some(COLON) => {
                    self.skip_ws();
                    self.builder.start_node(ARCHQUAL.into());
                    self.bump();
                    self.skip_ws();
                    if self.current() == Some(IDENT) {
                        self.bump();
                    } else {
                        self.error("Expected architecture name".to_string());
                    }
                    self.builder.finish_node();
                    self.skip_ws();
                }
                Some(PIPE) | Some(COMMA) => {}
                None | Some(L_PARENS) | Some(L_BRACKET) | Some(L_ANGLE) => {
                    self.skip_ws();
                }
                e => {
                    self.skip_ws();
                    self.error(format!(
                        "Expected ':' or '|' or '[' or '<' or ',' but got {:?}",
                        e
                    ));
                }
            }

            if self.peek_past_ws() == Some(L_PARENS) {
                self.skip_ws();
                self.builder.start_node(VERSION.into());
                self.bump();
                self.skip_ws();

                self.builder.start_node(CONSTRAINT.into());

                while self.current() == Some(L_ANGLE)
                    || self.current() == Some(R_ANGLE)
                    || self.current() == Some(EQUAL)
                {
                    self.bump();
                }

                self.builder.finish_node();

                self.skip_ws();

                // Read IDENT and COLON tokens until we see R_PARENS
                // This handles version strings with epochs (e.g., "1:2.3.2-2~")
                while matches!(self.current(), Some(IDENT) | Some(COLON)) {
                    self.bump();
                }

                if self.current() == Some(R_PARENS) {
                    self.bump();
                } else {
                    self.error("Expected ')'".to_string());
                }

                self.builder.finish_node();
            }

            if self.peek_past_ws() == Some(L_BRACKET) {
                self.skip_ws();
                self.builder.start_node(ARCHITECTURES.into());
                self.bump();
                loop {
                    self.skip_ws();
                    match self.current() {
                        Some(NOT) => {
                            self.bump();
                        }
                        Some(IDENT) => {
                            self.bump();
                        }
                        Some(R_BRACKET) => {
                            self.bump();
                            break;
                        }
                        _ => {
                            self.error("Expected architecture name or '!' or ']'".to_string());
                            break;
                        }
                    }
                }
                self.builder.finish_node();
            }

            while self.peek_past_ws() == Some(L_ANGLE) {
                self.skip_ws();
                self.builder.start_node(PROFILES.into());
                self.bump();

                loop {
                    self.skip_ws();
                    match self.current() {
                        Some(IDENT) => {
                            self.bump();
                        }
                        Some(NOT) => {
                            self.bump();
                            self.skip_ws();
                            if self.current() == Some(IDENT) {
                                self.bump();
                            } else {
                                self.error("Expected profile".to_string());
                            }
                        }
                        Some(R_ANGLE) => {
                            self.bump();
                            break;
                        }
                        None => {
                            self.error("Expected profile or '>'".to_string());
                            break;
                        }
                        _ => {
                            self.error("Expected profile or '!' or '>'".to_string());
                            break;
                        }
                    }
                }

                self.builder.finish_node();
            }

            self.builder.finish_node();
        }

        fn parse(mut self) -> Parse {
            self.builder.start_node(SyntaxKind::ROOT.into());

            self.skip_ws();

            while self.current().is_some() {
                match self.current() {
                    Some(IDENT) => self.parse_entry(),
                    Some(DOLLAR) => {
                        if self.allow_substvar {
                            self.parse_substvar()
                        } else {
                            self.error("Substvars are not allowed".to_string());
                        }
                    }
                    Some(COMMA) => {
                        // Empty entry, but that's okay - probably?
                    }
                    Some(c) => {
                        self.error(format!("expected $ or identifier but got {:?}", c));
                    }
                    None => {
                        self.error("expected identifier but got end of file".to_string());
                    }
                }

                self.skip_ws();
                match self.current() {
                    Some(COMMA) => {
                        self.bump();
                    }
                    None => {
                        break;
                    }
                    c => {
                        self.error(format!("expected comma or end of file but got {:?}", c));
                    }
                }
                self.skip_ws();
            }

            self.builder.finish_node();
            // Turn the builder into a GreenNode
            Parse {
                green_node: self.builder.finish(),
                errors: self.errors,
            }
        }
        /// Advance one token, adding it to the current branch of the tree builder.
        fn bump(&mut self) {
            let (kind, text) = self.tokens.pop().unwrap();
            self.builder.token(kind.into(), text.as_str());
        }
        /// Peek at the first unprocessed token
        fn current(&self) -> Option<SyntaxKind> {
            self.tokens.last().map(|(kind, _)| *kind)
        }
        fn skip_ws(&mut self) {
            while matches!(
                self.current(),
                Some(WHITESPACE) | Some(NEWLINE) | Some(COMMENT)
            ) {
                self.bump()
            }
        }

        fn peek_past_ws(&self) -> Option<SyntaxKind> {
            let mut i = self.tokens.len();
            while i > 0 {
                i -= 1;
                match self.tokens[i].0 {
                    WHITESPACE | NEWLINE | COMMENT => {}
                    _ => return Some(self.tokens[i].0),
                }
            }
            None
        }
    }

    let mut tokens = crate::relations::lex(text);
    tokens.reverse();
    Parser {
        tokens,
        builder: GreenNodeBuilder::new(),
        errors: Vec::new(),
        allow_substvar,
    }
    .parse()
}

// To work with the parse results we need a view into the
// green tree - the Syntax tree.
// It is also immutable, like a GreenNode,
// but it contains parent pointers, offsets, and
// has identity semantics.

/// A syntax node in the relations tree.
pub type SyntaxNode = rowan::SyntaxNode<Lang>;
/// A syntax token in the relations tree.
pub type SyntaxToken = rowan::SyntaxToken<Lang>;
/// A syntax element (node or token) in the relations tree.
pub type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

impl Parse {
    fn root_mut(&self) -> Relations {
        Relations::cast(SyntaxNode::new_root_mut(self.green_node.clone())).unwrap()
    }
}

macro_rules! ast_node {
    ($ast:ident, $kind:ident) => {
        /// A node in the syntax tree representing a $ast
        #[repr(transparent)]
        pub struct $ast(SyntaxNode);
        impl $ast {
            #[allow(unused)]
            fn cast(node: SyntaxNode) -> Option<Self> {
                if node.kind() == $kind {
                    Some(Self(node))
                } else {
                    None
                }
            }

            /// Access the underlying syntax node.
            pub fn syntax(&self) -> &rowan::SyntaxNode<Lang> {
                &self.0
            }
        }

        impl std::fmt::Display for $ast {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0.text().to_string())
            }
        }
    };
}

ast_node!(Relations, ROOT);
ast_node!(Entry, ENTRY);
ast_node!(Relation, RELATION);
ast_node!(Substvar, SUBSTVAR);

impl PartialEq for Relations {
    fn eq(&self, other: &Self) -> bool {
        self.entries().collect::<Vec<_>>() == other.entries().collect::<Vec<_>>()
    }
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.relations().collect::<Vec<_>>() == other.relations().collect::<Vec<_>>()
    }
}

impl PartialEq for Relation {
    fn eq(&self, other: &Self) -> bool {
        self.try_name() == other.try_name()
            && self.version() == other.version()
            && self.archqual() == other.archqual()
            && self.architectures().map(|x| x.collect::<HashSet<_>>())
                == other.architectures().map(|x| x.collect::<HashSet<_>>())
            && self.profiles().eq(other.profiles())
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Relations {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let rep = self.to_string();
        serializer.serialize_str(&rep)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Relations {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let relations = s.parse().map_err(serde::de::Error::custom)?;
        Ok(relations)
    }
}

impl std::fmt::Debug for Relations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Relations");

        for entry in self.entries() {
            s.field("entry", &entry);
        }

        s.finish()
    }
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Entry");

        for relation in self.relations() {
            s.field("relation", &relation);
        }

        s.finish()
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Entry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let rep = self.to_string();
        serializer.serialize_str(&rep)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Entry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let entry = s.parse().map_err(serde::de::Error::custom)?;
        Ok(entry)
    }
}

impl std::fmt::Debug for Relation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Relation");

        s.field("name", &self.try_name());

        if let Some((vc, version)) = self.version() {
            s.field("version", &vc);
            s.field("version", &version);
        }

        s.finish()
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Relation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let rep = self.to_string();
        serializer.serialize_str(&rep)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Relation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let relation = s.parse().map_err(serde::de::Error::custom)?;
        Ok(relation)
    }
}

impl Default for Relations {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a package name should be treated as special for sorting purposes.
///
/// Special names include substitution variables (like `${misc:Depends}`)
/// and template variables (like `@cdbs@`).
fn is_special_package_name(name: &str) -> bool {
    // Substitution variables like ${misc:Depends}
    if name.starts_with("${") && name.ends_with('}') {
        return true;
    }
    // Template variables like @cdbs@
    if name.starts_with('@') && name.ends_with('@') {
        return true;
    }
    false
}

/// Trait for defining sorting order of package relations.
pub trait SortingOrder {
    /// Compare two package names for sorting.
    ///
    /// Returns true if `name1` should come before `name2`.
    fn lt(&self, name1: &str, name2: &str) -> bool;

    /// Check if a package name should be ignored for sorting purposes.
    fn ignore(&self, name: &str) -> bool;
}

/// Default sorting order (lexicographical with special items last).
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultSortingOrder;

impl SortingOrder for DefaultSortingOrder {
    fn lt(&self, name1: &str, name2: &str) -> bool {
        let special1 = is_special_package_name(name1);
        let special2 = is_special_package_name(name2);

        // Special items always come last
        if special1 && !special2 {
            return false;
        }
        if !special1 && special2 {
            return true;
        }
        if special1 && special2 {
            // Both special - maintain original order
            return false;
        }

        // Both are regular packages, use alphabetical order
        name1 < name2
    }

    fn ignore(&self, name: &str) -> bool {
        is_special_package_name(name)
    }
}

/// Sorting order matching wrap-and-sort behavior.
///
/// This sorting order matches the behavior of the devscripts wrap-and-sort tool.
/// It sorts packages into three groups:
/// 1. Build-system packages (debhelper-compat, cdbs, etc.) - sorted first
/// 2. Regular packages starting with [a-z0-9] - sorted in the middle
/// 3. Substvars and other special packages - sorted last
///
/// Within each group, packages are sorted lexicographically.
#[derive(Debug, Clone, Copy, Default)]
pub struct WrapAndSortOrder;

impl WrapAndSortOrder {
    /// Build systems that should be sorted first, matching wrap-and-sort
    const BUILD_SYSTEMS: &'static [&'static str] = &[
        "cdbs",
        "debhelper-compat",
        "debhelper",
        "debputy",
        "dpkg-build-api",
        "dpkg-dev",
    ];

    fn get_sort_key<'a>(&self, name: &'a str) -> (i32, &'a str) {
        // Check if it's a build system (including dh-* packages)
        if Self::BUILD_SYSTEMS.contains(&name) || name.starts_with("dh-") {
            return (-1, name);
        }

        // Check if it starts with a regular character
        if name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return (0, name);
        }

        // Special packages (substvars, etc.) go last
        (1, name)
    }
}

impl SortingOrder for WrapAndSortOrder {
    fn lt(&self, name1: &str, name2: &str) -> bool {
        self.get_sort_key(name1) < self.get_sort_key(name2)
    }

    fn ignore(&self, _name: &str) -> bool {
        // wrap-and-sort doesn't ignore any packages - it sorts everything
        false
    }
}

impl Relations {
    /// Create a new relations field
    pub fn new() -> Self {
        Self::from(vec![])
    }

    /// Wrap and sort this relations field
    #[must_use]
    pub fn wrap_and_sort(self) -> Self {
        let mut entries = self
            .entries()
            .map(|e| e.wrap_and_sort())
            .collect::<Vec<_>>();
        entries.sort();
        // TODO: preserve comments
        Self::from(entries)
    }

    /// Iterate over the entries in this relations field
    pub fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        self.0.children().filter_map(Entry::cast)
    }

    /// Iterate over the entries in this relations field
    pub fn iter(&self) -> impl Iterator<Item = Entry> + '_ {
        self.entries()
    }

    /// Remove the entry at the given index
    pub fn get_entry(&self, idx: usize) -> Option<Entry> {
        self.entries().nth(idx)
    }

    /// Remove the entry at the given index
    pub fn remove_entry(&mut self, idx: usize) -> Entry {
        let mut entry = self.get_entry(idx).unwrap();
        entry.remove();
        entry
    }

    /// Helper to collect all consecutive WHITESPACE/NEWLINE/COMMENT tokens starting from a node
    fn collect_whitespace(start: Option<NodeOrToken<SyntaxNode, SyntaxToken>>) -> String {
        let mut pattern = String::new();
        let mut current = start;
        while let Some(token) = current {
            if matches!(token.kind(), WHITESPACE | NEWLINE | COMMENT) {
                if let NodeOrToken::Token(t) = &token {
                    pattern.push_str(t.text());
                }
                current = token.next_sibling_or_token();
            } else {
                break;
            }
        }
        pattern
    }

    /// Helper to convert a NodeOrToken to its green equivalent
    fn to_green(node: &NodeOrToken<SyntaxNode, SyntaxToken>) -> NodeOrToken<GreenNode, GreenToken> {
        match node {
            NodeOrToken::Node(n) => NodeOrToken::Node(n.green().into()),
            NodeOrToken::Token(t) => NodeOrToken::Token(t.green().to_owned()),
        }
    }

    /// Helper to check if a token is whitespace or a comment
    fn is_whitespace_token(token: &GreenToken) -> bool {
        token.kind() == rowan::SyntaxKind(WHITESPACE as u16)
            || token.kind() == rowan::SyntaxKind(NEWLINE as u16)
            || token.kind() == rowan::SyntaxKind(COMMENT as u16)
    }

    /// Helper to strip trailing whitespace tokens from a list of green children
    fn strip_trailing_ws_from_children(
        mut children: Vec<NodeOrToken<GreenNode, GreenToken>>,
    ) -> Vec<NodeOrToken<GreenNode, GreenToken>> {
        while let Some(last) = children.last() {
            if let NodeOrToken::Token(t) = last {
                if Self::is_whitespace_token(t) {
                    children.pop();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        children
    }

    /// Helper to strip trailing whitespace from a RELATION node's children
    fn strip_relation_trailing_ws(relation: &SyntaxNode) -> GreenNode {
        let children: Vec<_> = relation
            .children_with_tokens()
            .map(|c| Self::to_green(&c))
            .collect();
        let stripped = Self::strip_trailing_ws_from_children(children);
        GreenNode::new(relation.kind().into(), stripped)
    }

    /// Helper to build nodes for insertion with odd syntax
    fn build_odd_syntax_nodes(
        before_ws: &str,
        after_ws: &str,
    ) -> Vec<NodeOrToken<GreenNode, GreenToken>> {
        [
            (!before_ws.is_empty())
                .then(|| NodeOrToken::Token(GreenToken::new(WHITESPACE.into(), before_ws))),
            Some(NodeOrToken::Token(GreenToken::new(COMMA.into(), ","))),
            (!after_ws.is_empty())
                .then(|| NodeOrToken::Token(GreenToken::new(WHITESPACE.into(), after_ws))),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Detect if relations use odd syntax (whitespace before comma) and return whitespace parts
    fn detect_odd_syntax(&self) -> Option<(String, String)> {
        for entry_node in self.entries() {
            let mut node = entry_node.0.next_sibling_or_token()?;

            // Skip whitespace/comment tokens and collect their text
            let mut before = String::new();
            while matches!(node.kind(), WHITESPACE | NEWLINE | COMMENT) {
                if let NodeOrToken::Token(t) = &node {
                    before.push_str(t.text());
                }
                node = node.next_sibling_or_token()?;
            }

            // Check if we found a comma after whitespace
            if node.kind() == COMMA && !before.is_empty() {
                let after = Self::collect_whitespace(node.next_sibling_or_token());
                return Some((before, after));
            }
        }
        None
    }

    /// Detect the most common whitespace pattern after commas in the relations.
    ///
    /// This matches debmutate's behavior of analyzing existing whitespace
    /// patterns to preserve formatting when adding entries.
    ///
    /// # Arguments
    /// * `default` - The default whitespace pattern to use if no pattern is detected
    fn detect_whitespace_pattern(&self, default: &str) -> String {
        use std::collections::HashMap;

        let entries: Vec<_> = self.entries().collect();
        let num_entries = entries.len();

        if num_entries == 0 {
            // Check if there are any substvars
            if self.substvars().next().is_some() {
                // Has substvars but no entries - use default spacing
                return default.to_string();
            }
            return String::from(""); // Truly empty - first entry gets no prefix
        }

        if num_entries == 1 {
            // Single entry - check if there's a pattern after it
            if let Some(node) = entries[0].0.next_sibling_or_token() {
                if node.kind() == COMMA {
                    let pattern = Self::collect_whitespace(node.next_sibling_or_token());
                    if !pattern.is_empty() {
                        return pattern;
                    }
                }
            }
            return default.to_string(); // Use default for single entry with no pattern
        }

        // Count whitespace patterns after commas (excluding the last entry)
        let mut whitespace_counts: HashMap<String, usize> = HashMap::new();

        for (i, entry) in entries.iter().enumerate() {
            if i == num_entries - 1 {
                break; // Skip the last entry
            }

            // Look for comma and whitespace after this entry
            if let Some(mut node) = entry.0.next_sibling_or_token() {
                // Skip any whitespace/newlines/comments before the comma (odd syntax)
                while matches!(node.kind(), WHITESPACE | NEWLINE | COMMENT) {
                    if let Some(next) = node.next_sibling_or_token() {
                        node = next;
                    } else {
                        break;
                    }
                }

                // Found comma, collect all whitespace/newlines after it
                if node.kind() == COMMA {
                    let pattern = Self::collect_whitespace(node.next_sibling_or_token());
                    if !pattern.is_empty() {
                        *whitespace_counts.entry(pattern).or_insert(0) += 1;
                    }
                }
            }
        }

        // If there's exactly one pattern, use it
        if whitespace_counts.len() == 1 {
            if let Some((ws, _)) = whitespace_counts.iter().next() {
                return ws.clone();
            }
        }

        // Multiple patterns - use the most common
        if let Some((ws, _)) = whitespace_counts.iter().max_by_key(|(_, count)| *count) {
            return ws.clone();
        }

        // Use the provided default
        default.to_string()
    }

    /// Insert a new entry at the given index
    ///
    /// # Arguments
    /// * `idx` - The index to insert at
    /// * `entry` - The entry to insert
    /// * `default_sep` - Optional default separator to use if no pattern is detected (defaults to " ")
    pub fn insert_with_separator(&mut self, idx: usize, entry: Entry, default_sep: Option<&str>) {
        let is_empty = self.entries().next().is_none();
        let whitespace = self.detect_whitespace_pattern(default_sep.unwrap_or(" "));

        // Strip trailing whitespace first
        self.strip_trailing_whitespace();

        // Detect odd syntax (whitespace before comma)
        let odd_syntax = self.detect_odd_syntax();

        let (position, new_children) = if let Some(current_entry) = self.entries().nth(idx) {
            let to_insert = if idx == 0 && is_empty {
                vec![entry.0.green().into()]
            } else if let Some((before_ws, after_ws)) = &odd_syntax {
                let mut nodes = vec![entry.0.green().into()];
                nodes.extend(Self::build_odd_syntax_nodes(before_ws, after_ws));
                nodes
            } else {
                vec![
                    entry.0.green().into(),
                    NodeOrToken::Token(GreenToken::new(COMMA.into(), ",")),
                    NodeOrToken::Token(GreenToken::new(WHITESPACE.into(), whitespace.as_str())),
                ]
            };

            (current_entry.0.index(), to_insert)
        } else {
            let child_count = self.0.children_with_tokens().count();
            let to_insert = if idx == 0 {
                vec![entry.0.green().into()]
            } else if let Some((before_ws, after_ws)) = &odd_syntax {
                let mut nodes = Self::build_odd_syntax_nodes(before_ws, after_ws);
                nodes.push(entry.0.green().into());
                nodes
            } else {
                vec![
                    NodeOrToken::Token(GreenToken::new(COMMA.into(), ",")),
                    NodeOrToken::Token(GreenToken::new(WHITESPACE.into(), whitespace.as_str())),
                    entry.0.green().into(),
                ]
            };

            (child_count, to_insert)
        };
        // We can safely replace the root here since Relations is a root node
        self.0 = SyntaxNode::new_root_mut(
            self.0.replace_with(
                self.0
                    .green()
                    .splice_children(position..position, new_children),
            ),
        );
    }

    /// Insert a new entry at the given index with default separator
    pub fn insert(&mut self, idx: usize, entry: Entry) {
        self.insert_with_separator(idx, entry, None);
    }

    /// Helper to recursively strip trailing whitespace from an ENTRY node
    fn strip_entry_trailing_ws(entry: &SyntaxNode) -> GreenNode {
        let mut children: Vec<_> = entry
            .children_with_tokens()
            .map(|c| Self::to_green(&c))
            .collect();

        // Strip trailing whitespace from the last RELATION if present
        if let Some(NodeOrToken::Node(last)) = children.last() {
            if last.kind() == rowan::SyntaxKind(RELATION as u16) {
                // Replace last child with stripped version
                let relation_node = entry.children().last().unwrap();
                children.pop();
                children.push(NodeOrToken::Node(Self::strip_relation_trailing_ws(
                    &relation_node,
                )));
            }
        }

        // Strip trailing whitespace tokens at entry level
        let stripped = Self::strip_trailing_ws_from_children(children);
        GreenNode::new(ENTRY.into(), stripped)
    }

    fn strip_trailing_whitespace(&mut self) {
        let mut children: Vec<_> = self
            .0
            .children_with_tokens()
            .map(|c| Self::to_green(&c))
            .collect();

        // Strip trailing whitespace from the last ENTRY if present
        if let Some(NodeOrToken::Node(last)) = children.last() {
            if last.kind() == rowan::SyntaxKind(ENTRY as u16) {
                let last_entry = self.0.children().last().unwrap();
                children.pop();
                children.push(NodeOrToken::Node(Self::strip_entry_trailing_ws(
                    &last_entry,
                )));
            }
        }

        // Strip trailing whitespace tokens at root level
        let stripped = Self::strip_trailing_ws_from_children(children);

        let nc = self.0.children_with_tokens().count();
        self.0 = SyntaxNode::new_root_mut(
            self.0
                .replace_with(self.0.green().splice_children(0..nc, stripped)),
        );
    }

    /// Replace the entry at the given index
    pub fn replace(&mut self, idx: usize, entry: Entry) {
        let current_entry = self.get_entry(idx).unwrap();
        self.0.splice_children(
            current_entry.0.index()..current_entry.0.index() + 1,
            vec![entry.0.into()],
        );
    }

    /// Push a new entry to the relations field
    pub fn push(&mut self, entry: Entry) {
        let pos = self.entries().count();
        self.insert(pos, entry);
    }

    /// Return the names of substvars in this relations field
    pub fn substvars(&self) -> impl Iterator<Item = String> + '_ {
        self.substvar_nodes().map(|s| s.to_string())
    }

    /// Return the substvar nodes in this relations field
    pub fn substvar_nodes(&self) -> impl Iterator<Item = Substvar> + '_ {
        self.0.children().filter_map(Substvar::cast)
    }

    /// Parse a relations field from a string, allowing syntax errors
    pub fn parse_relaxed(s: &str, allow_substvar: bool) -> (Relations, Vec<String>) {
        let parse = parse(s, allow_substvar);
        (parse.root_mut(), parse.errors)
    }

    /// Check if this relations field is satisfied by the given package versions.
    pub fn satisfied_by(&self, package_version: impl crate::VersionLookup + Copy) -> bool {
        self.entries().all(|e| e.satisfied_by(package_version))
    }

    /// Check if this relations field is empty
    pub fn is_empty(&self) -> bool {
        self.entries().count() == 0
    }

    /// Get the number of entries in this relations field
    pub fn len(&self) -> usize {
        self.entries().count()
    }

    /// Ensure that a package has at least a minimum version constraint.
    ///
    /// If the package already exists with a version constraint that satisfies
    /// the minimum version, it is left unchanged. Otherwise, the constraint
    /// is updated or added.
    ///
    /// # Arguments
    /// * `package` - The package name
    /// * `minimum_version` - The minimum version required
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let mut relations: Relations = "debhelper (>= 9)".parse().unwrap();
    /// relations.ensure_minimum_version("debhelper", &"12".parse().unwrap());
    /// assert_eq!(relations.to_string(), "debhelper (>= 12)");
    ///
    /// let mut relations: Relations = "python3".parse().unwrap();
    /// relations.ensure_minimum_version("debhelper", &"12".parse().unwrap());
    /// assert!(relations.to_string().contains("debhelper (>= 12)"));
    /// ```
    pub fn ensure_minimum_version(&mut self, package: &str, minimum_version: &Version) {
        let mut found = false;
        let mut obsolete_indices = vec![];
        let mut update_idx = None;

        let entries: Vec<_> = self.entries().collect();
        for (idx, entry) in entries.iter().enumerate() {
            let relations: Vec<_> = entry.relations().collect();

            // Check if this entry has multiple alternatives with our package
            let names: Vec<_> = relations.iter().filter_map(|r| r.try_name()).collect();
            if names.len() > 1 && names.contains(&package.to_string()) {
                // This is a complex alternative relation, mark for removal if obsolete
                let is_obsolete = relations.iter().any(|r| {
                    if r.try_name().as_deref() != Some(package) {
                        return false;
                    }
                    if let Some((vc, ver)) = r.version() {
                        matches!(vc, VersionConstraint::GreaterThan if &ver < minimum_version)
                            || matches!(vc, VersionConstraint::GreaterThanEqual if &ver <= minimum_version)
                    } else {
                        false
                    }
                });
                if is_obsolete {
                    obsolete_indices.push(idx);
                }
                continue;
            }

            // Single package entry
            if names.len() == 1 && names[0] == package {
                found = true;
                let relation = relations.into_iter().next().unwrap();

                // Check if update is needed
                let should_update = if let Some((vc, ver)) = relation.version() {
                    match vc {
                        VersionConstraint::GreaterThanEqual | VersionConstraint::GreaterThan => {
                            &ver < minimum_version
                        }
                        _ => false,
                    }
                } else {
                    true
                };

                if should_update {
                    update_idx = Some(idx);
                }
                break;
            }
        }

        // Perform updates after iteration
        if let Some(idx) = update_idx {
            let relation = Relation::new(
                package,
                Some((VersionConstraint::GreaterThanEqual, minimum_version.clone())),
            );
            // Get the existing entry and replace its relation to preserve formatting
            let mut entry = self.get_entry(idx).unwrap();
            entry.replace(0, relation);
            self.replace(idx, entry);
        }

        // Remove obsolete entries
        for idx in obsolete_indices.into_iter().rev() {
            self.remove_entry(idx);
        }

        // Add if not found
        if !found {
            let relation = Relation::new(
                package,
                Some((VersionConstraint::GreaterThanEqual, minimum_version.clone())),
            );
            self.push(Entry::from(relation));
        }
    }

    /// Ensure that a package has an exact version constraint.
    ///
    /// # Arguments
    /// * `package` - The package name
    /// * `version` - The exact version required
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let mut relations: Relations = "debhelper (>= 9)".parse().unwrap();
    /// relations.ensure_exact_version("debhelper", &"12".parse().unwrap());
    /// assert_eq!(relations.to_string(), "debhelper (= 12)");
    /// ```
    pub fn ensure_exact_version(&mut self, package: &str, version: &Version) {
        let mut found = false;
        let mut update_idx = None;

        let entries: Vec<_> = self.entries().collect();
        for (idx, entry) in entries.iter().enumerate() {
            let relations: Vec<_> = entry.relations().collect();
            let names: Vec<_> = relations.iter().filter_map(|r| r.try_name()).collect();

            if names.len() > 1 && names[0] == package {
                panic!("Complex rule for {}, aborting", package);
            }

            if names.len() == 1 && names[0] == package {
                found = true;
                let relation = relations.into_iter().next().unwrap();

                let should_update = if let Some((vc, ver)) = relation.version() {
                    vc != VersionConstraint::Equal || &ver != version
                } else {
                    true
                };

                if should_update {
                    update_idx = Some(idx);
                }
                break;
            }
        }

        // Perform update after iteration
        if let Some(idx) = update_idx {
            let relation =
                Relation::new(package, Some((VersionConstraint::Equal, version.clone())));
            // Get the existing entry and replace its relation to preserve formatting
            let mut entry = self.get_entry(idx).unwrap();
            entry.replace(0, relation);
            self.replace(idx, entry);
        }

        if !found {
            let relation =
                Relation::new(package, Some((VersionConstraint::Equal, version.clone())));
            self.push(Entry::from(relation));
        }
    }

    /// Ensure that a package dependency exists, without specifying a version.
    ///
    /// If the package already exists (with or without a version constraint),
    /// the relations field is left unchanged. Otherwise, the package is added
    /// without a version constraint.
    ///
    /// # Arguments
    /// * `package` - The package name
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let mut relations: Relations = "python3".parse().unwrap();
    /// relations.ensure_some_version("debhelper");
    /// assert!(relations.to_string().contains("debhelper"));
    /// ```
    pub fn ensure_some_version(&mut self, package: &str) {
        for entry in self.entries() {
            let relations: Vec<_> = entry.relations().collect();
            let names: Vec<_> = relations.iter().filter_map(|r| r.try_name()).collect();

            if names.len() > 1 && names[0] == package {
                panic!("Complex rule for {}, aborting", package);
            }

            if names.len() == 1 && names[0] == package {
                // Package already exists, don't modify
                return;
            }
        }

        // Package not found, add it
        let relation = Relation::simple(package);
        self.push(Entry::from(relation));
    }

    /// Ensure that a relation exists in the dependencies.
    ///
    /// This function checks if the provided entry is already satisfied by an
    /// existing entry. If it is, no changes are made. If an existing entry is
    /// weaker than the new entry (i.e., the new entry implies the existing one),
    /// the existing entry is replaced with the new one. Otherwise, the new entry
    /// is added.
    ///
    /// # Arguments
    /// * `new_entry` - The entry to ensure exists
    ///
    /// # Returns
    /// `true` if the entry was added or replaced, `false` if it was already satisfied
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relations, Entry};
    ///
    /// let mut relations: Relations = "python3".parse().unwrap();
    /// let new_entry: Entry = "debhelper (>= 12)".parse().unwrap();
    /// let added = relations.ensure_relation(new_entry);
    /// assert!(added);
    /// assert!(relations.to_string().contains("debhelper (>= 12)"));
    /// ```
    pub fn ensure_relation(&mut self, new_entry: Entry) -> bool {
        let mut to_replace: Vec<usize> = Vec::new();
        let mut to_remove: Vec<usize> = Vec::new();
        let mut already_satisfied = false;

        // Check existing entries
        for (idx, existing_entry) in self.entries().enumerate() {
            if new_entry.is_implied_by(&existing_entry) {
                // The new entry is already satisfied by an existing entry
                already_satisfied = true;
                break;
            }
            if existing_entry.is_implied_by(&new_entry) {
                // The new entry implies the existing one (is stronger)
                // We should replace/remove the weaker existing entry
                if to_replace.is_empty() {
                    to_replace.push(idx);
                } else {
                    to_remove.push(idx);
                }
            }
        }

        if already_satisfied {
            return false;
        }

        // Remove weaker entries in reverse order
        for idx in to_remove.into_iter().rev() {
            self.remove_entry(idx);
        }

        // Replace or add the entry
        if let Some(&idx) = to_replace.first() {
            self.replace(idx, new_entry);
        } else {
            self.add_dependency(new_entry, None);
        }

        true
    }

    /// Ensure that a substitution variable is present in the relations.
    ///
    /// If the substvar already exists, it is left unchanged. Otherwise, it is added
    /// at the end of the relations list.
    ///
    /// # Arguments
    /// * `substvar` - The substitution variable (e.g., "${misc:Depends}")
    ///
    /// # Returns
    /// `Ok(())` on success, or `Err` with an error message if parsing fails
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let mut relations: Relations = "python3".parse().unwrap();
    /// relations.ensure_substvar("${misc:Depends}").unwrap();
    /// assert_eq!(relations.to_string(), "python3, ${misc:Depends}");
    /// ```
    pub fn ensure_substvar(&mut self, substvar: &str) -> Result<(), String> {
        // Check if the substvar already exists
        for existing in self.substvars() {
            if existing.trim() == substvar.trim() {
                return Ok(());
            }
        }

        // Parse the substvar
        let (parsed, errors) = Relations::parse_relaxed(substvar, true);
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }

        // Detect whitespace pattern to preserve formatting
        let whitespace = self.detect_whitespace_pattern(" ");

        // Find the substvar node and inject it
        for substvar_node in parsed.0.children().filter(|n| n.kind() == SUBSTVAR) {
            let has_content = self.entries().next().is_some() || self.substvars().next().is_some();

            let mut builder = GreenNodeBuilder::new();
            builder.start_node(ROOT.into());

            // Copy existing content
            for child in self.0.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => inject(&mut builder, n),
                    NodeOrToken::Token(t) => builder.token(t.kind().into(), t.text()),
                }
            }

            // Add separator if needed, using detected whitespace pattern
            if has_content {
                builder.token(COMMA.into(), ",");
                builder.token(WHITESPACE.into(), whitespace.as_str());
            }

            // Inject the substvar node
            inject(&mut builder, substvar_node);

            builder.finish_node();
            self.0 = SyntaxNode::new_root_mut(builder.finish());
        }

        Ok(())
    }

    /// Remove a substitution variable from the relations.
    ///
    /// If the substvar exists, it is removed along with its surrounding separators.
    /// If the substvar does not exist, this is a no-op.
    ///
    /// # Arguments
    /// * `substvar` - The substitution variable to remove (e.g., "${misc:Depends}")
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let (mut relations, _) = Relations::parse_relaxed("python3, ${misc:Depends}", true);
    /// relations.drop_substvar("${misc:Depends}");
    /// assert_eq!(relations.to_string(), "python3");
    /// ```
    pub fn drop_substvar(&mut self, substvar: &str) {
        // Find all substvar nodes that match the given string
        let substvars_to_remove: Vec<_> = self
            .0
            .children()
            .filter_map(Substvar::cast)
            .filter(|s| s.to_string().trim() == substvar.trim())
            .collect();

        for substvar_node in substvars_to_remove {
            // Determine if this is the first substvar (no previous ENTRY or SUBSTVAR siblings)
            let is_first = !substvar_node
                .0
                .siblings(Direction::Prev)
                .skip(1)
                .any(|n| n.kind() == ENTRY || n.kind() == SUBSTVAR);

            let mut removed_comma = false;

            // Remove whitespace/comments and comma after the substvar
            while let Some(n) = substvar_node.0.next_sibling_or_token() {
                if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                    n.detach();
                } else if n.kind() == COMMA {
                    n.detach();
                    removed_comma = true;
                    break;
                } else {
                    break;
                }
            }

            // If not first, remove preceding whitespace/comments and comma
            if !is_first {
                while let Some(n) = substvar_node.0.prev_sibling_or_token() {
                    if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                        n.detach();
                    } else if !removed_comma && n.kind() == COMMA {
                        n.detach();
                        break;
                    } else {
                        break;
                    }
                }
            } else {
                // If first and we didn't remove a comma after, clean up any leading whitespace
                while let Some(n) = substvar_node.0.next_sibling_or_token() {
                    if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                        n.detach();
                    } else {
                        break;
                    }
                }
            }

            // Finally, detach the substvar node itself
            substvar_node.0.detach();
        }
    }

    /// Filter entries based on a predicate function.
    ///
    /// # Arguments
    /// * `keep` - A function that returns true for entries to keep
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let mut relations: Relations = "python3, debhelper, rustc".parse().unwrap();
    /// relations.filter_entries(|entry| {
    ///     entry.relations().any(|r| r.name().starts_with("python"))
    /// });
    /// assert_eq!(relations.to_string(), "python3");
    /// ```
    pub fn filter_entries<F>(&mut self, keep: F)
    where
        F: Fn(&Entry) -> bool,
    {
        let indices_to_remove: Vec<_> = self
            .entries()
            .enumerate()
            .filter_map(|(idx, entry)| if keep(&entry) { None } else { Some(idx) })
            .collect();

        // Remove in reverse order to maintain correct indices
        for idx in indices_to_remove.into_iter().rev() {
            self.remove_entry(idx);
        }
    }

    /// Check whether the relations are sorted according to a given sorting order.
    ///
    /// # Arguments
    /// * `sorting_order` - The sorting order to check against
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relations, WrapAndSortOrder};
    ///
    /// let relations: Relations = "debhelper, python3, rustc".parse().unwrap();
    /// assert!(relations.is_sorted(&WrapAndSortOrder));
    ///
    /// let relations: Relations = "rustc, debhelper, python3".parse().unwrap();
    /// assert!(!relations.is_sorted(&WrapAndSortOrder));
    /// ```
    pub fn is_sorted(&self, sorting_order: &impl SortingOrder) -> bool {
        let mut last_name: Option<String> = None;
        for entry in self.entries() {
            // Skip empty entries
            let mut relations = entry.relations();
            let Some(relation) = relations.next() else {
                continue;
            };

            let Some(name) = relation.try_name() else {
                continue;
            };

            // Skip items that should be ignored
            if sorting_order.ignore(&name) {
                continue;
            }

            // Check if this breaks the sort order
            if let Some(ref last) = last_name {
                if sorting_order.lt(&name, last) {
                    return false;
                }
            }

            last_name = Some(name);
        }
        true
    }

    /// Find the position to insert an entry while maintaining sort order.
    ///
    /// This method detects the current sorting order and returns the appropriate
    /// insertion position. If there are fewer than 2 entries, it defaults to
    /// WrapAndSortOrder. If no sorting order is detected, it returns the end position.
    ///
    /// # Arguments
    /// * `entry` - The entry to insert
    ///
    /// # Returns
    /// The index where the entry should be inserted
    fn find_insert_position(&self, entry: &Entry) -> usize {
        // Get the package name from the first relation in the entry
        let Some(relation) = entry.relations().next() else {
            // Empty entry, just append at the end
            return self.len();
        };
        let Some(package_name) = relation.try_name() else {
            return self.len();
        };

        // Count non-empty entries
        let count = self.entries().filter(|e| !e.is_empty()).count();

        // If there are less than 2 items, default to WrapAndSortOrder
        let sorting_order: Box<dyn SortingOrder> = if count < 2 {
            Box::new(WrapAndSortOrder)
        } else {
            // Try to detect which sorting order is being used
            // Try WrapAndSortOrder first, then DefaultSortingOrder
            if self.is_sorted(&WrapAndSortOrder) {
                Box::new(WrapAndSortOrder)
            } else if self.is_sorted(&DefaultSortingOrder) {
                Box::new(DefaultSortingOrder)
            } else {
                // No sorting order detected, just append at the end
                return self.len();
            }
        };

        // If adding a special item that should be ignored by this sort order, append at the end
        if sorting_order.ignore(&package_name) {
            return self.len();
        }

        // Insert in sorted order among regular items
        let mut position = 0;
        for (idx, existing_entry) in self.entries().enumerate() {
            let mut existing_relations = existing_entry.relations();
            let Some(existing_relation) = existing_relations.next() else {
                // Empty entry, skip
                position += 1;
                continue;
            };

            let Some(existing_name) = existing_relation.try_name() else {
                position += 1;
                continue;
            };

            // Skip special items when finding insertion position
            if sorting_order.ignore(&existing_name) {
                position += 1;
                continue;
            }

            // Compare with regular items only
            if sorting_order.lt(&package_name, &existing_name) {
                return idx;
            }
            position += 1;
        }

        position
    }

    /// Drop a dependency from the relations by package name.
    ///
    /// # Arguments
    /// * `package` - The package name to remove
    ///
    /// # Returns
    /// `true` if the package was found and removed, `false` otherwise
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let mut relations: Relations = "python3, debhelper, rustc".parse().unwrap();
    /// assert!(relations.drop_dependency("debhelper"));
    /// assert_eq!(relations.to_string(), "python3, rustc");
    /// assert!(!relations.drop_dependency("nonexistent"));
    /// ```
    pub fn drop_dependency(&mut self, package: &str) -> bool {
        let indices_to_remove: Vec<_> = self
            .entries()
            .enumerate()
            .filter_map(|(idx, entry)| {
                let relations: Vec<_> = entry.relations().collect();
                let names: Vec<_> = relations.iter().filter_map(|r| r.try_name()).collect();
                if names == vec![package] {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();

        let found = !indices_to_remove.is_empty();

        // Remove in reverse order to maintain correct indices
        for idx in indices_to_remove.into_iter().rev() {
            self.remove_entry(idx);
        }

        found
    }

    /// Add a dependency at a specific position or auto-detect the position.
    ///
    /// If `position` is `None`, the position is automatically determined based
    /// on the detected sorting order. If a sorting order is detected, the entry
    /// is inserted in the appropriate position to maintain that order. Otherwise,
    /// it is appended at the end.
    ///
    /// # Arguments
    /// * `entry` - The entry to add
    /// * `position` - Optional position to insert at
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relations, Relation, Entry};
    ///
    /// let mut relations: Relations = "python3, rustc".parse().unwrap();
    /// let entry = Entry::from(Relation::simple("debhelper"));
    /// relations.add_dependency(entry, None);
    /// // debhelper is inserted in sorted order (if order is detected)
    /// ```
    pub fn add_dependency(&mut self, entry: Entry, position: Option<usize>) {
        let pos = position.unwrap_or_else(|| self.find_insert_position(&entry));
        self.insert(pos, entry);
    }

    /// Get the entry containing a specific package.
    ///
    /// This returns the first entry that contains exactly one relation with the
    /// specified package name (no alternatives).
    ///
    /// # Arguments
    /// * `package` - The package name to search for
    ///
    /// # Returns
    /// A tuple of (index, Entry) if found
    ///
    /// # Errors
    /// Returns `Err` with a message if the package is found in a complex rule (with alternatives)
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let relations: Relations = "python3, debhelper (>= 12), rustc".parse().unwrap();
    /// let (idx, entry) = relations.get_relation("debhelper").unwrap();
    /// assert_eq!(idx, 1);
    /// assert_eq!(entry.to_string(), "debhelper (>= 12)");
    /// ```
    pub fn get_relation(&self, package: &str) -> Result<(usize, Entry), String> {
        for (idx, entry) in self.entries().enumerate() {
            let relations: Vec<_> = entry.relations().collect();
            let names: Vec<_> = relations.iter().filter_map(|r| r.try_name()).collect();

            if names.len() > 1 && names.contains(&package.to_string()) {
                return Err(format!("Complex rule for {}, aborting", package));
            }

            if names.len() == 1 && names[0] == package {
                return Ok((idx, entry));
            }
        }
        Err(format!("Package {} not found", package))
    }

    /// Iterate over all entries containing a specific package.
    ///
    /// # Arguments
    /// * `package` - The package name to search for
    ///
    /// # Returns
    /// An iterator over tuples of (index, Entry)
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let relations: Relations = "python3 | python3-minimal, python3-dev".parse().unwrap();
    /// let entries: Vec<_> = relations.iter_relations_for("python3").collect();
    /// assert_eq!(entries.len(), 1);
    /// ```
    pub fn iter_relations_for(&self, package: &str) -> impl Iterator<Item = (usize, Entry)> + '_ {
        let package = package.to_string();
        self.entries().enumerate().filter(move |(_, entry)| {
            let names: Vec<_> = entry.relations().filter_map(|r| r.try_name()).collect();
            names.contains(&package)
        })
    }

    /// Check whether a package exists in the relations.
    ///
    /// # Arguments
    /// * `package` - The package name to search for
    ///
    /// # Returns
    /// `true` if the package is found, `false` otherwise
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relations;
    ///
    /// let relations: Relations = "python3, debhelper, rustc".parse().unwrap();
    /// assert!(relations.has_relation("debhelper"));
    /// assert!(!relations.has_relation("nonexistent"));
    /// ```
    pub fn has_relation(&self, package: &str) -> bool {
        self.entries().any(|entry| {
            entry
                .relations()
                .any(|r| r.try_name().as_deref() == Some(package))
        })
    }
}

impl From<Vec<Entry>> for Relations {
    fn from(entries: Vec<Entry>) -> Self {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ROOT.into());
        for (i, entry) in entries.into_iter().enumerate() {
            if i > 0 {
                builder.token(COMMA.into(), ",");
                builder.token(WHITESPACE.into(), " ");
            }
            inject(&mut builder, entry.0);
        }
        builder.finish_node();
        Relations(SyntaxNode::new_root_mut(builder.finish()))
    }
}

impl From<Entry> for Relations {
    fn from(entry: Entry) -> Self {
        Self::from(vec![entry])
    }
}

impl Default for Entry {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Entry {}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let mut rels_a = self.relations();
        let mut rels_b = other.relations();
        while let (Some(a), Some(b)) = (rels_a.next(), rels_b.next()) {
            match a.cmp(&b) {
                std::cmp::Ordering::Equal => continue,
                x => return x,
            }
        }

        if rels_a.next().is_some() {
            return std::cmp::Ordering::Greater;
        }

        if rels_b.next().is_some() {
            return std::cmp::Ordering::Less;
        }

        std::cmp::Ordering::Equal
    }
}

impl Entry {
    /// Create a new entry
    pub fn new() -> Self {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::ENTRY.into());
        builder.finish_node();
        Entry(SyntaxNode::new_root_mut(builder.finish()))
    }

    /// Replace the relation at the given index
    pub fn replace(&mut self, idx: usize, relation: Relation) {
        let current_relation = self.get_relation(idx).unwrap();

        let old_root = current_relation.0;
        let new_root = relation.0;
        // Preserve white the current relation has
        let mut prev = new_root.first_child_or_token();
        let mut new_head_len = 0;
        // First, strip off any whitespace from the new relation
        while let Some(p) = prev {
            if p.kind() == WHITESPACE || p.kind() == NEWLINE {
                new_head_len += 1;
                prev = p.next_sibling_or_token();
            } else {
                break;
            }
        }
        let mut new_tail_len = 0;
        let mut next = new_root.last_child_or_token();
        while let Some(n) = next {
            if n.kind() == WHITESPACE || n.kind() == NEWLINE {
                new_tail_len += 1;
                next = n.prev_sibling_or_token();
            } else {
                break;
            }
        }
        // Then, inherit the whitespace from the old relation
        let mut prev = old_root.first_child_or_token();
        let mut old_head = vec![];
        while let Some(p) = prev {
            if p.kind() == WHITESPACE || p.kind() == NEWLINE {
                old_head.push(p.clone());
                prev = p.next_sibling_or_token();
            } else {
                break;
            }
        }
        let mut old_tail = vec![];
        let mut next = old_root.last_child_or_token();
        while let Some(n) = next {
            if n.kind() == WHITESPACE || n.kind() == NEWLINE {
                old_tail.push(n.clone());
                next = n.prev_sibling_or_token();
            } else {
                break;
            }
        }
        new_root.splice_children(0..new_head_len, old_head);
        let tail_pos = new_root.children_with_tokens().count() - new_tail_len;
        new_root.splice_children(
            tail_pos - new_tail_len..tail_pos,
            old_tail.into_iter().rev(),
        );
        let index = old_root.index();
        self.0
            .splice_children(index..index + 1, vec![new_root.into()]);
    }

    /// Wrap and sort the relations in this entry
    #[must_use]
    pub fn wrap_and_sort(&self) -> Self {
        let mut relations = self
            .relations()
            .map(|r| r.wrap_and_sort())
            .collect::<Vec<_>>();
        // TODO: preserve comments
        relations.sort();
        Self::from(relations)
    }

    /// Iterate over the relations in this entry
    pub fn relations(&self) -> impl Iterator<Item = Relation> + '_ {
        self.0.children().filter_map(Relation::cast)
    }

    /// Iterate over the relations in this entry
    pub fn iter(&self) -> impl Iterator<Item = Relation> + '_ {
        self.relations()
    }

    /// Get the relation at the given index
    pub fn get_relation(&self, idx: usize) -> Option<Relation> {
        self.relations().nth(idx)
    }

    /// Remove the relation at the given index
    ///
    /// # Arguments
    /// * `idx` - The index of the relation to remove
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relation,Entry};
    /// let mut entry: Entry = r"python3-dulwich (>= 0.19.0) | python3-requests".parse().unwrap();
    /// entry.remove_relation(1);
    /// assert_eq!(entry.to_string(), "python3-dulwich (>= 0.19.0)");
    /// ```
    pub fn remove_relation(&self, idx: usize) -> Relation {
        let mut relation = self.get_relation(idx).unwrap();
        relation.remove();
        relation
    }

    /// Check if this entry is satisfied by the given package versions.
    ///
    /// # Arguments
    /// * `package_version` - A function that returns the version of a package.
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relation,Entry};
    /// let entry = Entry::from(vec!["samba (>= 2.0)".parse::<Relation>().unwrap()]);
    /// assert!(entry.satisfied_by(|name: &str| -> Option<debversion::Version> {
    ///    match name {
    ///    "samba" => Some("2.0".parse().unwrap()),
    ///    _ => None
    /// }}));
    /// ```
    pub fn satisfied_by(&self, package_version: impl crate::VersionLookup + Copy) -> bool {
        self.relations().any(|r| {
            let Some(name) = r.try_name() else {
                return false;
            };
            let actual = package_version.lookup_version(name.as_str());
            if let Some((vc, version)) = r.version() {
                if let Some(actual) = actual {
                    match vc {
                        VersionConstraint::GreaterThanEqual => *actual >= version,
                        VersionConstraint::LessThanEqual => *actual <= version,
                        VersionConstraint::Equal => *actual == version,
                        VersionConstraint::GreaterThan => *actual > version,
                        VersionConstraint::LessThan => *actual < version,
                    }
                } else {
                    false
                }
            } else {
                actual.is_some()
            }
        })
    }

    /// Remove this entry
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relations,Entry};
    /// let mut relations: Relations = r"python3-dulwich (>= 0.19.0), python3-urllib3 (<< 1.26.0)".parse().unwrap();
    /// let mut entry = relations.get_entry(0).unwrap();
    /// entry.remove();
    /// assert_eq!(relations.to_string(), "python3-urllib3 (<< 1.26.0)");
    /// ```
    pub fn remove(&mut self) {
        let mut removed_comma = false;
        let is_first = !self
            .0
            .siblings(Direction::Prev)
            .skip(1)
            .any(|n| n.kind() == ENTRY);
        while let Some(n) = self.0.next_sibling_or_token() {
            if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                n.detach();
            } else if n.kind() == COMMA {
                n.detach();
                removed_comma = true;
                break;
            } else {
                panic!("Unexpected node: {:?}", n);
            }
        }
        if !is_first {
            while let Some(n) = self.0.prev_sibling_or_token() {
                if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                    n.detach();
                } else if !removed_comma && n.kind() == COMMA {
                    n.detach();
                    break;
                } else {
                    break;
                }
            }
        } else {
            while let Some(n) = self.0.next_sibling_or_token() {
                if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                    n.detach();
                } else {
                    break;
                }
            }
        }
        self.0.detach();
    }

    /// Check if this entry is empty
    pub fn is_empty(&self) -> bool {
        self.relations().count() == 0
    }

    /// Get the number of relations in this entry
    pub fn len(&self) -> usize {
        self.relations().count()
    }

    /// Push a new relation to the entry
    ///
    /// # Arguments
    /// * `relation` - The relation to push
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relation,Entry};
    /// let mut entry: Entry = "samba (>= 2.0)".parse().unwrap();
    /// entry.push("python3-requests".parse().unwrap());
    /// assert_eq!(entry.to_string(), "samba (>= 2.0) | python3-requests");
    /// ```
    pub fn push(&mut self, relation: Relation) {
        let is_empty = !self
            .0
            .children_with_tokens()
            .any(|n| n.kind() == PIPE || n.kind() == RELATION);

        let (position, new_children) = if let Some(current_relation) = self.relations().last() {
            let to_insert: Vec<NodeOrToken<GreenNode, GreenToken>> = if is_empty {
                vec![relation.0.green().into()]
            } else {
                vec![
                    NodeOrToken::Token(GreenToken::new(WHITESPACE.into(), " ")),
                    NodeOrToken::Token(GreenToken::new(PIPE.into(), "|")),
                    NodeOrToken::Token(GreenToken::new(WHITESPACE.into(), " ")),
                    relation.0.green().into(),
                ]
            };

            (current_relation.0.index() + 1, to_insert)
        } else {
            let child_count = self.0.children_with_tokens().count();
            (
                child_count,
                if is_empty {
                    vec![relation.0.green().into()]
                } else {
                    vec![
                        NodeOrToken::Token(GreenToken::new(PIPE.into(), "|")),
                        NodeOrToken::Token(GreenToken::new(WHITESPACE.into(), " ")),
                        relation.0.green().into(),
                    ]
                },
            )
        };

        let new_root = SyntaxNode::new_root_mut(
            self.0.replace_with(
                self.0
                    .green()
                    .splice_children(position..position, new_children),
            ),
        );

        if let Some(parent) = self.0.parent() {
            parent.splice_children(self.0.index()..self.0.index() + 1, vec![new_root.into()]);
            self.0 = parent
                .children_with_tokens()
                .nth(self.0.index())
                .unwrap()
                .clone()
                .into_node()
                .unwrap();
        } else {
            self.0 = new_root;
        }
    }

    /// Check if this entry (OR-group) is implied by another entry.
    ///
    /// An entry is implied by another if any of the relations in this entry
    /// is implied by any relation in the outer entry. This follows the semantics
    /// of OR-groups in Debian dependencies.
    ///
    /// For example:
    /// - `pkg >= 1.0` is implied by `pkg >= 1.5 | libc6` (first relation matches)
    /// - `pkg1 | pkg2` is implied by `pkg1` (pkg1 satisfies the requirement)
    ///
    /// # Arguments
    /// * `outer` - The outer entry that may imply this entry
    ///
    /// # Returns
    /// `true` if this entry is implied by `outer`, `false` otherwise
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Entry;
    ///
    /// let inner: Entry = "pkg (>= 1.0)".parse().unwrap();
    /// let outer: Entry = "pkg (>= 1.5) | libc6".parse().unwrap();
    /// assert!(inner.is_implied_by(&outer));
    /// ```
    pub fn is_implied_by(&self, outer: &Entry) -> bool {
        // If entries are identical, they imply each other
        if self == outer {
            return true;
        }

        // Check if any relation in inner is implied by any relation in outer
        for inner_rel in self.relations() {
            for outer_rel in outer.relations() {
                if inner_rel.is_implied_by(&outer_rel) {
                    return true;
                }
            }
        }

        false
    }
}

fn inject(builder: &mut GreenNodeBuilder, node: SyntaxNode) {
    builder.start_node(node.kind().into());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(child) => {
                inject(builder, child);
            }
            rowan::NodeOrToken::Token(token) => {
                builder.token(token.kind().into(), token.text());
            }
        }
    }
    builder.finish_node();
}

impl From<Vec<Relation>> for Entry {
    fn from(relations: Vec<Relation>) -> Self {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::ENTRY.into());
        for (i, relation) in relations.into_iter().enumerate() {
            if i > 0 {
                builder.token(WHITESPACE.into(), " ");
                builder.token(COMMA.into(), "|");
                builder.token(WHITESPACE.into(), " ");
            }
            inject(&mut builder, relation.0);
        }
        builder.finish_node();
        Entry(SyntaxNode::new_root_mut(builder.finish()))
    }
}

impl From<Relation> for Entry {
    fn from(relation: Relation) -> Self {
        Self::from(vec![relation])
    }
}

/// Helper function to tokenize a version string, handling epochs
/// Version strings like "1:2.3.2-2~" need to be split into: IDENT("1"), COLON, IDENT("2.3.2-2~")
fn tokenize_version(builder: &mut GreenNodeBuilder, version: &Version) {
    let version_str = version.to_string();

    // Split on the first colon (if any) to handle epochs
    if let Some(colon_pos) = version_str.find(':') {
        // Epoch part (before colon)
        builder.token(IDENT.into(), &version_str[..colon_pos]);
        builder.token(COLON.into(), ":");
        // Version part (after colon)
        builder.token(IDENT.into(), &version_str[colon_pos + 1..]);
    } else {
        // No epoch, just a regular version
        builder.token(IDENT.into(), version_str.as_str());
    }
}

impl Relation {
    /// Create a new relation
    ///
    /// # Arguments
    /// * `name` - The name of the package
    /// * `version_constraint` - The version constraint and version to use
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relation};
    /// use debian_control::relations::VersionConstraint;
    /// let relation = Relation::new("samba", Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())));
    /// assert_eq!(relation.to_string(), "samba (>= 2.0)");
    /// ```
    pub fn new(name: &str, version_constraint: Option<(VersionConstraint, Version)>) -> Self {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::RELATION.into());
        builder.token(IDENT.into(), name);
        if let Some((vc, version)) = version_constraint {
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(SyntaxKind::VERSION.into());
            builder.token(L_PARENS.into(), "(");
            builder.start_node(SyntaxKind::CONSTRAINT.into());
            for c in vc.to_string().chars() {
                builder.token(
                    match c {
                        '>' => R_ANGLE.into(),
                        '<' => L_ANGLE.into(),
                        '=' => EQUAL.into(),
                        _ => unreachable!(),
                    },
                    c.to_string().as_str(),
                );
            }
            builder.finish_node();

            builder.token(WHITESPACE.into(), " ");

            tokenize_version(&mut builder, &version);

            builder.token(R_PARENS.into(), ")");

            builder.finish_node();
        }

        builder.finish_node();
        Relation(SyntaxNode::new_root_mut(builder.finish()))
    }

    /// Wrap and sort this relation
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let relation = "  samba  (  >= 2.0) ".parse::<Relation>().unwrap();
    /// assert_eq!(relation.wrap_and_sort().to_string(), "samba (>= 2.0)");
    /// ```
    #[must_use]
    pub fn wrap_and_sort(&self) -> Self {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::RELATION.into());
        if let Some(name) = self.try_name() {
            builder.token(IDENT.into(), name.as_str());
        }
        if let Some(archqual) = self.archqual() {
            builder.token(COLON.into(), ":");
            builder.token(IDENT.into(), archqual.as_str());
        }
        if let Some((vc, version)) = self.version() {
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(SyntaxKind::VERSION.into());
            builder.token(L_PARENS.into(), "(");
            builder.start_node(SyntaxKind::CONSTRAINT.into());
            builder.token(
                match vc {
                    VersionConstraint::GreaterThanEqual => R_ANGLE.into(),
                    VersionConstraint::LessThanEqual => L_ANGLE.into(),
                    VersionConstraint::Equal => EQUAL.into(),
                    VersionConstraint::GreaterThan => R_ANGLE.into(),
                    VersionConstraint::LessThan => L_ANGLE.into(),
                },
                vc.to_string().as_str(),
            );
            builder.finish_node();
            builder.token(WHITESPACE.into(), " ");
            tokenize_version(&mut builder, &version);
            builder.token(R_PARENS.into(), ")");
            builder.finish_node();
        }
        if let Some(architectures) = self.architectures() {
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(ARCHITECTURES.into());
            builder.token(L_BRACKET.into(), "[");
            for (i, arch) in architectures.enumerate() {
                if i > 0 {
                    builder.token(WHITESPACE.into(), " ");
                }
                builder.token(IDENT.into(), arch.as_str());
            }
            builder.token(R_BRACKET.into(), "]");
            builder.finish_node();
        }
        for profiles in self.profiles() {
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(PROFILES.into());
            builder.token(L_ANGLE.into(), "<");
            for (i, profile) in profiles.into_iter().enumerate() {
                if i > 0 {
                    builder.token(WHITESPACE.into(), " ");
                }
                match profile {
                    BuildProfile::Disabled(name) => {
                        builder.token(NOT.into(), "!");
                        builder.token(IDENT.into(), name.as_str());
                    }
                    BuildProfile::Enabled(name) => {
                        builder.token(IDENT.into(), name.as_str());
                    }
                }
            }
            builder.token(R_ANGLE.into(), ">");
            builder.finish_node();
        }
        builder.finish_node();
        Relation(SyntaxNode::new_root_mut(builder.finish()))
    }

    /// Create a new simple relation, without any version constraints.
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let relation = Relation::simple("samba");
    /// assert_eq!(relation.to_string(), "samba");
    /// ```
    pub fn simple(name: &str) -> Self {
        Self::new(name, None)
    }

    /// Remove the version constraint from the relation.
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relation};
    /// use debian_control::relations::VersionConstraint;
    /// let mut relation = Relation::new("samba", Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())));
    /// relation.drop_constraint();
    /// assert_eq!(relation.to_string(), "samba");
    /// ```
    pub fn drop_constraint(&mut self) -> bool {
        let version_token = self.0.children().find(|n| n.kind() == VERSION);
        if let Some(version_token) = version_token {
            // Remove any whitespace before the version token
            while let Some(prev) = version_token.prev_sibling_or_token() {
                if prev.kind() == WHITESPACE || prev.kind() == NEWLINE {
                    prev.detach();
                } else {
                    break;
                }
            }
            version_token.detach();
            return true;
        }

        false
    }

    /// Return the name of the package in the relation, if present.
    ///
    /// Returns `None` for malformed relations that lack a package name
    /// (e.g. when substvars are parsed without substvar support).
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let relation = Relation::simple("samba");
    /// assert_eq!(relation.try_name(), Some("samba".to_string()));
    /// ```
    pub fn try_name(&self) -> Option<String> {
        self.name_token().map(|token| token.text().to_string())
    }

    /// Return the text range of the package-name token, if present.
    ///
    /// Useful for editor tooling that wants to highlight or attach metadata
    /// to the package name itself, separate from any version constraint or
    /// architecture qualifier.
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let relation: Relation = "samba (>= 4.0)".parse().unwrap();
    /// let range = relation.name_range().unwrap();
    /// assert_eq!(&relation.to_string()[range], "samba");
    /// ```
    pub fn name_range(&self) -> Option<rowan::TextRange> {
        self.name_token().map(|token| token.text_range())
    }

    fn name_token(&self) -> Option<crate::edit::relations::SyntaxToken> {
        self.0.children_with_tokens().find_map(|it| match it {
            SyntaxElement::Token(token) if token.kind() == IDENT => Some(token),
            _ => None,
        })
    }

    /// Return the name of the package in the relation.
    ///
    /// # Panics
    /// Panics if the relation has no package name (e.g. malformed input).
    /// Prefer [`try_name`](Self::try_name) for potentially malformed relations.
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let relation = Relation::simple("samba");
    /// assert_eq!(relation.name(), "samba");
    /// ```
    #[deprecated(
        since = "0.3.6",
        note = "Use try_name() instead, which returns Option<String>"
    )]
    pub fn name(&self) -> String {
        self.try_name().expect("Relation has no package name")
    }

    /// Return the archqual
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let relation: Relation = "samba:any".parse().unwrap();
    /// assert_eq!(relation.archqual(), Some("any".to_string()));
    /// ```
    pub fn archqual(&self) -> Option<String> {
        let archqual = self.0.children().find(|n| n.kind() == ARCHQUAL);
        let node = if let Some(archqual) = archqual {
            archqual.children_with_tokens().find_map(|it| match it {
                SyntaxElement::Token(token) if token.kind() == IDENT => Some(token),
                _ => None,
            })
        } else {
            None
        };
        node.map(|n| n.text().to_string())
    }

    /// Set the architecture qualifier for this relation.
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let mut relation = Relation::simple("samba");
    /// relation.set_archqual("any");
    /// assert_eq!(relation.to_string(), "samba:any");
    /// ```
    pub fn set_archqual(&mut self, archqual: &str) {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ARCHQUAL.into());
        builder.token(COLON.into(), ":");
        builder.token(IDENT.into(), archqual);
        builder.finish_node();

        let node_archqual = self.0.children().find(|n| n.kind() == ARCHQUAL);
        if let Some(node_archqual) = node_archqual {
            self.0.splice_children(
                node_archqual.index()..node_archqual.index() + 1,
                vec![SyntaxNode::new_root_mut(builder.finish()).into()],
            );
        } else {
            let name_node = self.0.children_with_tokens().find(|n| n.kind() == IDENT);
            let idx = if let Some(name_node) = name_node {
                name_node.index() + 1
            } else {
                0
            };
            self.0.splice_children(
                idx..idx,
                vec![SyntaxNode::new_root_mut(builder.finish()).into()],
            );
        }
    }

    /// Return the version constraint and the version it is constrained to.
    pub fn version(&self) -> Option<(VersionConstraint, Version)> {
        let vc = self.0.children().find(|n| n.kind() == VERSION);
        let vc = vc.as_ref()?;
        let constraint = vc.children().find(|n| n.kind() == CONSTRAINT);

        // Collect all IDENT and COLON tokens to handle versions with epochs (e.g., "1:2.3.2-2~")
        let version_str: String = vc
            .children_with_tokens()
            .filter_map(|it| match it {
                SyntaxElement::Token(token) if token.kind() == IDENT || token.kind() == COLON => {
                    Some(token.text().to_string())
                }
                _ => None,
            })
            .collect();

        if let Some(constraint) = constraint {
            if !version_str.is_empty() {
                let vc: VersionConstraint = constraint.to_string().parse().unwrap();
                Some((vc, version_str.parse().unwrap()))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Set the version constraint for this relation
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relation};
    /// use debian_control::relations::VersionConstraint;
    /// let mut relation = Relation::simple("samba");
    /// relation.set_version(Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())));
    /// assert_eq!(relation.to_string(), "samba (>= 2.0)");
    /// ```
    pub fn set_version(&mut self, version_constraint: Option<(VersionConstraint, Version)>) {
        let current_version = self.0.children().find(|n| n.kind() == VERSION);
        if let Some((vc, version)) = version_constraint {
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(VERSION.into());
            builder.token(L_PARENS.into(), "(");
            builder.start_node(CONSTRAINT.into());
            match vc {
                VersionConstraint::GreaterThanEqual => {
                    builder.token(R_ANGLE.into(), ">");
                    builder.token(EQUAL.into(), "=");
                }
                VersionConstraint::LessThanEqual => {
                    builder.token(L_ANGLE.into(), "<");
                    builder.token(EQUAL.into(), "=");
                }
                VersionConstraint::Equal => {
                    builder.token(EQUAL.into(), "=");
                }
                VersionConstraint::GreaterThan => {
                    builder.token(R_ANGLE.into(), ">");
                    builder.token(R_ANGLE.into(), ">");
                }
                VersionConstraint::LessThan => {
                    builder.token(L_ANGLE.into(), "<");
                    builder.token(L_ANGLE.into(), "<");
                }
            }
            builder.finish_node(); // CONSTRAINT
            builder.token(WHITESPACE.into(), " ");
            tokenize_version(&mut builder, &version);
            builder.token(R_PARENS.into(), ")");
            builder.finish_node(); // VERSION

            if let Some(current_version) = current_version {
                self.0.splice_children(
                    current_version.index()..current_version.index() + 1,
                    vec![SyntaxNode::new_root_mut(builder.finish()).into()],
                );
            } else {
                let name_node = self.0.children_with_tokens().find(|n| n.kind() == IDENT);
                let idx = if let Some(name_node) = name_node {
                    name_node.index() + 1
                } else {
                    0
                };
                let new_children = vec![
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                    builder.finish().into(),
                ];
                let new_root = SyntaxNode::new_root_mut(
                    self.0.green().splice_children(idx..idx, new_children),
                );
                if let Some(parent) = self.0.parent() {
                    parent
                        .splice_children(self.0.index()..self.0.index() + 1, vec![new_root.into()]);
                    self.0 = parent
                        .children_with_tokens()
                        .nth(self.0.index())
                        .unwrap()
                        .clone()
                        .into_node()
                        .unwrap();
                } else {
                    self.0 = new_root;
                }
            }
        } else if let Some(current_version) = current_version {
            // Remove any whitespace before the version token
            while let Some(prev) = current_version.prev_sibling_or_token() {
                if prev.kind() == WHITESPACE || prev.kind() == NEWLINE {
                    prev.detach();
                } else {
                    break;
                }
            }
            current_version.detach();
        }
    }

    /// Return an iterator over the architectures for this relation
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let relation: Relation = "samba [amd64]".parse().unwrap();
    /// assert_eq!(relation.architectures().unwrap().collect::<Vec<_>>(), vec!["amd64".to_string()]);
    /// ```
    pub fn architectures(&self) -> Option<impl Iterator<Item = String> + '_> {
        let architectures = self.0.children().find(|n| n.kind() == ARCHITECTURES)?;

        Some(architectures.children_with_tokens().filter_map(|node| {
            let token = node.as_token()?;
            if token.kind() == IDENT {
                Some(token.text().to_string())
            } else {
                None
            }
        }))
    }

    /// Returns an iterator over the build profiles for this relation
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relation};
    /// use debian_control::relations::{BuildProfile};
    /// let relation: Relation = "samba <!nocheck>".parse().unwrap();
    /// assert_eq!(relation.profiles().collect::<Vec<_>>(), vec![vec![BuildProfile::Disabled("nocheck".to_string())]]);
    /// ```
    pub fn profiles(&self) -> impl Iterator<Item = Vec<BuildProfile>> + '_ {
        let profiles = self.0.children().filter(|n| n.kind() == PROFILES);

        profiles.map(|profile| {
            // iterate over nodes separated by whitespace tokens
            let mut ret = vec![];
            let mut current = vec![];
            for token in profile.children_with_tokens() {
                match token.kind() {
                    WHITESPACE | NEWLINE => {
                        if !current.is_empty() {
                            ret.push(current.join("").parse::<BuildProfile>().unwrap());
                            current = vec![];
                        }
                    }
                    L_ANGLE | R_ANGLE => {}
                    _ => {
                        current.push(token.to_string());
                    }
                }
            }
            if !current.is_empty() {
                ret.push(current.concat().parse().unwrap());
            }
            ret
        })
    }

    /// Return an iterator over the text ranges of each build-profile name
    /// token (the IDENT inside `<...>` groups).
    ///
    /// Useful for tooling that wants to highlight, hover over, or navigate
    /// from a specific profile name. The leading `!` of disabled profiles is
    /// not included.
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let relation: Relation = "samba <!nocheck>".parse().unwrap();
    /// let ranges: Vec<_> = relation.profile_ranges().collect();
    /// assert_eq!(ranges.len(), 1);
    /// let r = ranges[0];
    /// assert_eq!(&relation.to_string()[r], "nocheck");
    /// ```
    pub fn profile_ranges(&self) -> impl Iterator<Item = rowan::TextRange> + '_ {
        self.0
            .children()
            .filter(|n| n.kind() == PROFILES)
            .flat_map(|profile| {
                profile
                    .children_with_tokens()
                    .filter_map(|t| t.into_token())
                    .filter(|t| t.kind() == IDENT)
                    .map(|t| t.text_range())
                    .collect::<Vec<_>>()
            })
    }

    /// Remove this relation
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::{Relation,Entry};
    /// let mut entry: Entry = r"python3-dulwich (>= 0.19.0) | python3-urllib3 (<< 1.26.0)".parse().unwrap();
    /// let mut relation = entry.get_relation(0).unwrap();
    /// relation.remove();
    /// assert_eq!(entry.to_string(), "python3-urllib3 (<< 1.26.0)");
    /// ```
    pub fn remove(&mut self) {
        let is_first = !self
            .0
            .siblings(Direction::Prev)
            .skip(1)
            .any(|n| n.kind() == RELATION);
        if !is_first {
            // Not the first item in the list. Remove whitespace backwards to the previous
            // pipe, the pipe and any whitespace until the previous relation
            while let Some(n) = self.0.prev_sibling_or_token() {
                if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                    n.detach();
                } else if n.kind() == PIPE {
                    n.detach();
                    break;
                } else {
                    break;
                }
            }
            while let Some(n) = self.0.prev_sibling_or_token() {
                if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                    n.detach();
                } else {
                    break;
                }
            }
        } else {
            // First item in the list. Remove whitespace up to the pipe, the pipe and anything
            // before the next relation
            while let Some(n) = self.0.next_sibling_or_token() {
                if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                    n.detach();
                } else if n.kind() == PIPE {
                    n.detach();
                    break;
                } else {
                    break;
                }
            }

            while let Some(n) = self.0.next_sibling_or_token() {
                if matches!(n.kind(), WHITESPACE | NEWLINE | COMMENT) {
                    n.detach();
                } else {
                    break;
                }
            }
        }
        // If this was the last relation in the entry, remove the entire entry
        if let Some(mut parent) = self.0.parent().and_then(Entry::cast) {
            if parent.is_empty() {
                parent.remove();
            } else {
                self.0.detach();
            }
        } else {
            self.0.detach();
        }
    }

    /// Set the architectures for this relation
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// let mut relation = Relation::simple("samba");
    /// relation.set_architectures(vec!["amd64", "i386"].into_iter());
    /// assert_eq!(relation.to_string(), "samba [amd64 i386]");
    /// ```
    pub fn set_architectures<'a>(&mut self, architectures: impl Iterator<Item = &'a str>) {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ARCHITECTURES.into());
        builder.token(L_BRACKET.into(), "[");
        for (i, arch) in architectures.enumerate() {
            if i > 0 {
                builder.token(WHITESPACE.into(), " ");
            }
            builder.token(IDENT.into(), arch);
        }
        builder.token(R_BRACKET.into(), "]");
        builder.finish_node();

        let node_architectures = self.0.children().find(|n| n.kind() == ARCHITECTURES);
        if let Some(node_architectures) = node_architectures {
            let new_root = SyntaxNode::new_root_mut(builder.finish());
            self.0.splice_children(
                node_architectures.index()..node_architectures.index() + 1,
                vec![new_root.into()],
            );
        } else {
            let profiles = self.0.children().find(|n| n.kind() == PROFILES);
            let idx = if let Some(profiles) = profiles {
                profiles.index()
            } else {
                self.0.children_with_tokens().count()
            };
            let new_root = SyntaxNode::new_root(self.0.green().splice_children(
                idx..idx,
                vec![
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                    builder.finish().into(),
                ],
            ));
            if let Some(parent) = self.0.parent() {
                parent.splice_children(self.0.index()..self.0.index() + 1, vec![new_root.into()]);
                self.0 = parent
                    .children_with_tokens()
                    .nth(self.0.index())
                    .unwrap()
                    .clone()
                    .into_node()
                    .unwrap();
            } else {
                self.0 = new_root;
            }
        }
    }

    /// Add a build profile to this relation
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// use debian_control::relations::BuildProfile;
    /// let mut relation = Relation::simple("samba");
    /// relation.add_profile(&[BuildProfile::Disabled("nocheck".to_string())]);
    /// assert_eq!(relation.to_string(), "samba <!nocheck>");
    /// ```
    pub fn add_profile(&mut self, profile: &[BuildProfile]) {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(PROFILES.into());
        builder.token(L_ANGLE.into(), "<");
        for (i, profile) in profile.iter().enumerate() {
            if i > 0 {
                builder.token(WHITESPACE.into(), " ");
            }
            match profile {
                BuildProfile::Disabled(name) => {
                    builder.token(NOT.into(), "!");
                    builder.token(IDENT.into(), name.as_str());
                }
                BuildProfile::Enabled(name) => {
                    builder.token(IDENT.into(), name.as_str());
                }
            }
        }
        builder.token(R_ANGLE.into(), ">");
        builder.finish_node();

        let node_profiles = self.0.children().find(|n| n.kind() == PROFILES);
        if let Some(node_profiles) = node_profiles {
            let new_root = SyntaxNode::new_root_mut(builder.finish());
            self.0.splice_children(
                node_profiles.index()..node_profiles.index() + 1,
                vec![new_root.into()],
            );
        } else {
            let idx = self.0.children_with_tokens().count();
            let new_root = SyntaxNode::new_root(self.0.green().splice_children(
                idx..idx,
                vec![
                    GreenToken::new(WHITESPACE.into(), " ").into(),
                    builder.finish().into(),
                ],
            ));
            if let Some(parent) = self.0.parent() {
                parent.splice_children(self.0.index()..self.0.index() + 1, vec![new_root.into()]);
                self.0 = parent
                    .children_with_tokens()
                    .nth(self.0.index())
                    .unwrap()
                    .clone()
                    .into_node()
                    .unwrap();
            } else {
                self.0 = new_root;
            }
        }
    }

    /// Build a new relation
    pub fn build(name: &str) -> RelationBuilder {
        RelationBuilder::new(name)
    }

    /// Check if this relation is implied by another relation.
    ///
    /// A relation is implied by another if the outer relation is more restrictive
    /// or equal to this relation. For example:
    /// - `pkg >= 1.0` is implied by `pkg >= 1.5` (outer is more restrictive)
    /// - `pkg >= 1.0` is implied by `pkg = 1.5` (outer is more restrictive)
    /// - `pkg` (no version) is implied by any versioned constraint on `pkg`
    ///
    /// # Arguments
    /// * `outer` - The outer relation that may imply this relation
    ///
    /// # Returns
    /// `true` if this relation is implied by `outer`, `false` otherwise
    ///
    /// # Example
    /// ```
    /// use debian_control::edit::relations::Relation;
    /// use debian_control::relations::VersionConstraint;
    ///
    /// let inner = Relation::new("pkg", Some((VersionConstraint::GreaterThanEqual, "1.0".parse().unwrap())));
    /// let outer = Relation::new("pkg", Some((VersionConstraint::GreaterThanEqual, "1.5".parse().unwrap())));
    /// assert!(inner.is_implied_by(&outer));
    ///
    /// let inner2 = Relation::new("pkg", None);
    /// assert!(inner2.is_implied_by(&outer));
    /// ```
    pub fn is_implied_by(&self, outer: &Relation) -> bool {
        if self.try_name() != outer.try_name() {
            return false;
        }

        let inner_version = self.version();
        let outer_version = outer.version();

        // No version constraint on inner means it's always implied
        if inner_version.is_none() {
            return true;
        }

        // If versions are identical, they imply each other
        if inner_version == outer_version {
            return true;
        }

        // Inner has version but outer doesn't - not implied
        if outer_version.is_none() {
            return false;
        }

        let (inner_constraint, inner_ver) = inner_version.unwrap();
        let (outer_constraint, outer_ver) = outer_version.unwrap();

        use VersionConstraint::*;
        match inner_constraint {
            GreaterThanEqual => match outer_constraint {
                GreaterThan => outer_ver > inner_ver,
                GreaterThanEqual | Equal => outer_ver >= inner_ver,
                LessThan | LessThanEqual => false,
            },
            Equal => match outer_constraint {
                Equal => outer_ver == inner_ver,
                _ => false,
            },
            LessThan => match outer_constraint {
                LessThan => outer_ver <= inner_ver,
                LessThanEqual | Equal => outer_ver < inner_ver,
                GreaterThan | GreaterThanEqual => false,
            },
            LessThanEqual => match outer_constraint {
                LessThanEqual | Equal | LessThan => outer_ver <= inner_ver,
                GreaterThan | GreaterThanEqual => false,
            },
            GreaterThan => match outer_constraint {
                GreaterThan => outer_ver >= inner_ver,
                Equal | GreaterThanEqual => outer_ver > inner_ver,
                LessThan | LessThanEqual => false,
            },
        }
    }
}

/// A builder for creating a `Relation`
///
/// # Example
/// ```
/// use debian_control::edit::relations::{Relation};
/// use debian_control::relations::VersionConstraint;
/// let relation = Relation::build("samba")
///    .version_constraint(VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())
///    .archqual("any")
///    .architectures(vec!["amd64".to_string(), "i386".to_string()])
///    .build();
/// assert_eq!(relation.to_string(), "samba:any (>= 2.0) [amd64 i386]");
/// ```
pub struct RelationBuilder {
    name: String,
    version_constraint: Option<(VersionConstraint, Version)>,
    archqual: Option<String>,
    architectures: Option<Vec<String>>,
    profiles: Vec<Vec<BuildProfile>>,
}

impl RelationBuilder {
    /// Create a new `RelationBuilder` with the given package name
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version_constraint: None,
            archqual: None,
            architectures: None,
            profiles: vec![],
        }
    }

    /// Set the version constraint for this relation
    pub fn version_constraint(mut self, vc: VersionConstraint, version: Version) -> Self {
        self.version_constraint = Some((vc, version));
        self
    }

    /// Set the architecture qualifier for this relation
    pub fn archqual(mut self, archqual: &str) -> Self {
        self.archqual = Some(archqual.to_string());
        self
    }

    /// Set the architectures for this relation
    pub fn architectures(mut self, architectures: Vec<String>) -> Self {
        self.architectures = Some(architectures);
        self
    }

    /// Set the build profiles for this relation
    pub fn profiles(mut self, profiles: Vec<Vec<BuildProfile>>) -> Self {
        self.profiles = profiles;
        self
    }

    /// Add a build profile to this relation
    pub fn add_profile(mut self, profile: Vec<BuildProfile>) -> Self {
        self.profiles.push(profile);
        self
    }

    /// Build the `Relation`
    pub fn build(self) -> Relation {
        let mut relation = Relation::new(&self.name, self.version_constraint);
        if let Some(archqual) = &self.archqual {
            relation.set_archqual(archqual);
        }
        if let Some(architectures) = &self.architectures {
            relation.set_architectures(architectures.iter().map(|s| s.as_str()));
        }
        for profile in &self.profiles {
            relation.add_profile(profile);
        }
        relation
    }
}

impl PartialOrd for Relation {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Relation {}

impl Ord for Relation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare by name first, then by version
        let name_cmp = self.try_name().cmp(&other.try_name());
        if name_cmp != std::cmp::Ordering::Equal {
            return name_cmp;
        }

        let self_version = self.version();
        let other_version = other.version();

        match (self_version, other_version) {
            (Some((self_vc, self_version)), Some((other_vc, other_version))) => {
                let vc_cmp = self_vc.cmp(&other_vc);
                if vc_cmp != std::cmp::Ordering::Equal {
                    return vc_cmp;
                }

                self_version.cmp(&other_version)
            }
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }
}

impl std::str::FromStr for Relations {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parse = parse(s, false);
        if parse.errors.is_empty() {
            Ok(parse.root_mut())
        } else {
            Err(parse.errors.join("\n"))
        }
    }
}

impl std::str::FromStr for Entry {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let root: Relations = s.parse()?;

        let mut entries = root.entries();
        let entry = if let Some(entry) = entries.next() {
            entry
        } else {
            return Err("No entry found".to_string());
        };

        if entries.next().is_some() {
            return Err("Multiple entries found".to_string());
        }

        Ok(entry)
    }
}

impl std::str::FromStr for Relation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let entry: Entry = s.parse()?;

        let mut relations = entry.relations();
        let relation = if let Some(relation) = relations.next() {
            relation
        } else {
            return Err("No relation found".to_string());
        };

        if relations.next().is_some() {
            return Err("Multiple relations found".to_string());
        }

        Ok(relation)
    }
}

impl From<crate::lossy::Relation> for Relation {
    fn from(relation: crate::lossy::Relation) -> Self {
        let mut builder = Relation::build(&relation.name);

        if let Some((vc, version)) = relation.version {
            builder = builder.version_constraint(vc, version);
        }

        if let Some(archqual) = relation.archqual {
            builder = builder.archqual(&archqual);
        }

        if let Some(architectures) = relation.architectures {
            builder = builder.architectures(architectures);
        }

        builder = builder.profiles(relation.profiles);

        builder.build()
    }
}

impl From<Relation> for crate::lossy::Relation {
    fn from(relation: Relation) -> Self {
        crate::lossy::Relation {
            name: relation.try_name().unwrap_or_default(),
            version: relation.version(),
            archqual: relation.archqual(),
            architectures: relation.architectures().map(|a| a.collect()),
            profiles: relation.profiles().collect(),
        }
    }
}

impl From<Entry> for Vec<crate::lossy::Relation> {
    fn from(entry: Entry) -> Self {
        entry.relations().map(|r| r.into()).collect()
    }
}

impl From<Vec<crate::lossy::Relation>> for Entry {
    fn from(relations: Vec<crate::lossy::Relation>) -> Self {
        let relations: Vec<Relation> = relations.into_iter().map(|r| r.into()).collect();
        Entry::from(relations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        let input = "python3-dulwich";
        let parsed: Relations = input.parse().unwrap();
        assert_eq!(parsed.to_string(), input);
        assert_eq!(parsed.entries().count(), 1);
        let entry = parsed.entries().next().unwrap();
        assert_eq!(entry.to_string(), "python3-dulwich");
        assert_eq!(entry.relations().count(), 1);
        let relation = entry.relations().next().unwrap();
        assert_eq!(relation.to_string(), "python3-dulwich");
        assert_eq!(relation.version(), None);

        let input = "python3-dulwich (>= 0.20.21)";
        let parsed: Relations = input.parse().unwrap();
        assert_eq!(parsed.to_string(), input);
        assert_eq!(parsed.entries().count(), 1);
        let entry = parsed.entries().next().unwrap();
        assert_eq!(entry.to_string(), "python3-dulwich (>= 0.20.21)");
        assert_eq!(entry.relations().count(), 1);
        let relation = entry.relations().next().unwrap();
        assert_eq!(relation.to_string(), "python3-dulwich (>= 0.20.21)");
        assert_eq!(
            relation.version(),
            Some((
                VersionConstraint::GreaterThanEqual,
                "0.20.21".parse().unwrap()
            ))
        );
    }

    #[test]
    fn test_multiple() {
        let input = "python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)";
        let parsed: Relations = input.parse().unwrap();
        assert_eq!(parsed.to_string(), input);
        assert_eq!(parsed.entries().count(), 2);
        let entry = parsed.entries().next().unwrap();
        assert_eq!(entry.to_string(), "python3-dulwich (>= 0.20.21)");
        assert_eq!(entry.relations().count(), 1);
        let relation = entry.relations().next().unwrap();
        assert_eq!(relation.to_string(), "python3-dulwich (>= 0.20.21)");
        assert_eq!(
            relation.version(),
            Some((
                VersionConstraint::GreaterThanEqual,
                "0.20.21".parse().unwrap()
            ))
        );
        let entry = parsed.entries().nth(1).unwrap();
        assert_eq!(entry.to_string(), "python3-dulwich (<< 0.21)");
        assert_eq!(entry.relations().count(), 1);
        let relation = entry.relations().next().unwrap();
        assert_eq!(relation.to_string(), "python3-dulwich (<< 0.21)");
        assert_eq!(
            relation.version(),
            Some((VersionConstraint::LessThan, "0.21".parse().unwrap()))
        );
    }

    #[test]
    fn test_architectures() {
        let input = "python3-dulwich [amd64 arm64 armhf i386 mips mips64el mipsel ppc64el s390x]";
        let parsed: Relations = input.parse().unwrap();
        assert_eq!(parsed.to_string(), input);
        assert_eq!(parsed.entries().count(), 1);
        let entry = parsed.entries().next().unwrap();
        assert_eq!(
            entry.to_string(),
            "python3-dulwich [amd64 arm64 armhf i386 mips mips64el mipsel ppc64el s390x]"
        );
        assert_eq!(entry.relations().count(), 1);
        let relation = entry.relations().next().unwrap();
        assert_eq!(
            relation.to_string(),
            "python3-dulwich [amd64 arm64 armhf i386 mips mips64el mipsel ppc64el s390x]"
        );
        assert_eq!(relation.version(), None);
        assert_eq!(
            relation.architectures().unwrap().collect::<Vec<_>>(),
            vec![
                "amd64", "arm64", "armhf", "i386", "mips", "mips64el", "mipsel", "ppc64el", "s390x"
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_profiles() {
        let input = "foo (>= 1.0) [i386 arm] <!nocheck> <!cross>, bar";
        let parsed: Relations = input.parse().unwrap();
        assert_eq!(parsed.to_string(), input);
        assert_eq!(parsed.entries().count(), 2);
        let entry = parsed.entries().next().unwrap();
        assert_eq!(
            entry.to_string(),
            "foo (>= 1.0) [i386 arm] <!nocheck> <!cross>"
        );
        assert_eq!(entry.relations().count(), 1);
        let relation = entry.relations().next().unwrap();
        assert_eq!(
            relation.to_string(),
            "foo (>= 1.0) [i386 arm] <!nocheck> <!cross>"
        );
        assert_eq!(
            relation.version(),
            Some((VersionConstraint::GreaterThanEqual, "1.0".parse().unwrap()))
        );
        assert_eq!(
            relation.architectures().unwrap().collect::<Vec<_>>(),
            vec!["i386", "arm"]
                .into_iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            relation.profiles().collect::<Vec<_>>(),
            vec![
                vec![BuildProfile::Disabled("nocheck".to_string())],
                vec![BuildProfile::Disabled("cross".to_string())]
            ]
        );
    }

    #[test]
    fn test_substvar() {
        let input = "${shlibs:Depends}";

        let (parsed, errors) = Relations::parse_relaxed(input, true);
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(parsed.to_string(), input);
        assert_eq!(parsed.entries().count(), 0);

        assert_eq!(
            parsed.substvars().collect::<Vec<_>>(),
            vec!["${shlibs:Depends}"]
        );
    }

    #[test]
    fn test_substvar_nodes() {
        let input = "foo, ${shlibs:Depends}, bar, ${misc:Depends}";

        let (parsed, errors) = Relations::parse_relaxed(input, true);
        assert_eq!(errors, Vec::<String>::new());

        let substvar_nodes: Vec<Substvar> = parsed.substvar_nodes().collect();
        assert_eq!(substvar_nodes.len(), 2);
        assert_eq!(substvar_nodes[0].to_string(), "${shlibs:Depends}");
        assert_eq!(substvar_nodes[1].to_string(), "${misc:Depends}");
    }

    #[test]
    fn test_substvar_nodes_empty() {
        let parsed: Relations = "foo, bar".parse().unwrap();

        let substvar_nodes: Vec<Substvar> = parsed.substvar_nodes().collect();
        assert_eq!(substvar_nodes.len(), 0);
    }

    #[test]
    fn test_new() {
        let r = Relation::new(
            "samba",
            Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())),
        );

        assert_eq!(r.to_string(), "samba (>= 2.0)");
    }

    #[test]
    fn test_drop_constraint() {
        let mut r = Relation::new(
            "samba",
            Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())),
        );

        r.drop_constraint();

        assert_eq!(r.to_string(), "samba");
    }

    #[test]
    fn test_simple() {
        let r = Relation::simple("samba");

        assert_eq!(r.to_string(), "samba");
    }

    #[test]
    fn test_remove_first_entry() {
        let mut rels: Relations = r#"python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"#
            .parse()
            .unwrap();
        let removed = rels.remove_entry(0);
        assert_eq!(removed.to_string(), "python3-dulwich (>= 0.20.21)");
        assert_eq!(rels.to_string(), "python3-dulwich (<< 0.21)");
    }

    #[test]
    fn test_remove_last_entry() {
        let mut rels: Relations = r#"python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"#
            .parse()
            .unwrap();
        rels.remove_entry(1);
        assert_eq!(rels.to_string(), "python3-dulwich (>= 0.20.21)");
    }

    #[test]
    fn test_remove_middle() {
        let mut rels: Relations =
            r#"python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21), python3-dulwich (<< 0.22)"#
                .parse()
                .unwrap();
        rels.remove_entry(1);
        assert_eq!(
            rels.to_string(),
            "python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.22)"
        );
    }

    #[test]
    fn test_remove_added() {
        let mut rels: Relations = r#"python3-dulwich (>= 0.20.21)"#.parse().unwrap();
        let entry = Entry::from(vec![Relation::simple("python3-dulwich")]);
        rels.push(entry);
        rels.remove_entry(1);
        assert_eq!(rels.to_string(), "python3-dulwich (>= 0.20.21)");
    }

    #[test]
    fn test_push() {
        let mut rels: Relations = r#"python3-dulwich (>= 0.20.21)"#.parse().unwrap();
        let entry = Entry::from(vec![Relation::simple("python3-dulwich")]);
        rels.push(entry);
        assert_eq!(
            rels.to_string(),
            "python3-dulwich (>= 0.20.21), python3-dulwich"
        );
    }

    #[test]
    fn test_insert_with_custom_separator() {
        let mut rels: Relations = "python3".parse().unwrap();
        let entry = Entry::from(vec![Relation::simple("debhelper")]);
        rels.insert_with_separator(1, entry, Some("\n "));
        assert_eq!(rels.to_string(), "python3,\n debhelper");
    }

    #[test]
    fn test_insert_with_wrap_and_sort_separator() {
        let mut rels: Relations = "python3".parse().unwrap();
        let entry = Entry::from(vec![Relation::simple("rustc")]);
        // Simulate wrap-and-sort -a style with field name "Depends: " (9 chars)
        rels.insert_with_separator(1, entry, Some("\n         "));
        assert_eq!(rels.to_string(), "python3,\n         rustc");
    }

    #[test]
    fn test_push_from_empty() {
        let mut rels: Relations = "".parse().unwrap();
        let entry = Entry::from(vec![Relation::simple("python3-dulwich")]);
        rels.push(entry);
        assert_eq!(rels.to_string(), "python3-dulwich");
    }

    #[test]
    fn test_insert() {
        let mut rels: Relations = r#"python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"#
            .parse()
            .unwrap();
        let entry = Entry::from(vec![Relation::simple("python3-dulwich")]);
        rels.insert(1, entry);
        assert_eq!(
            rels.to_string(),
            "python3-dulwich (>= 0.20.21), python3-dulwich, python3-dulwich (<< 0.21)"
        );
    }

    #[test]
    fn test_insert_at_start() {
        let mut rels: Relations = r#"python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"#
            .parse()
            .unwrap();
        let entry = Entry::from(vec![Relation::simple("python3-dulwich")]);
        rels.insert(0, entry);
        assert_eq!(
            rels.to_string(),
            "python3-dulwich, python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"
        );
    }

    #[test]
    fn test_insert_after_error() {
        let (mut rels, errors) = Relations::parse_relaxed("@foo@, debhelper (>= 1.0)", false);
        assert_eq!(
            errors,
            vec![
                "expected $ or identifier but got ERROR",
                "expected comma or end of file but got Some(IDENT)",
                "expected $ or identifier but got ERROR"
            ]
        );
        let entry = Entry::from(vec![Relation::simple("bar")]);
        rels.push(entry);
        assert_eq!(rels.to_string(), "@foo@, debhelper (>= 1.0), bar");
    }

    #[test]
    fn test_insert_before_error() {
        let (mut rels, errors) = Relations::parse_relaxed("debhelper (>= 1.0), @foo@, bla", false);
        assert_eq!(
            errors,
            vec![
                "expected $ or identifier but got ERROR",
                "expected comma or end of file but got Some(IDENT)",
                "expected $ or identifier but got ERROR"
            ]
        );
        let entry = Entry::from(vec![Relation::simple("bar")]);
        rels.insert(0, entry);
        assert_eq!(rels.to_string(), "bar, debhelper (>= 1.0), @foo@, bla");
    }

    #[test]
    fn test_replace() {
        let mut rels: Relations = r#"python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"#
            .parse()
            .unwrap();
        let entry = Entry::from(vec![Relation::simple("python3-dulwich")]);
        rels.replace(1, entry);
        assert_eq!(
            rels.to_string(),
            "python3-dulwich (>= 0.20.21), python3-dulwich"
        );
    }

    #[test]
    fn test_relation_from_entries() {
        let entries = vec![
            Entry::from(vec![Relation::simple("python3-dulwich")]),
            Entry::from(vec![Relation::simple("python3-breezy")]),
        ];
        let rels: Relations = entries.into();
        assert_eq!(rels.entries().count(), 2);
        assert_eq!(rels.to_string(), "python3-dulwich, python3-breezy");
    }

    #[test]
    fn test_entry_from_relations() {
        let relations = vec![
            Relation::simple("python3-dulwich"),
            Relation::simple("python3-breezy"),
        ];
        let entry: Entry = relations.into();
        assert_eq!(entry.relations().count(), 2);
        assert_eq!(entry.to_string(), "python3-dulwich | python3-breezy");
    }

    #[test]
    fn test_parse_entry() {
        let parsed: Entry = "python3-dulwich (>= 0.20.21) | bar".parse().unwrap();
        assert_eq!(parsed.to_string(), "python3-dulwich (>= 0.20.21) | bar");
        assert_eq!(parsed.relations().count(), 2);

        assert_eq!(
            "foo, bar".parse::<Entry>().unwrap_err(),
            "Multiple entries found"
        );
        assert_eq!("".parse::<Entry>().unwrap_err(), "No entry found");
    }

    #[test]
    fn test_parse_relation() {
        let parsed: Relation = "python3-dulwich (>= 0.20.21)".parse().unwrap();
        assert_eq!(parsed.to_string(), "python3-dulwich (>= 0.20.21)");
        assert_eq!(
            parsed.version(),
            Some((
                VersionConstraint::GreaterThanEqual,
                "0.20.21".parse().unwrap()
            ))
        );
        assert_eq!(
            "foo | bar".parse::<Relation>().unwrap_err(),
            "Multiple relations found"
        );
        assert_eq!("".parse::<Relation>().unwrap_err(), "No entry found");
    }

    #[test]
    fn test_special() {
        let parsed: Relation = "librust-breezyshim+dirty-tracker-dev:amd64 (>= 0.1.138-~~)"
            .parse()
            .unwrap();
        assert_eq!(
            parsed.to_string(),
            "librust-breezyshim+dirty-tracker-dev:amd64 (>= 0.1.138-~~)"
        );
        assert_eq!(
            parsed.version(),
            Some((
                VersionConstraint::GreaterThanEqual,
                "0.1.138-~~".parse().unwrap()
            ))
        );
        assert_eq!(parsed.archqual(), Some("amd64".to_string()));
        assert_eq!(parsed.name(), "librust-breezyshim+dirty-tracker-dev");
    }

    #[test]
    fn test_relations_satisfied_by() {
        let rels: Relations = "python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"
            .parse()
            .unwrap();
        let satisfied = |name: &str| -> Option<debversion::Version> {
            match name {
                "python3-dulwich" => Some("0.20.21".parse().unwrap()),
                _ => None,
            }
        };
        assert!(rels.satisfied_by(satisfied));

        let satisfied = |name: &str| match name {
            "python3-dulwich" => Some("0.21".parse().unwrap()),
            _ => None,
        };
        assert!(!rels.satisfied_by(satisfied));

        let satisfied = |name: &str| match name {
            "python3-dulwich" => Some("0.20.20".parse().unwrap()),
            _ => None,
        };
        assert!(!rels.satisfied_by(satisfied));
    }

    #[test]
    fn test_entry_satisfied_by() {
        let entry: Entry = "python3-dulwich (>= 0.20.21) | python3-dulwich (<< 0.18)"
            .parse()
            .unwrap();
        let satisfied = |name: &str| -> Option<debversion::Version> {
            match name {
                "python3-dulwich" => Some("0.20.21".parse().unwrap()),
                _ => None,
            }
        };
        assert!(entry.satisfied_by(satisfied));
        let satisfied = |name: &str| -> Option<debversion::Version> {
            match name {
                "python3-dulwich" => Some("0.18".parse().unwrap()),
                _ => None,
            }
        };
        assert!(!entry.satisfied_by(satisfied));
    }

    #[test]
    fn test_wrap_and_sort_relation() {
        let relation: Relation = "   python3-dulwich   (>= 11) [  amd64 ] <  lala>"
            .parse()
            .unwrap();

        let wrapped = relation.wrap_and_sort();

        assert_eq!(
            wrapped.to_string(),
            "python3-dulwich (>= 11) [amd64] <lala>"
        );
    }

    #[test]
    fn test_wrap_and_sort_relations() {
        let entry: Relations =
            "python3-dulwich (>= 0.20.21)   | bar, \n\n\n\npython3-dulwich (<< 0.21)"
                .parse()
                .unwrap();

        let wrapped = entry.wrap_and_sort();

        assert_eq!(
            wrapped.to_string(),
            "bar | python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serialize_relations() {
        let relations: Relations = "python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"
            .parse()
            .unwrap();
        let serialized = serde_json::to_string(&relations).unwrap();
        assert_eq!(
            serialized,
            r#""python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)""#
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_deserialize_relations() {
        let relations: Relations = "python3-dulwich (>= 0.20.21), python3-dulwich (<< 0.21)"
            .parse()
            .unwrap();
        let serialized = serde_json::to_string(&relations).unwrap();
        let deserialized: Relations = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.to_string(), relations.to_string());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serialize_relation() {
        let relation: Relation = "python3-dulwich (>= 0.20.21)".parse().unwrap();
        let serialized = serde_json::to_string(&relation).unwrap();
        assert_eq!(serialized, r#""python3-dulwich (>= 0.20.21)""#);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_deserialize_relation() {
        let relation: Relation = "python3-dulwich (>= 0.20.21)".parse().unwrap();
        let serialized = serde_json::to_string(&relation).unwrap();
        let deserialized: Relation = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.to_string(), relation.to_string());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serialize_entry() {
        let entry: Entry = "python3-dulwich (>= 0.20.21) | python3-dulwich (<< 0.18)"
            .parse()
            .unwrap();
        let serialized = serde_json::to_string(&entry).unwrap();
        assert_eq!(
            serialized,
            r#""python3-dulwich (>= 0.20.21) | python3-dulwich (<< 0.18)""#
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_deserialize_entry() {
        let entry: Entry = "python3-dulwich (>= 0.20.21) | python3-dulwich (<< 0.18)"
            .parse()
            .unwrap();
        let serialized = serde_json::to_string(&entry).unwrap();
        let deserialized: Entry = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.to_string(), entry.to_string());
    }

    #[test]
    fn test_remove_first_relation() {
        let entry: Entry = "python3-dulwich (>= 0.20.21) | python3-dulwich (<< 0.18)"
            .parse()
            .unwrap();
        let mut rel = entry.relations().next().unwrap();
        rel.remove();
        assert_eq!(entry.to_string(), "python3-dulwich (<< 0.18)");
    }

    #[test]
    fn test_remove_last_relation() {
        let entry: Entry = "python3-dulwich (>= 0.20.21) | python3-dulwich (<< 0.18)"
            .parse()
            .unwrap();
        let mut rel = entry.relations().nth(1).unwrap();
        rel.remove();
        assert_eq!(entry.to_string(), "python3-dulwich (>= 0.20.21)");
    }

    #[test]
    fn test_remove_only_relation() {
        let entry: Entry = "python3-dulwich (>= 0.20.21)".parse().unwrap();
        let mut rel = entry.relations().next().unwrap();
        rel.remove();
        assert_eq!(entry.to_string(), "");
    }

    #[test]
    fn test_relations_is_empty() {
        let entry: Relations = "python3-dulwich (>= 0.20.21)".parse().unwrap();
        assert!(!entry.is_empty());
        assert_eq!(1, entry.len());
        let mut rel = entry.entries().next().unwrap();
        rel.remove();
        assert!(entry.is_empty());
        assert_eq!(0, entry.len());
    }

    #[test]
    fn test_entry_is_empty() {
        let entry: Entry = "python3-dulwich (>= 0.20.21) | python3-dulwich (<< 0.18)"
            .parse()
            .unwrap();
        assert!(!entry.is_empty());
        assert_eq!(2, entry.len());
        let mut rel = entry.relations().next().unwrap();
        rel.remove();
        assert!(!entry.is_empty());
        assert_eq!(1, entry.len());
        let mut rel = entry.relations().next().unwrap();
        rel.remove();
        assert!(entry.is_empty());
        assert_eq!(0, entry.len());
    }

    #[test]
    fn test_relation_set_version() {
        let mut rel: Relation = "samba".parse().unwrap();
        rel.set_version(None);
        assert_eq!("samba", rel.to_string());
        rel.set_version(Some((
            VersionConstraint::GreaterThanEqual,
            "2.0".parse().unwrap(),
        )));
        assert_eq!("samba (>= 2.0)", rel.to_string());
        rel.set_version(None);
        assert_eq!("samba", rel.to_string());
        rel.set_version(Some((
            VersionConstraint::GreaterThanEqual,
            "2.0".parse().unwrap(),
        )));
        rel.set_version(Some((
            VersionConstraint::GreaterThanEqual,
            "1.1".parse().unwrap(),
        )));
        assert_eq!("samba (>= 1.1)", rel.to_string());
    }

    #[test]
    fn test_relation_set_version_constraints() {
        let version = "1.0".parse::<Version>().unwrap();
        for (vc, expected) in [
            (VersionConstraint::GreaterThanEqual, "samba (>= 1.0)"),
            (VersionConstraint::LessThanEqual, "samba (<= 1.0)"),
            (VersionConstraint::Equal, "samba (= 1.0)"),
            (VersionConstraint::GreaterThan, "samba (>> 1.0)"),
            (VersionConstraint::LessThan, "samba (<< 1.0)"),
        ] {
            let mut rel: Relation = "samba".parse().unwrap();
            rel.set_version(Some((vc.clone(), version.clone())));
            assert_eq!(expected, rel.to_string());
            // The result must round-trip: re-parsing yields the same constraint.
            let reparsed: Relation = rel.to_string().parse().unwrap();
            assert_eq!(Some((vc, version.clone())), reparsed.version());
        }
    }

    #[test]
    fn test_replace_relation() {
        let mut entry: Entry = "python3-dulwich (>= 0.20.21) | python3-dulwich (<< 0.18)"
            .parse()
            .unwrap();
        let new_rel = Relation::simple("python3-breezy");
        entry.replace(0, new_rel);
        assert_eq!(
            entry.to_string(),
            "python3-breezy | python3-dulwich (<< 0.18)"
        );
    }

    #[test]
    fn test_entry_push_relation() {
        let relations: Relations = "python3-dulwich (>= 0.20.21)".parse().unwrap();
        let new_rel = Relation::simple("python3-breezy");
        let mut entry = relations.entries().next().unwrap();
        entry.push(new_rel);
        assert_eq!(
            entry.to_string(),
            "python3-dulwich (>= 0.20.21) | python3-breezy"
        );
        assert_eq!(
            relations.to_string(),
            "python3-dulwich (>= 0.20.21) | python3-breezy"
        );
    }

    #[test]
    fn test_relations_remove_empty_entry() {
        let (mut relations, errors) = Relations::parse_relaxed("foo, , bar, ", false);
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(relations.to_string(), "foo, , bar, ");
        assert_eq!(relations.len(), 2);
        assert_eq!(
            relations.entries().next().unwrap().to_string(),
            "foo".to_string()
        );
        assert_eq!(
            relations.entries().nth(1).unwrap().to_string(),
            "bar".to_string()
        );
        relations.remove_entry(1);
        assert_eq!(relations.to_string(), "foo, , ");
    }

    #[test]
    fn test_entry_remove_relation() {
        let entry: Entry = "python3-dulwich | samba".parse().unwrap();
        let removed = entry.remove_relation(0);
        assert_eq!(removed.to_string(), "python3-dulwich");
        assert_eq!(entry.to_string(), "samba");
    }

    #[test]
    fn test_wrap_and_sort_removes_empty_entries() {
        let relations: Relations = "foo, , bar, ".parse().unwrap();
        let wrapped = relations.wrap_and_sort();
        assert_eq!(wrapped.to_string(), "bar, foo");
    }

    #[test]
    fn test_set_archqual() {
        let entry: Entry = "python3-dulwich | samba".parse().unwrap();
        let mut rel = entry.relations().next().unwrap();
        rel.set_archqual("amd64");
        assert_eq!(rel.to_string(), "python3-dulwich:amd64");
        assert_eq!(rel.archqual(), Some("amd64".to_string()));
        assert_eq!(entry.to_string(), "python3-dulwich:amd64 | samba");
        rel.set_archqual("i386");
        assert_eq!(rel.to_string(), "python3-dulwich:i386");
        assert_eq!(rel.archqual(), Some("i386".to_string()));
        assert_eq!(entry.to_string(), "python3-dulwich:i386 | samba");
    }

    #[test]
    fn test_set_architectures() {
        let mut relation = Relation::simple("samba");
        relation.set_architectures(vec!["amd64", "i386"].into_iter());
        assert_eq!(relation.to_string(), "samba [amd64 i386]");
    }

    #[test]
    fn test_relation_builder_no_architectures() {
        // Test that building a relation without architectures doesn't add empty brackets
        let relation = Relation::build("debhelper").build();
        assert_eq!(relation.to_string(), "debhelper");
    }

    #[test]
    fn test_relation_builder_with_architectures() {
        // Test that building a relation with architectures works correctly
        let relation = Relation::build("samba")
            .architectures(vec!["amd64".to_string(), "i386".to_string()])
            .build();
        assert_eq!(relation.to_string(), "samba [amd64 i386]");
    }

    #[test]
    fn test_ensure_minimum_version_add_new() {
        let mut relations: Relations = "python3".parse().unwrap();
        relations.ensure_minimum_version("debhelper", &"12".parse().unwrap());
        assert_eq!(relations.to_string(), "python3, debhelper (>= 12)");
    }

    #[test]
    fn test_ensure_minimum_version_update() {
        let mut relations: Relations = "debhelper (>= 9)".parse().unwrap();
        relations.ensure_minimum_version("debhelper", &"12".parse().unwrap());
        assert_eq!(relations.to_string(), "debhelper (>= 12)");
    }

    #[test]
    fn test_ensure_minimum_version_no_change() {
        let mut relations: Relations = "debhelper (>= 13)".parse().unwrap();
        relations.ensure_minimum_version("debhelper", &"12".parse().unwrap());
        assert_eq!(relations.to_string(), "debhelper (>= 13)");
    }

    #[test]
    fn test_ensure_minimum_version_no_version() {
        let mut relations: Relations = "debhelper".parse().unwrap();
        relations.ensure_minimum_version("debhelper", &"12".parse().unwrap());
        assert_eq!(relations.to_string(), "debhelper (>= 12)");
    }

    #[test]
    fn test_ensure_minimum_version_preserves_newline() {
        // Test that newline after the field name is preserved
        // This is the format often used in Debian control files:
        // Build-Depends:
        //  debhelper (>= 9),
        //  pkg-config
        let input = "\n debhelper (>= 9),\n pkg-config,\n uuid-dev";
        let mut relations: Relations = input.parse().unwrap();
        relations.ensure_minimum_version("debhelper", &"12~".parse().unwrap());
        let result = relations.to_string();

        // The newline before the first entry should be preserved
        assert!(
            result.starts_with('\n'),
            "Expected result to start with newline, got: {:?}",
            result
        );
        assert_eq!(result, "\n debhelper (>= 12~),\n pkg-config,\n uuid-dev");
    }

    #[test]
    fn test_ensure_minimum_version_preserves_newline_in_control() {
        // Test the full scenario from the bug report
        use crate::edit::Control;
        use std::str::FromStr;

        let input = r#"Source: f2fs-tools
Section: admin
Priority: optional
Maintainer: Test <test@example.com>
Build-Depends:
 debhelper (>= 9),
 pkg-config,
 uuid-dev

Package: f2fs-tools
Description: test
"#;

        let control = Control::from_str(input).unwrap();
        let mut source = control.source().unwrap();
        let mut build_depends = source.build_depends().unwrap();

        let version = Version::from_str("12~").unwrap();
        build_depends.ensure_minimum_version("debhelper", &version);

        source.set_build_depends(&build_depends);

        let output = control.to_string();

        // Check that the formatting is preserved - the newline after "Build-Depends:" should still be there
        assert!(
            output.contains("Build-Depends:\n debhelper (>= 12~)"),
            "Expected 'Build-Depends:\\n debhelper (>= 12~)' but got:\n{}",
            output
        );
    }

    #[test]
    fn test_ensure_exact_version_add_new() {
        let mut relations: Relations = "python3".parse().unwrap();
        relations.ensure_exact_version("debhelper", &"12".parse().unwrap());
        assert_eq!(relations.to_string(), "python3, debhelper (= 12)");
    }

    #[test]
    fn test_ensure_exact_version_update() {
        let mut relations: Relations = "debhelper (>= 9)".parse().unwrap();
        relations.ensure_exact_version("debhelper", &"12".parse().unwrap());
        assert_eq!(relations.to_string(), "debhelper (= 12)");
    }

    #[test]
    fn test_ensure_exact_version_no_change() {
        let mut relations: Relations = "debhelper (= 12)".parse().unwrap();
        relations.ensure_exact_version("debhelper", &"12".parse().unwrap());
        assert_eq!(relations.to_string(), "debhelper (= 12)");
    }

    #[test]
    fn test_ensure_some_version_add_new() {
        let mut relations: Relations = "python3".parse().unwrap();
        relations.ensure_some_version("debhelper");
        assert_eq!(relations.to_string(), "python3, debhelper");
    }

    #[test]
    fn test_ensure_some_version_exists_with_version() {
        let mut relations: Relations = "debhelper (>= 12)".parse().unwrap();
        relations.ensure_some_version("debhelper");
        assert_eq!(relations.to_string(), "debhelper (>= 12)");
    }

    #[test]
    fn test_ensure_some_version_exists_no_version() {
        let mut relations: Relations = "debhelper".parse().unwrap();
        relations.ensure_some_version("debhelper");
        assert_eq!(relations.to_string(), "debhelper");
    }

    #[test]
    fn test_ensure_substvar() {
        let mut relations: Relations = "python3".parse().unwrap();
        relations.ensure_substvar("${misc:Depends}").unwrap();
        assert_eq!(relations.to_string(), "python3, ${misc:Depends}");
    }

    #[test]
    fn test_ensure_substvar_already_exists() {
        let (mut relations, _) = Relations::parse_relaxed("python3, ${misc:Depends}", true);
        relations.ensure_substvar("${misc:Depends}").unwrap();
        assert_eq!(relations.to_string(), "python3, ${misc:Depends}");
    }

    #[test]
    fn test_ensure_substvar_empty_relations() {
        let mut relations: Relations = Relations::new();
        relations.ensure_substvar("${misc:Depends}").unwrap();
        assert_eq!(relations.to_string(), "${misc:Depends}");
    }

    #[test]
    fn test_ensure_substvar_preserves_whitespace() {
        // Test with non-standard whitespace (multiple spaces)
        let (mut relations, _) = Relations::parse_relaxed("python3,  rustc", false);
        relations.ensure_substvar("${misc:Depends}").unwrap();
        // Should preserve the double-space pattern
        assert_eq!(relations.to_string(), "python3,  rustc,  ${misc:Depends}");
    }

    #[test]
    fn test_ensure_substvar_to_existing_substvar() {
        // Test adding a substvar to existing substvar (no entries)
        // This reproduces the bug where space after comma is lost
        let (mut relations, _) = Relations::parse_relaxed("${shlibs:Depends}", true);
        relations.ensure_substvar("${misc:Depends}").unwrap();
        // Should have a space after the comma
        assert_eq!(relations.to_string(), "${shlibs:Depends}, ${misc:Depends}");
    }

    #[test]
    fn test_drop_substvar_basic() {
        let (mut relations, _) = Relations::parse_relaxed("python3, ${misc:Depends}", true);
        relations.drop_substvar("${misc:Depends}");
        assert_eq!(relations.to_string(), "python3");
    }

    #[test]
    fn test_drop_substvar_first_position() {
        let (mut relations, _) = Relations::parse_relaxed("${misc:Depends}, python3", true);
        relations.drop_substvar("${misc:Depends}");
        assert_eq!(relations.to_string(), "python3");
    }

    #[test]
    fn test_drop_substvar_middle_position() {
        let (mut relations, _) = Relations::parse_relaxed("python3, ${misc:Depends}, rustc", true);
        relations.drop_substvar("${misc:Depends}");
        assert_eq!(relations.to_string(), "python3, rustc");
    }

    #[test]
    fn test_drop_substvar_only_substvar() {
        let (mut relations, _) = Relations::parse_relaxed("${misc:Depends}", true);
        relations.drop_substvar("${misc:Depends}");
        assert_eq!(relations.to_string(), "");
    }

    #[test]
    fn test_drop_substvar_not_exists() {
        let (mut relations, _) = Relations::parse_relaxed("python3, rustc", false);
        relations.drop_substvar("${misc:Depends}");
        assert_eq!(relations.to_string(), "python3, rustc");
    }

    #[test]
    fn test_drop_substvar_multiple_substvars() {
        let (mut relations, _) =
            Relations::parse_relaxed("python3, ${misc:Depends}, ${shlibs:Depends}", true);
        relations.drop_substvar("${misc:Depends}");
        assert_eq!(relations.to_string(), "python3, ${shlibs:Depends}");
    }

    #[test]
    fn test_drop_substvar_preserves_whitespace() {
        let (mut relations, _) = Relations::parse_relaxed("python3,  ${misc:Depends}", true);
        relations.drop_substvar("${misc:Depends}");
        assert_eq!(relations.to_string(), "python3");
    }

    #[test]
    fn test_filter_entries_basic() {
        let mut relations: Relations = "python3, debhelper, rustc".parse().unwrap();
        relations.filter_entries(|entry| entry.relations().any(|r| r.name().starts_with("python")));
        assert_eq!(relations.to_string(), "python3");
    }

    #[test]
    fn test_filter_entries_keep_all() {
        let mut relations: Relations = "python3, debhelper".parse().unwrap();
        relations.filter_entries(|_| true);
        assert_eq!(relations.to_string(), "python3, debhelper");
    }

    #[test]
    fn test_filter_entries_remove_all() {
        let mut relations: Relations = "python3, debhelper".parse().unwrap();
        relations.filter_entries(|_| false);
        assert_eq!(relations.to_string(), "");
    }

    #[test]
    fn test_filter_entries_keep_middle() {
        let mut relations: Relations = "aaa, bbb, ccc".parse().unwrap();
        relations.filter_entries(|entry| entry.relations().any(|r| r.name() == "bbb"));
        assert_eq!(relations.to_string(), "bbb");
    }

    // Tests for new convenience methods

    #[test]
    fn test_is_sorted_wrap_and_sort_order() {
        // Sorted according to WrapAndSortOrder
        let relations: Relations = "debhelper, python3, rustc".parse().unwrap();
        assert!(relations.is_sorted(&WrapAndSortOrder));

        // Not sorted
        let relations: Relations = "rustc, debhelper, python3".parse().unwrap();
        assert!(!relations.is_sorted(&WrapAndSortOrder));

        // Build systems first (sorted alphabetically within their group)
        let (relations, _) =
            Relations::parse_relaxed("cdbs, debhelper-compat, python3, ${misc:Depends}", true);
        assert!(relations.is_sorted(&WrapAndSortOrder));
    }

    #[test]
    fn test_is_sorted_default_order() {
        // Sorted alphabetically
        let relations: Relations = "aaa, bbb, ccc".parse().unwrap();
        assert!(relations.is_sorted(&DefaultSortingOrder));

        // Not sorted
        let relations: Relations = "ccc, aaa, bbb".parse().unwrap();
        assert!(!relations.is_sorted(&DefaultSortingOrder));

        // Special items at end
        let (relations, _) = Relations::parse_relaxed("aaa, bbb, ${misc:Depends}", true);
        assert!(relations.is_sorted(&DefaultSortingOrder));
    }

    #[test]
    fn test_is_sorted_with_substvars() {
        // Substvars should be ignored by DefaultSortingOrder
        let (relations, _) = Relations::parse_relaxed("python3, ${misc:Depends}, rustc", true);
        // This is considered sorted because ${misc:Depends} is ignored
        assert!(relations.is_sorted(&DefaultSortingOrder));
    }

    #[test]
    fn test_drop_dependency_exists() {
        let mut relations: Relations = "python3, debhelper, rustc".parse().unwrap();
        assert!(relations.drop_dependency("debhelper"));
        assert_eq!(relations.to_string(), "python3, rustc");
    }

    #[test]
    fn test_drop_dependency_not_exists() {
        let mut relations: Relations = "python3, rustc".parse().unwrap();
        assert!(!relations.drop_dependency("nonexistent"));
        assert_eq!(relations.to_string(), "python3, rustc");
    }

    #[test]
    fn test_drop_dependency_only_item() {
        let mut relations: Relations = "python3".parse().unwrap();
        assert!(relations.drop_dependency("python3"));
        assert_eq!(relations.to_string(), "");
    }

    #[test]
    fn test_add_dependency_to_empty() {
        let mut relations: Relations = "".parse().unwrap();
        let entry = Entry::from(Relation::simple("python3"));
        relations.add_dependency(entry, None);
        assert_eq!(relations.to_string(), "python3");
    }

    #[test]
    fn test_add_dependency_sorted_position() {
        let mut relations: Relations = "debhelper, rustc".parse().unwrap();
        let entry = Entry::from(Relation::simple("python3"));
        relations.add_dependency(entry, None);
        // Should be inserted in sorted position
        assert_eq!(relations.to_string(), "debhelper, python3, rustc");
    }

    #[test]
    fn test_add_dependency_explicit_position() {
        let mut relations: Relations = "python3, rustc".parse().unwrap();
        let entry = Entry::from(Relation::simple("debhelper"));
        relations.add_dependency(entry, Some(0));
        assert_eq!(relations.to_string(), "debhelper, python3, rustc");
    }

    #[test]
    fn test_add_dependency_build_system_first() {
        let mut relations: Relations = "python3, rustc".parse().unwrap();
        let entry = Entry::from(Relation::simple("debhelper-compat"));
        relations.add_dependency(entry, None);
        // debhelper-compat should be inserted first (build system)
        assert_eq!(relations.to_string(), "debhelper-compat, python3, rustc");
    }

    #[test]
    fn test_add_dependency_at_end() {
        let mut relations: Relations = "debhelper, python3".parse().unwrap();
        let entry = Entry::from(Relation::simple("zzz-package"));
        relations.add_dependency(entry, None);
        // Should be added at the end (alphabetically after python3)
        assert_eq!(relations.to_string(), "debhelper, python3, zzz-package");
    }

    #[test]
    fn test_add_dependency_to_single_entry() {
        // Regression test: ensure comma is added when inserting into single-entry Relations
        let mut relations: Relations = "python3-dulwich".parse().unwrap();
        let entry: Entry = "debhelper-compat (= 12)".parse().unwrap();
        relations.add_dependency(entry, None);
        // Should insert with comma separator
        assert_eq!(
            relations.to_string(),
            "debhelper-compat (= 12), python3-dulwich"
        );
    }

    #[test]
    fn test_get_relation_exists() {
        let relations: Relations = "python3, debhelper (>= 12), rustc".parse().unwrap();
        let result = relations.get_relation("debhelper");
        assert!(result.is_ok());
        let (idx, entry) = result.unwrap();
        assert_eq!(idx, 1);
        assert_eq!(entry.to_string(), "debhelper (>= 12)");
    }

    #[test]
    fn test_get_relation_not_exists() {
        let relations: Relations = "python3, rustc".parse().unwrap();
        let result = relations.get_relation("nonexistent");
        assert_eq!(result, Err("Package nonexistent not found".to_string()));
    }

    #[test]
    fn test_get_relation_complex_rule() {
        let relations: Relations = "python3 | python3-minimal, rustc".parse().unwrap();
        let result = relations.get_relation("python3");
        assert_eq!(
            result,
            Err("Complex rule for python3, aborting".to_string())
        );
    }

    #[test]
    fn test_iter_relations_for_simple() {
        let relations: Relations = "python3, debhelper, python3-dev".parse().unwrap();
        let entries: Vec<_> = relations.iter_relations_for("python3").collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, 0);
        assert_eq!(entries[0].1.to_string(), "python3");
    }

    #[test]
    fn test_iter_relations_for_alternatives() {
        let relations: Relations = "python3 | python3-minimal, python3-dev".parse().unwrap();
        let entries: Vec<_> = relations.iter_relations_for("python3").collect();
        // Should find both the alternative entry and python3-dev is not included
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, 0);
    }

    #[test]
    fn test_iter_relations_for_not_found() {
        let relations: Relations = "python3, rustc".parse().unwrap();
        let entries: Vec<_> = relations.iter_relations_for("debhelper").collect();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_has_relation_exists() {
        let relations: Relations = "python3, debhelper, rustc".parse().unwrap();
        assert!(relations.has_relation("debhelper"));
        assert!(relations.has_relation("python3"));
        assert!(relations.has_relation("rustc"));
    }

    #[test]
    fn test_has_relation_not_exists() {
        let relations: Relations = "python3, rustc".parse().unwrap();
        assert!(!relations.has_relation("debhelper"));
    }

    #[test]
    fn test_has_relation_in_alternative() {
        let relations: Relations = "python3 | python3-minimal".parse().unwrap();
        assert!(relations.has_relation("python3"));
        assert!(relations.has_relation("python3-minimal"));
    }

    #[test]
    fn test_sorting_order_wrap_and_sort_build_systems() {
        let order = WrapAndSortOrder;
        // Build systems should come before regular packages
        assert!(order.lt("debhelper", "python3"));
        assert!(order.lt("debhelper-compat", "rustc"));
        assert!(order.lt("cdbs", "aaa"));
        assert!(order.lt("dh-python", "python3"));
    }

    #[test]
    fn test_sorting_order_wrap_and_sort_regular_packages() {
        let order = WrapAndSortOrder;
        // Regular packages sorted alphabetically
        assert!(order.lt("aaa", "bbb"));
        assert!(order.lt("python3", "rustc"));
        assert!(!order.lt("rustc", "python3"));
    }

    #[test]
    fn test_sorting_order_wrap_and_sort_substvars() {
        let order = WrapAndSortOrder;
        // Substvars should come after regular packages
        assert!(order.lt("python3", "${misc:Depends}"));
        assert!(!order.lt("${misc:Depends}", "python3"));
        // But wrap-and-sort doesn't ignore them
        assert!(!order.ignore("${misc:Depends}"));
    }

    #[test]
    fn test_sorting_order_default_special_items() {
        let order = DefaultSortingOrder;
        // Special items should come after regular items
        assert!(order.lt("python3", "${misc:Depends}"));
        assert!(order.lt("aaa", "@cdbs@"));
        // And should be ignored
        assert!(order.ignore("${misc:Depends}"));
        assert!(order.ignore("@cdbs@"));
        assert!(!order.ignore("python3"));
    }

    #[test]
    fn test_is_special_package_name() {
        assert!(is_special_package_name("${misc:Depends}"));
        assert!(is_special_package_name("${shlibs:Depends}"));
        assert!(is_special_package_name("@cdbs@"));
        assert!(!is_special_package_name("python3"));
        assert!(!is_special_package_name("debhelper"));
    }

    #[test]
    fn test_add_dependency_with_explicit_position() {
        // Test that add_dependency works with explicit position and preserves whitespace
        let mut relations: Relations = "python3,  rustc".parse().unwrap();
        let entry = Entry::from(Relation::simple("debhelper"));
        relations.add_dependency(entry, Some(1));
        // Should preserve the 2-space pattern from the original
        assert_eq!(relations.to_string(), "python3,  debhelper,  rustc");
    }

    #[test]
    fn test_whitespace_detection_single_space() {
        let mut relations: Relations = "python3, rustc".parse().unwrap();
        let entry = Entry::from(Relation::simple("debhelper"));
        relations.add_dependency(entry, Some(1));
        assert_eq!(relations.to_string(), "python3, debhelper, rustc");
    }

    #[test]
    fn test_whitespace_detection_multiple_spaces() {
        let mut relations: Relations = "python3,  rustc,  gcc".parse().unwrap();
        let entry = Entry::from(Relation::simple("debhelper"));
        relations.add_dependency(entry, Some(1));
        // Should detect and use the 2-space pattern
        assert_eq!(relations.to_string(), "python3,  debhelper,  rustc,  gcc");
    }

    #[test]
    fn test_whitespace_detection_mixed_patterns() {
        // When patterns differ, use the most common one
        let mut relations: Relations = "a, b, c,  d, e".parse().unwrap();
        let entry = Entry::from(Relation::simple("x"));
        relations.push(entry);
        // Three single-space (after a, b, d), one double-space (after c)
        // Should use single space as it's most common
        assert_eq!(relations.to_string(), "a, b, c,  d, e, x");
    }

    #[test]
    fn test_whitespace_detection_newlines() {
        let mut relations: Relations = "python3,\n rustc".parse().unwrap();
        let entry = Entry::from(Relation::simple("debhelper"));
        relations.add_dependency(entry, Some(1));
        // Detects full pattern including newline
        assert_eq!(relations.to_string(), "python3,\n debhelper,\n rustc");
    }

    #[test]
    fn test_append_with_newline_no_trailing() {
        let mut relations: Relations = "foo,\n bar".parse().unwrap();
        let entry = Entry::from(Relation::simple("blah"));
        relations.add_dependency(entry, None);
        assert_eq!(relations.to_string(), "foo,\n bar,\n blah");
    }

    #[test]
    fn test_append_with_trailing_newline() {
        let mut relations: Relations = "foo,\n bar\n".parse().unwrap();
        let entry = Entry::from(Relation::simple("blah"));
        relations.add_dependency(entry, None);
        assert_eq!(relations.to_string(), "foo,\n bar,\n blah");
    }

    #[test]
    fn test_append_with_4_space_indent() {
        let mut relations: Relations = "foo,\n    bar".parse().unwrap();
        let entry = Entry::from(Relation::simple("blah"));
        relations.add_dependency(entry, None);
        assert_eq!(relations.to_string(), "foo,\n    bar,\n    blah");
    }

    #[test]
    fn test_append_with_4_space_and_trailing_newline() {
        let mut relations: Relations = "foo,\n    bar\n".parse().unwrap();
        let entry = Entry::from(Relation::simple("blah"));
        relations.add_dependency(entry, None);
        assert_eq!(relations.to_string(), "foo,\n    bar,\n    blah");
    }

    #[test]
    fn test_odd_syntax_append_no_trailing() {
        let mut relations: Relations = "\n foo\n , bar".parse().unwrap();
        let entry = Entry::from(Relation::simple("blah"));
        relations.add_dependency(entry, None);
        assert_eq!(relations.to_string(), "\n foo\n , bar\n , blah");
    }

    #[test]
    fn test_odd_syntax_append_with_trailing() {
        let mut relations: Relations = "\n foo\n , bar\n".parse().unwrap();
        let entry = Entry::from(Relation::simple("blah"));
        relations.add_dependency(entry, None);
        assert_eq!(relations.to_string(), "\n foo\n , bar\n , blah");
    }

    #[test]
    fn test_insert_at_1_no_trailing() {
        let mut relations: Relations = "foo,\n bar".parse().unwrap();
        let entry = Entry::from(Relation::simple("blah"));
        relations.add_dependency(entry, Some(1));
        assert_eq!(relations.to_string(), "foo,\n blah,\n bar");
    }

    #[test]
    fn test_insert_at_1_with_trailing() {
        let mut relations: Relations = "foo,\n bar\n".parse().unwrap();
        let entry = Entry::from(Relation::simple("blah"));
        relations.add_dependency(entry, Some(1));
        assert_eq!(relations.to_string(), "foo,\n blah,\n bar");
    }

    #[test]
    fn test_odd_syntax_insert_at_1() {
        let mut relations: Relations = "\n foo\n , bar\n".parse().unwrap();
        let entry = Entry::from(Relation::simple("blah"));
        relations.add_dependency(entry, Some(1));
        assert_eq!(relations.to_string(), "\n foo\n , blah\n , bar");
    }

    #[test]
    fn test_relations_preserves_exact_whitespace() {
        // Test that Relations preserves exact whitespace from input
        let input =
            "debhelper (>= 10), quilt (>= 0.40),\n    libsystemd-dev [linux-any], pkg-config";

        let relations: Relations = input.parse().unwrap();

        // The whitespace should be preserved in the syntax tree
        assert_eq!(
            relations.to_string(),
            input,
            "Relations should preserve exact whitespace from input"
        );
    }

    #[test]
    fn test_remove_entry_preserves_indentation() {
        // Test that removing an entry preserves the indentation pattern
        let input = "debhelper (>= 10), quilt (>= 0.40),\n    libsystemd-dev [linux-any], dh-systemd (>= 1.5), pkg-config";

        let mut relations: Relations = input.parse().unwrap();

        // Find and remove dh-systemd entry (index 2)
        let mut to_remove = Vec::new();
        for (idx, entry) in relations.entries().enumerate() {
            for relation in entry.relations() {
                if relation.name() == "dh-systemd" {
                    to_remove.push(idx);
                    break;
                }
            }
        }

        for idx in to_remove.into_iter().rev() {
            relations.remove_entry(idx);
        }

        let output = relations.to_string();
        println!("After removal: '{}'", output);

        // The 4-space indentation should be preserved
        assert!(
            output.contains("\n    libsystemd-dev"),
            "Expected 4-space indentation to be preserved, but got:\n'{}'",
            output
        );
    }

    #[test]
    fn test_relation_is_implied_by_same_package() {
        // Same package name with compatible version constraints
        let inner = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "1.0".parse().unwrap())),
        );
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "1.5".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));
    }

    #[test]
    fn test_relation_is_implied_by_different_package() {
        // Different package names should not imply
        let inner = Relation::new("pkg1", None);
        let outer = Relation::new("pkg2", None);
        assert!(!inner.is_implied_by(&outer));
    }

    #[test]
    fn test_relation_is_implied_by_no_version() {
        // No version constraint is implied by any version
        let inner = Relation::new("pkg", None);
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "1.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));
    }

    #[test]
    fn test_relation_is_implied_by_identical() {
        // Identical relations imply each other
        let inner = Relation::new(
            "pkg",
            Some((VersionConstraint::Equal, "1.0".parse().unwrap())),
        );
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::Equal, "1.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));
        assert!(outer.is_implied_by(&inner));
    }

    #[test]
    fn test_relation_is_implied_by_greater_than_equal() {
        // pkg >= 1.0 is implied by pkg >= 2.0
        let inner = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "1.0".parse().unwrap())),
        );
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));
        assert!(!outer.is_implied_by(&inner));

        // pkg >= 1.0 is implied by pkg = 2.0
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::Equal, "2.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));

        // pkg >= 1.0 is implied by pkg >> 1.5
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThan, "1.5".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));

        // pkg >= 3.0 is NOT implied by pkg >> 3.0 (>> 3.0 doesn't include 3.0 itself)
        let inner = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "3.0".parse().unwrap())),
        );
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThan, "3.0".parse().unwrap())),
        );
        assert!(!inner.is_implied_by(&outer));
    }

    #[test]
    fn test_relation_is_implied_by_less_than_equal() {
        // pkg <= 2.0 is implied by pkg <= 1.0
        let inner = Relation::new(
            "pkg",
            Some((VersionConstraint::LessThanEqual, "2.0".parse().unwrap())),
        );
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::LessThanEqual, "1.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));
        assert!(!outer.is_implied_by(&inner));

        // pkg <= 2.0 is implied by pkg = 1.0
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::Equal, "1.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));

        // pkg <= 2.0 is implied by pkg << 1.5
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::LessThan, "1.5".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));
    }

    #[test]
    fn test_relation_is_implied_by_equal() {
        // pkg = 1.0 is only implied by pkg = 1.0
        let inner = Relation::new(
            "pkg",
            Some((VersionConstraint::Equal, "1.0".parse().unwrap())),
        );
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::Equal, "1.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));

        // Not implied by different version
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::Equal, "2.0".parse().unwrap())),
        );
        assert!(!inner.is_implied_by(&outer));

        // Not implied by >= constraint
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "1.0".parse().unwrap())),
        );
        assert!(!inner.is_implied_by(&outer));
    }

    #[test]
    fn test_relation_is_implied_by_greater_than() {
        // pkg >> 1.0 is implied by pkg >> 2.0
        let inner = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThan, "1.0".parse().unwrap())),
        );
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThan, "2.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));

        // pkg >> 1.0 is implied by pkg = 2.0
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::Equal, "2.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));

        // pkg >> 1.0 is implied by pkg >= 1.5 (strictly greater)
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "1.5".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));

        // pkg >> 1.0 is NOT implied by pkg >= 1.0 (could be equal)
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "1.0".parse().unwrap())),
        );
        assert!(!inner.is_implied_by(&outer));
    }

    #[test]
    fn test_relation_is_implied_by_less_than() {
        // pkg << 2.0 is implied by pkg << 1.0
        let inner = Relation::new(
            "pkg",
            Some((VersionConstraint::LessThan, "2.0".parse().unwrap())),
        );
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::LessThan, "1.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));

        // pkg << 2.0 is implied by pkg = 1.0
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::Equal, "1.0".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));

        // pkg << 2.0 is implied by pkg <= 1.5 (strictly less)
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::LessThanEqual, "1.5".parse().unwrap())),
        );
        assert!(inner.is_implied_by(&outer));
    }

    #[test]
    fn test_relation_is_implied_by_incompatible_constraints() {
        // >= and <= are incompatible
        let inner = Relation::new(
            "pkg",
            Some((VersionConstraint::GreaterThanEqual, "1.0".parse().unwrap())),
        );
        let outer = Relation::new(
            "pkg",
            Some((VersionConstraint::LessThanEqual, "2.0".parse().unwrap())),
        );
        assert!(!inner.is_implied_by(&outer));
        assert!(!outer.is_implied_by(&inner));
    }

    #[test]
    fn test_entry_is_implied_by_identical() {
        let inner: Entry = "pkg (>= 1.0)".parse().unwrap();
        let outer: Entry = "pkg (>= 1.0)".parse().unwrap();
        assert!(inner.is_implied_by(&outer));
    }

    #[test]
    fn test_entry_is_implied_by_or_group() {
        // "pkg >= 1.0" is implied by "pkg >= 1.5 | libc6"
        let inner: Entry = "pkg (>= 1.0)".parse().unwrap();
        let outer: Entry = "pkg (>= 1.5) | libc6".parse().unwrap();
        assert!(inner.is_implied_by(&outer));
    }

    #[test]
    fn test_entry_is_implied_by_simple_or() {
        // "pkg1 | pkg2" is implied by "pkg1" (first alternative satisfies)
        let inner: Entry = "pkg1 | pkg2".parse().unwrap();
        let outer: Entry = "pkg1".parse().unwrap();
        assert!(inner.is_implied_by(&outer));

        // Also implied by "pkg2"
        let outer: Entry = "pkg2".parse().unwrap();
        assert!(inner.is_implied_by(&outer));
    }

    #[test]
    fn test_entry_is_implied_by_not_implied() {
        // "pkg >= 2.0" is NOT implied by "pkg >= 1.0"
        let inner: Entry = "pkg (>= 2.0)".parse().unwrap();
        let outer: Entry = "pkg (>= 1.0)".parse().unwrap();
        assert!(!inner.is_implied_by(&outer));
    }

    #[test]
    fn test_entry_is_implied_by_different_packages() {
        let inner: Entry = "pkg1".parse().unwrap();
        let outer: Entry = "pkg2".parse().unwrap();
        assert!(!inner.is_implied_by(&outer));
    }

    #[test]
    fn test_entry_is_implied_by_complex_or() {
        // "pkg1 | pkg2" is implied by "pkg1 | pkg2" (identical)
        let inner: Entry = "pkg1 | pkg2".parse().unwrap();
        let outer: Entry = "pkg1 | pkg2".parse().unwrap();
        assert!(inner.is_implied_by(&outer));

        // "pkg1 | pkg2" is implied by "pkg1 | pkg2 | pkg3" (one matches)
        let outer: Entry = "pkg1 | pkg2 | pkg3".parse().unwrap();
        assert!(inner.is_implied_by(&outer));
    }

    #[test]
    fn test_parse_version_with_epoch() {
        // Test parsing version strings with epoch (e.g., "1:2.3.2-2~")
        // The colon should be treated as part of the version, not as a delimiter
        let input = "amule-dbg (<< 1:2.3.2-2~)";
        let parsed: Relations = input.parse().unwrap();
        assert_eq!(parsed.to_string(), input);
        assert_eq!(parsed.entries().count(), 1);
        let entry = parsed.entries().next().unwrap();
        assert_eq!(entry.to_string(), "amule-dbg (<< 1:2.3.2-2~)");
        assert_eq!(entry.relations().count(), 1);
        let relation = entry.relations().next().unwrap();
        assert_eq!(relation.name(), "amule-dbg");
        assert_eq!(relation.to_string(), "amule-dbg (<< 1:2.3.2-2~)");
        assert_eq!(
            relation.version(),
            Some((VersionConstraint::LessThan, "1:2.3.2-2~".parse().unwrap()))
        );
    }

    #[test]
    fn test_ensure_relation_add_new() {
        // Test adding a new relation that doesn't exist yet
        let mut relations: Relations = "python3".parse().unwrap();
        let new_entry: Entry = "debhelper (>= 12)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        // debhelper is inserted in sorted position (alphabetically before python3)
        assert_eq!(relations.to_string(), "debhelper (>= 12), python3");
    }

    #[test]
    fn test_ensure_relation_already_satisfied() {
        // Test that a relation is not added if it's already satisfied by a stronger constraint
        let mut relations: Relations = "debhelper (>= 13)".parse().unwrap();
        let new_entry: Entry = "debhelper (>= 12)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(!added);
        assert_eq!(relations.to_string(), "debhelper (>= 13)");
    }

    #[test]
    fn test_ensure_relation_replace_weaker() {
        // Test that a weaker relation is replaced with a stronger one
        let mut relations: Relations = "debhelper (>= 11)".parse().unwrap();
        let new_entry: Entry = "debhelper (>= 13)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        assert_eq!(relations.to_string(), "debhelper (>= 13)");
    }

    #[test]
    fn test_ensure_relation_replace_multiple_weaker() {
        // Test that multiple weaker relations are replaced/removed when a stronger one is added
        let mut relations: Relations = "debhelper (>= 11), debhelper (>= 10), python3"
            .parse()
            .unwrap();
        let new_entry: Entry = "debhelper (>= 13)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        assert_eq!(relations.to_string(), "debhelper (>= 13), python3");
    }

    #[test]
    fn test_ensure_relation_identical_entry() {
        // Test that an identical entry is not added again
        let mut relations: Relations = "debhelper (>= 12)".parse().unwrap();
        let new_entry: Entry = "debhelper (>= 12)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(!added);
        assert_eq!(relations.to_string(), "debhelper (>= 12)");
    }

    #[test]
    fn test_ensure_relation_no_version_constraint() {
        // Test that a relation without version constraint is added
        let mut relations: Relations = "python3".parse().unwrap();
        let new_entry: Entry = "debhelper".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        // debhelper is inserted in sorted position (alphabetically before python3)
        assert_eq!(relations.to_string(), "debhelper, python3");
    }

    #[test]
    fn test_ensure_relation_strengthen_unversioned() {
        // Test that a versioned constraint replaces an unversioned one
        // An unversioned dependency is weaker than a versioned one
        let mut relations: Relations = "debhelper".parse().unwrap();
        let new_entry: Entry = "debhelper (>= 12)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        assert_eq!(relations.to_string(), "debhelper (>= 12)");
    }

    #[test]
    fn test_ensure_relation_versioned_implies_unversioned() {
        // Test that an unversioned dependency is already satisfied by a versioned one
        // A versioned dependency is stronger and implies the unversioned one
        let mut relations: Relations = "debhelper (>= 12)".parse().unwrap();
        let new_entry: Entry = "debhelper".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(!added);
        assert_eq!(relations.to_string(), "debhelper (>= 12)");
    }

    #[test]
    fn test_ensure_relation_preserves_whitespace() {
        // Test that whitespace is preserved when adding a new relation
        let mut relations: Relations = "python3,  rustc".parse().unwrap();
        let new_entry: Entry = "debhelper (>= 12)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        // debhelper is inserted in sorted position (alphabetically before python3 and rustc)
        assert_eq!(relations.to_string(), "debhelper (>= 12),  python3,  rustc");
    }

    #[test]
    fn test_ensure_relation_empty_relations() {
        // Test adding to empty relations
        let mut relations: Relations = Relations::new();
        let new_entry: Entry = "debhelper (>= 12)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        assert_eq!(relations.to_string(), "debhelper (>= 12)");
    }

    #[test]
    fn test_ensure_relation_alternative_dependencies() {
        // Test with alternative dependencies (|)
        let mut relations: Relations = "python3 | python3-minimal".parse().unwrap();
        let new_entry: Entry = "debhelper (>= 12)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        // debhelper is inserted in sorted position (alphabetically before python3)
        assert_eq!(
            relations.to_string(),
            "debhelper (>= 12), python3 | python3-minimal"
        );
    }

    #[test]
    fn test_ensure_relation_replace_in_middle() {
        // Test that replacing a weaker entry in the middle preserves order
        let mut relations: Relations = "python3, debhelper (>= 11), rustc".parse().unwrap();
        let new_entry: Entry = "debhelper (>= 13)".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        assert_eq!(relations.to_string(), "python3, debhelper (>= 13), rustc");
    }

    #[test]
    fn test_ensure_relation_with_different_package() {
        // Test that adding a different package doesn't affect existing ones
        let mut relations: Relations = "python3, debhelper (>= 12)".parse().unwrap();
        let new_entry: Entry = "rustc".parse().unwrap();
        let added = relations.ensure_relation(new_entry);
        assert!(added);
        assert_eq!(relations.to_string(), "python3, debhelper (>= 12), rustc");
    }

    #[test]
    fn test_parse_invalid_token_in_arch_list() {
        let input = "foo [>= bar]";
        let result: Result<Relations, _> = input.parse();
        assert!(
            result.is_err(),
            "Expected error for invalid token in architecture list"
        );
    }

    #[test]
    fn test_parse_invalid_token_in_profile_list() {
        let input = "foo <[] baz>";
        let result: Result<Relations, _> = input.parse();
        assert!(
            result.is_err(),
            "Expected error for invalid token in profile list"
        );
    }

    #[test]
    fn test_parse_relaxed_unterminated_arch_list() {
        let (relations, errors) = Relations::parse_relaxed("libc6 [", true);
        assert!(!errors.is_empty());
        assert_eq!(relations.to_string(), "libc6 [");
    }

    #[test]
    fn test_parse_relaxed_partial_arch_name() {
        let (relations, errors) = Relations::parse_relaxed("libc6 [amd", true);
        assert!(!errors.is_empty());
        assert_eq!(relations.to_string(), "libc6 [amd");
    }

    #[test]
    fn test_parse_relaxed_unterminated_profile_list() {
        let (relations, errors) = Relations::parse_relaxed("libc6 <cross", true);
        assert!(!errors.is_empty());
        assert_eq!(relations.to_string(), "libc6 <cross");
    }

    #[test]
    fn test_parse_relaxed_unterminated_substvar() {
        let (relations, errors) = Relations::parse_relaxed("${shlibs:Depends", true);
        assert!(!errors.is_empty());
        assert_eq!(relations.to_string(), "${shlibs:Depends");
    }

    #[test]
    fn test_parse_relaxed_empty_substvar() {
        let (relations, errors) = Relations::parse_relaxed("${", true);
        assert!(!errors.is_empty());
        assert_eq!(relations.to_string(), "${");
    }

    #[test]
    fn test_parse_with_comments() {
        let input = "dh-python,\nlibsvn-dev,\n#               python-all-dbg (>= 2.6.6-3),\npython3-all-dev,\n#               python3-all-dbg,\npython3-docutils";
        let relations: Relations = input.parse().unwrap();
        let entries: Vec<_> = relations.entries().collect();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].to_string(), "dh-python");
        assert_eq!(entries[1].to_string(), "libsvn-dev");
        assert_eq!(entries[2].to_string(), "python3-all-dev");
        assert_eq!(entries[3].to_string(), "python3-docutils");
        // Round-trip preserves comments
        assert_eq!(relations.to_string(), input);
    }

    #[test]
    fn test_remove_entry_with_adjacent_comment() {
        let input = "dh-python,\n#  commented-out,\npython3-all-dev";
        let mut relations: Relations = input.parse().unwrap();
        assert_eq!(relations.entries().count(), 2);
        relations.remove_entry(0);
        assert_eq!(relations.entries().count(), 1);
        assert_eq!(
            relations.entries().next().unwrap().to_string(),
            "python3-all-dev"
        );
    }

    #[test]
    fn test_insert_entry_with_comments_present() {
        let input = "dh-python,\n#  commented-out,\npython3-all-dev";
        let mut relations: Relations = input.parse().unwrap();
        let new_entry: Entry = "libfoo-dev".parse().unwrap();
        relations.push(new_entry);
        // New entry should be appended
        let entries: Vec<_> = relations.entries().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].to_string(), "libfoo-dev");
    }

    #[test]
    fn test_drop_dependency_with_comments() {
        let input = "dh-python,\n#  commented-out,\npython3-all-dev,\nlibfoo-dev";
        let mut relations: Relations = input.parse().unwrap();
        assert!(relations.drop_dependency("python3-all-dev"));
        let entries: Vec<_> = relations.entries().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].to_string(), "dh-python");
        assert_eq!(entries[1].to_string(), "libfoo-dev");
    }

    #[test]
    fn test_relation_remove_first_with_malformed_tree() {
        // Relaxed parsing can produce ERROR nodes inside an entry.
        // Removing the first relation should not panic on unexpected tokens.
        let (relations, errors) = Relations::parse_relaxed("foo @ bar | baz", false);
        assert!(!errors.is_empty());
        let entry = relations.get_entry(0).unwrap();
        let mut relation = entry.get_relation(0).unwrap();
        // Should not panic
        relation.remove();
    }
}
