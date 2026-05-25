use rowan::{GreenNode, GreenNodeBuilder};
use std::fmt;

/// Check if an info string matches a pattern with wildcards
///
/// Supports `*` wildcards in patterns like `[debian/copyright:*]` matching `[debian/copyright:31]`
/// The asterisk matches arbitrary strings similar to shell wildcards.
pub fn info_matches(pattern: &str, value: &str) -> bool {
    if pattern == value {
        return true;
    }

    // Check if pattern contains wildcards
    if !pattern.contains('*') {
        return false;
    }

    // Split pattern by wildcards
    let parts: Vec<&str> = pattern.split('*').collect();

    // Check prefix (before first *)
    if !parts[0].is_empty() && !value.starts_with(parts[0]) {
        return false;
    }

    // Check suffix (after last *)
    if !parts[parts.len() - 1].is_empty() && !value.ends_with(parts[parts.len() - 1]) {
        return false;
    }

    // Check middle parts appear in order
    let mut pos = parts[0].len();
    for part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = value[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }

    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
#[repr(u16)]
/// Syntax kinds for lintian override files
pub enum SyntaxKind {
    /// Whitespace token
    WHITESPACE = 0,
    /// Comment token
    COMMENT,
    /// Package name token
    PACKAGE_NAME,
    /// Colon token
    COLON,
    /// Package type token
    PACKAGE_TYPE,
    /// Tag token
    TAG,
    /// Info token
    INFO,
    /// Newline token
    NEWLINE,

    /// Root node
    ROOT,
    /// Override line node
    OVERRIDE_LINE,
    /// Package specification node
    PACKAGE_SPEC,

    /// Error node
    ERROR,
}

use SyntaxKind::*;

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

/// Language type for the lintian override parser
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {}

impl rowan::Language for Lang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= ERROR as u16);
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

/// Syntax node type for lintian overrides
pub type SyntaxNode = rowan::SyntaxNode<Lang>;
/// Syntax token type for lintian overrides
pub type SyntaxToken = rowan::SyntaxToken<Lang>;
/// Syntax element type for lintian overrides
pub type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

/// The result of parsing a lintian-overrides file.
///
/// `PartialEq` / `Eq` compare the underlying `GreenNode` (cheap —
/// rowan green nodes are interned) and the errors, so this type
/// can be stored in a Salsa database. The manual `Send` / `Sync`
/// impls assert that `Parse` itself is thread-safe even when `T`
/// (the AST node type) wraps a non-thread-safe `SyntaxNode`: the
/// only field carrying real data is the `GreenNode`, which is
/// thread-safe. The phantom `T` exists for type-tagging only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse<T> {
    green: GreenNode,
    errors: Vec<String>,
    _phantom: std::marker::PhantomData<T>,
}

// SAFETY: The only data Parse holds is the GreenNode (thread-safe
// — that's the whole point of rowan's red-green split) and the
// errors Vec. The PhantomData<T> contributes no runtime data.
unsafe impl<T> Send for Parse<T> {}
unsafe impl<T> Sync for Parse<T> {}

impl<T> Parse<T> {
    fn new(green: GreenNode, errors: Vec<String>) -> Self {
        Parse {
            green,
            errors,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the syntax tree
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// Get the parse errors
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Convert to result, returning errors if any
    pub fn ok(self) -> Result<T, Vec<String>>
    where
        T: AstNode,
    {
        if self.errors.is_empty() {
            Ok(T::cast(self.syntax()).unwrap())
        } else {
            Err(self.errors)
        }
    }

    /// Return the parsed tree even when there are errors.
    ///
    /// Lintian-overrides parsing is resilient: the parser always
    /// produces a green tree (well-formed lines + recovered tokens
    /// for malformed ones), so consumers that want partial output —
    /// LSP semantic tokens, completion lookups, hover — should walk
    /// `tree()` rather than throwing the whole document away on the
    /// first parse error via `ok()`.
    pub fn tree(&self) -> T
    where
        T: AstNode,
    {
        T::cast(self.syntax()).expect("root node has wrong type")
    }
}

/// Trait for AST nodes
pub trait AstNode: Clone {
    /// Cast a syntax node to this AST node type
    fn cast(syntax: SyntaxNode) -> Option<Self>
    where
        Self: Sized;

    /// Get the underlying syntax node
    fn syntax(&self) -> &SyntaxNode;
}

/// The root node of a lintian-overrides file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintianOverrides {
    syntax: SyntaxNode,
}

impl AstNode for LintianOverrides {
    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if syntax.kind() == ROOT {
            Some(Self { syntax })
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }
}

impl LintianOverrides {
    /// Capture an independent snapshot of this lintian-overrides file.
    ///
    /// The returned value shares the underlying immutable green-node data
    /// with `self` at the time of the call, but lives in its own mutable
    /// tree: subsequent mutations to `self` do not propagate to the snapshot.
    /// Pair with [`Self::tree_eq`] to detect later mutations.
    pub fn snapshot(&self) -> Self {
        LintianOverrides {
            syntax: SyntaxNode::new_root_mut(self.syntax.green().into_owned()),
        }
    }

    /// Returns true iff the syntax trees of `self` and `other` are
    /// value-equal. An O(1) pointer-identity fast path makes this free for
    /// trees that still share state with a recent `snapshot()`.
    pub fn tree_eq(&self, other: &Self) -> bool {
        let a = self.syntax.green();
        let b = other.syntax.green();
        let a_ref: &rowan::GreenNodeData = &a;
        let b_ref: &rowan::GreenNodeData = &b;
        std::ptr::eq(a_ref as *const _, b_ref as *const _) || a_ref == b_ref
    }

    /// Parse a lintian-overrides file
    pub fn parse(text: &str) -> Parse<Self> {
        let (green, errors) = parse_lintian_overrides(text);
        Parse::new(green, errors)
    }

    /// Get all override lines
    pub fn lines(&self) -> impl Iterator<Item = OverrideLine> + '_ {
        self.syntax.children().filter_map(OverrideLine::cast)
    }

    /// Convert back to text
    pub fn text(&self) -> String {
        self.syntax.text().to_string()
    }

    /// Get a reference to the underlying syntax node
    pub fn syntax_node(&self) -> &SyntaxNode {
        &self.syntax
    }
}

impl fmt::Display for LintianOverrides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.syntax.text())
    }
}

/// A single override line
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideLine {
    syntax: SyntaxNode,
}

impl AstNode for OverrideLine {
    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if syntax.kind() == OVERRIDE_LINE {
            Some(Self { syntax })
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }
}

impl OverrideLine {
    /// Check if this line is a comment
    pub fn is_comment(&self) -> bool {
        self.syntax
            .children_with_tokens()
            .any(|it| matches!(it.as_token(), Some(token) if token.kind() == COMMENT))
    }

    /// Check if this line is empty
    pub fn is_empty(&self) -> bool {
        self.syntax
            .children_with_tokens()
            .all(|it| matches!(it.as_token(), Some(token) if token.kind() == WHITESPACE || token.kind() == NEWLINE))
    }

    /// Get the package specification if present
    pub fn package_spec(&self) -> Option<PackageSpec> {
        self.syntax.children().find_map(PackageSpec::cast)
    }

    /// Get the tag token
    pub fn tag(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == TAG)
    }

    /// Get the source range of the tag token, if present.
    pub fn tag_range(&self) -> Option<rowan::TextRange> {
        self.tag().map(|t| t.text_range())
    }

    /// Get the source range of this override line.
    pub fn text_range(&self) -> rowan::TextRange {
        self.syntax.text_range()
    }

    /// Get the info text
    pub fn info(&self) -> Option<String> {
        let tokens: Vec<_> = self
            .syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|it| it.kind() == INFO)
            .collect();

        if tokens.is_empty() {
            None
        } else {
            Some(
                tokens
                    .iter()
                    .map(|t| t.text())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
    }

    /// Get the source range spanned by the info tokens, if any are present.
    ///
    /// When multiple `INFO` tokens are present the range spans from the start
    /// of the first to the end of the last.
    pub fn info_range(&self) -> Option<rowan::TextRange> {
        let mut tokens = self
            .syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|it| it.kind() == INFO);
        let first = tokens.next()?;
        let last = tokens.last().unwrap_or_else(|| first.clone());
        Some(first.text_range().cover(last.text_range()))
    }

    /// Get the package name from the package spec, if present.
    pub fn package(&self) -> Option<String> {
        self.package_spec()?.package_name()
    }

    /// Get the text representation of this line
    pub fn text(&self) -> String {
        self.syntax.text().to_string()
    }

    /// Get the package type from the package spec (e.g., "source", "binary")
    /// The package spec can be in format "package-name type:" or just "type:"
    pub fn package_type(&self) -> Option<String> {
        let pkg_name = self.package_spec()?.package_name()?;
        // The package_name might be "blah source" or just "source"
        // Split on whitespace and take the last word as the type
        let parts: Vec<&str> = pkg_name.split_whitespace().collect();
        parts.last().map(|s| s.to_string())
    }

    /// Check if this override line matches a given issue described by its components.
    ///
    /// # Arguments
    /// * `issue_tag` - The lintian tag to match against
    /// * `issue_package` - The package name to match against
    /// * `issue_package_type` - The package type (e.g. "source", "binary") to match against
    /// * `issue_info` - Additional info to match against (supports wildcard matching)
    pub fn matches(
        &self,
        issue_tag: Option<&str>,
        issue_package: Option<&str>,
        issue_package_type: Option<&str>,
        issue_info: Option<&str>,
    ) -> bool {
        // Check if tag matches
        if let Some(tag) = self.tag() {
            if Some(tag.text()) != issue_tag {
                return false;
            }
        } else {
            return false;
        }

        // Check package name and/or type if specified in override
        if let Some(pkg_spec) = self.package_spec() {
            if let Some(pkg_name) = pkg_spec.package_name() {
                let parts: Vec<&str> = pkg_name.split_whitespace().collect();

                if parts.len() == 1 && (parts[0] == "binary" || parts[0] == "source") {
                    // Just "binary:" or "source:" — match on type only
                    if Some(parts[0]) != issue_package_type {
                        return false;
                    }
                } else if parts.len() == 2 && (parts[1] == "binary" || parts[1] == "source") {
                    // "package-name binary:" or "package-name source:"
                    if Some(parts[0]) != issue_package || Some(parts[1]) != issue_package_type {
                        return false;
                    }
                } else {
                    // Just a package name without explicit type
                    if Some(parts[0]) != issue_package {
                        return false;
                    }
                }
            }
        }

        // Check info if present on the issue
        if let Some(our_info) = issue_info {
            if let Some(override_info) = self.info() {
                let override_info = override_info.trim();
                if !info_matches(override_info, our_info) {
                    return false;
                }
            }
        }

        true
    }
}

/// Package specification (e.g., "package:" or "binary:")
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    syntax: SyntaxNode,
}

impl AstNode for PackageSpec {
    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if syntax.kind() == PACKAGE_SPEC {
            Some(Self { syntax })
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }
}

impl PackageSpec {
    /// Get the package name
    pub fn package_name(&self) -> Option<String> {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == PACKAGE_NAME)
            .map(|t| t.text().to_string())
    }

    /// Get the package type (source or binary)
    pub fn package_type(&self) -> Option<String> {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == PACKAGE_TYPE)
            .map(|t| t.text().to_string())
    }
}

/// Split `s` into alternating runs of non-whitespace and whitespace
/// characters. Concatenating the returned slices reproduces `s` exactly.
fn split_runs(s: &str) -> Vec<&str> {
    let mut runs = Vec::new();
    let mut start = 0;
    let mut in_ws = s.starts_with(char::is_whitespace);
    for (i, c) in s.char_indices() {
        let here_ws = c.is_whitespace();
        if here_ws != in_ws {
            runs.push(&s[start..i]);
            start = i;
            in_ws = here_ws;
        }
    }
    if start < s.len() {
        runs.push(&s[start..]);
    }
    runs
}

/// Parse a lintian-overrides file
fn parse_lintian_overrides(text: &str) -> (GreenNode, Vec<String>) {
    let mut builder = GreenNodeBuilder::new();
    let mut errors = Vec::new();

    builder.start_node(ROOT.into());

    // split_inclusive keeps the trailing '\n' on each line if it was present,
    // so we only emit a NEWLINE token when the source actually had one and
    // round-trip back to exactly the input text.
    for raw_line in text.split_inclusive('\n') {
        let (line, has_newline) = match raw_line.strip_suffix('\n') {
            Some(stripped) => (stripped, true),
            None => (raw_line, false),
        };
        parse_line(&mut builder, line, &mut errors);
        if has_newline {
            builder.token(NEWLINE.into(), "\n");
        }
    }

    builder.finish_node();
    (builder.finish(), errors)
}

fn parse_line(builder: &mut GreenNodeBuilder, line: &str, _errors: &mut Vec<String>) {
    builder.start_node(OVERRIDE_LINE.into());

    // Handle leading whitespace
    let trimmed_start = line.trim_start();
    let leading_ws = &line[..line.len() - trimmed_start.len()];
    if !leading_ws.is_empty() {
        builder.token(WHITESPACE.into(), leading_ws);
    }

    // Check for comment
    if trimmed_start.starts_with('#') {
        builder.token(COMMENT.into(), trimmed_start);
        builder.finish_node();
        return;
    }

    // Empty line
    if trimmed_start.is_empty() {
        builder.finish_node();
        return;
    }

    // Parse the override line
    let mut current_start = 0;

    // First, check if we have a package spec by looking for a colon
    // The package spec format is "package-name:" or "package-name type:"
    // We need to distinguish this from info that may contain colons (e.g., "line 51:")
    // A package spec will have:
    // 1. A colon followed by whitespace or end-of-line
    // 2. The part before the colon should be a reasonable package spec (1-2 words)
    let mut has_package_spec = false;
    let mut colon_pos = 0;

    if let Some(pos) = trimmed_start.find(':') {
        // Check if the colon is followed by whitespace or is at the end
        let after_colon = &trimmed_start[pos + 1..];
        if after_colon.is_empty() || after_colon.starts_with(char::is_whitespace) {
            // Check if the part before the colon looks like a package spec
            // It should be 1-2 words (package name, optionally with "source" or "binary")
            let before_colon = &trimmed_start[..pos];
            let words_before: Vec<&str> = before_colon.split_whitespace().collect();

            // Valid package specs:
            // - Single word: "source:", "binary:", "package-name:"
            // - Two words: "source package-name:", "binary package-name:", "package-name source:", "package-name binary:"
            let is_valid_package_spec = match words_before.len() {
                1 => true, // Single word is always valid
                2 => {
                    // Two words: either first or second must be "source" or "binary"
                    words_before[0] == "source"
                        || words_before[0] == "binary"
                        || words_before[1] == "source"
                        || words_before[1] == "binary"
                }
                _ => false, // More than 2 words is never a valid package spec
            };

            if is_valid_package_spec {
                // This looks like a valid package spec
                has_package_spec = true;
                colon_pos = pos;
            }
        }
    }

    if has_package_spec {
        // Found package spec - parse it into package name and optionally package type
        builder.start_node(PACKAGE_SPEC.into());

        let package_spec_text = &trimmed_start[current_start..colon_pos];

        // Walk the spec text run by run so we can emit each word and the
        // whitespace between them as separate tokens (the previous code
        // hard-coded a single space and dropped any other interior or
        // trailing whitespace). Valid spec layouts:
        //   "package-name", "source", "binary",
        //   "package-name source", "package-name binary",
        //   "source package-name", "binary package-name".
        // In a two-word spec the "source"/"binary" word is the type and the
        // other is the name; otherwise everything is a name.
        let runs = split_runs(package_spec_text);
        let two_words = runs
            .iter()
            .filter(|r| !r.starts_with(char::is_whitespace))
            .count()
            == 2;
        for run in runs {
            let kind = if run.starts_with(char::is_whitespace) {
                WHITESPACE
            } else if two_words && (run == "source" || run == "binary") {
                PACKAGE_TYPE
            } else {
                PACKAGE_NAME
            };
            builder.token(kind.into(), run);
        }

        builder.token(COLON.into(), ":");
        builder.finish_node();

        current_start = colon_pos + 1;

        // Skip any whitespace after colon
        let after_colon = &trimmed_start[current_start..];
        let trimmed_after = after_colon.trim_start();
        let ws_len = after_colon.len() - trimmed_after.len();
        if ws_len > 0 {
            builder.token(WHITESPACE.into(), &after_colon[..ws_len]);
            current_start += ws_len;
        }
    }

    // The remainder is: [whitespace] tag [whitespace info-spanning-the-rest].
    // Info may itself contain whitespace, so we don't tokenise inside it —
    // it's a single INFO token from its first byte to the end of `rest`.
    let rest = &trimmed_start[current_start..];
    let ws_end = rest.len() - rest.trim_start().len();
    if ws_end > 0 {
        builder.token(WHITESPACE.into(), &rest[..ws_end]);
    }
    let after_ws = &rest[ws_end..];
    if after_ws.is_empty() {
        builder.finish_node();
        return;
    }
    let tag_end = after_ws.find(char::is_whitespace).unwrap_or(after_ws.len());
    builder.token(TAG.into(), &after_ws[..tag_end]);
    let after_tag = &after_ws[tag_end..];
    if after_tag.is_empty() {
        builder.finish_node();
        return;
    }
    let info_start = after_tag.len() - after_tag.trim_start().len();
    if info_start > 0 {
        builder.token(WHITESPACE.into(), &after_tag[..info_start]);
    }
    let info = &after_tag[info_start..];
    if !info.is_empty() {
        builder.token(INFO.into(), info);
    }

    builder.finish_node();
}

/// Builder for creating/modifying lintian-overrides files
pub struct LintianOverridesBuilder<'a> {
    builder: GreenNodeBuilder<'a>,
}

impl<'a> LintianOverridesBuilder<'a> {
    /// Create a new builder
    pub fn new() -> Self {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ROOT.into());
        Self { builder }
    }

    /// Add a comment line
    pub fn add_comment(&mut self, text: &str) -> &mut Self {
        self.builder.start_node(OVERRIDE_LINE.into());
        self.builder.token(COMMENT.into(), text);
        self.builder.finish_node();
        self.builder.token(NEWLINE.into(), "\n");
        self
    }

    /// Add an override line
    pub fn add_override(
        &mut self,
        package: Option<&str>,
        tag: &str,
        info: Option<&str>,
    ) -> &mut Self {
        self.builder.start_node(OVERRIDE_LINE.into());

        if let Some(pkg) = package {
            self.builder.start_node(PACKAGE_SPEC.into());
            self.builder.token(PACKAGE_NAME.into(), pkg);
            self.builder.token(COLON.into(), ":");
            self.builder.finish_node();
            self.builder.token(WHITESPACE.into(), " ");
        }

        self.builder.token(TAG.into(), tag);

        if let Some(info_text) = info {
            self.builder.token(WHITESPACE.into(), " ");
            self.builder.token(INFO.into(), info_text);
        }

        self.builder.finish_node();
        self.builder.token(NEWLINE.into(), "\n");
        self
    }

    /// Finish building and return the LintianOverrides
    pub fn finish(mut self) -> LintianOverrides {
        self.builder.finish_node();
        let green = self.builder.finish();
        LintianOverrides {
            syntax: SyntaxNode::new_root(green),
        }
    }
}

impl<'a> Default for LintianOverridesBuilder<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// Copy a syntax node into a green node builder
pub fn copy_node(builder: &mut GreenNodeBuilder, node: &SyntaxNode) {
    builder.start_node(node.kind().into());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                builder.token(token.kind().into(), token.text());
            }
            rowan::NodeOrToken::Node(child_node) => {
                copy_node(builder, &child_node);
            }
        }
    }
    builder.finish_node();
}

/// Filter override lines based on a predicate
pub fn filter_overrides<F>(overrides: &LintianOverrides, mut predicate: F) -> LintianOverrides
where
    F: FnMut(&OverrideLine) -> bool,
{
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(ROOT.into());

    for line_node in overrides.syntax.children() {
        if line_node.kind() == OVERRIDE_LINE {
            let line = OverrideLine {
                syntax: line_node.clone(),
            };

            if predicate(&line) {
                copy_node(&mut builder, &line_node);
                builder.token(NEWLINE.into(), "\n");
            }
        }
    }

    builder.finish_node();
    let green = builder.finish();
    LintianOverrides {
        syntax: SyntaxNode::new_root(green),
    }
}

/// Rebuild `overrides` with each line's TAG token rewritten by `rename`.
///
/// Lines whose tag is unchanged keep their original tokens (whitespace,
/// comments, package spec) verbatim — only the TAG token is replaced.
/// `rename` is invoked once per override line that has a tag, and should
/// return the new tag text, or `None` to leave the tag as-is.
///
/// # Example
/// ```
/// use lintian_overrides::{LintianOverrides, rename_tags};
/// let parsed = LintianOverrides::parse("# keep\nold-tag info\n");
/// let overrides = parsed.ok().unwrap();
/// let renamed = rename_tags(&overrides, |tag| {
///     if tag == "old-tag" { Some("new-tag".to_string()) } else { None }
/// });
/// assert_eq!(renamed.text(), "# keep\nnew-tag info\n");
/// ```
pub fn rename_tags<F>(overrides: &LintianOverrides, mut rename: F) -> LintianOverrides
where
    F: FnMut(&str) -> Option<String>,
{
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(ROOT.into());

    for child in overrides.syntax.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(node) if node.kind() == OVERRIDE_LINE => {
                let line = OverrideLine {
                    syntax: node.clone(),
                };
                let new_tag = line.tag().and_then(|t| rename(t.text()));
                if let Some(new_tag) = new_tag {
                    builder.start_node(OVERRIDE_LINE.into());
                    for element in line.syntax.children_with_tokens() {
                        match element {
                            rowan::NodeOrToken::Token(token) if token.kind() == TAG => {
                                builder.token(TAG.into(), &new_tag);
                            }
                            rowan::NodeOrToken::Token(token) => {
                                builder.token(token.kind().into(), token.text());
                            }
                            rowan::NodeOrToken::Node(child_node) => {
                                copy_node(&mut builder, &child_node);
                            }
                        }
                    }
                    builder.finish_node();
                } else {
                    copy_node(&mut builder, &node);
                }
            }
            rowan::NodeOrToken::Node(node) => {
                copy_node(&mut builder, &node);
            }
            rowan::NodeOrToken::Token(token) => {
                builder.token(token.kind().into(), token.text());
            }
        }
    }

    builder.finish_node();
    let green = builder.finish();
    LintianOverrides {
        syntax: SyntaxNode::new_root(green),
    }
}

/// Map override lines using a transformation function
/// Returns a new LintianOverrides with the lines transformed by the function
/// If the function returns None, the original line is kept unchanged
pub fn map_overrides<F>(overrides: &LintianOverrides, mut transform: F) -> LintianOverrides
where
    F: FnMut(&OverrideLine) -> Option<(Option<String>, Option<String>, String, Option<String>)>,
{
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(ROOT.into());

    for line in overrides.lines() {
        // Try to transform the line
        if let Some((package, package_type, tag, info)) = transform(&line) {
            // Build a new override line with the transformed values
            builder.start_node(OVERRIDE_LINE.into());

            if let Some(pkg) = package {
                builder.start_node(PACKAGE_SPEC.into());
                builder.token(PACKAGE_NAME.into(), &pkg);
                if let Some(ptype) = package_type {
                    builder.token(WHITESPACE.into(), " ");
                    builder.token(PACKAGE_TYPE.into(), &ptype);
                }
                builder.token(COLON.into(), ":");
                builder.finish_node();
                builder.token(WHITESPACE.into(), " ");
            }

            builder.token(TAG.into(), &tag);

            if let Some(info_text) = info {
                builder.token(WHITESPACE.into(), " ");
                builder.token(INFO.into(), &info_text);
            }

            builder.finish_node();
        } else {
            // Keep the original line unchanged
            copy_node(&mut builder, line.syntax());
        }
        builder.token(NEWLINE.into(), "\n");
    }

    builder.finish_node();
    let green = builder.finish();
    LintianOverrides {
        syntax: SyntaxNode::new_root(green),
    }
}

/// Find all lintian-overrides files in a debian directory
pub fn find_override_files(base_path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    // Check debian/source/lintian-overrides
    let source_overrides = base_path.join("debian/source/lintian-overrides");
    if source_overrides.exists() {
        files.push(source_overrides);
    }

    // Check debian/*.lintian-overrides
    let debian_dir = base_path.join("debian");
    if debian_dir.exists() && debian_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&debian_dir) {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if filename.ends_with(".lintian-overrides") {
                        files.push(entry.path());
                    }
                }
            }
        }
    }

    files
}

/// Iterate over all lintian override lines in a debian directory
pub fn iter_overrides(base_path: &std::path::Path) -> impl Iterator<Item = OverrideLine> {
    let files = find_override_files(base_path);

    files
        .into_iter()
        .flat_map(|override_file| {
            let content = std::fs::read_to_string(&override_file).ok()?;
            let parsed = LintianOverrides::parse(&content);
            let overrides = parsed.ok().ok()?;
            Some(overrides.lines().collect::<Vec<_>>())
        })
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_matches_exact() {
        assert!(info_matches("foo", "foo"));
        assert!(!info_matches("foo", "bar"));
    }

    #[test]
    fn test_info_matches_wildcard_simple() {
        assert!(info_matches("*", "anything"));
        assert!(info_matches("*", ""));
        assert!(info_matches("**", "anything"));
    }

    #[test]
    fn test_info_matches_wildcard_prefix() {
        assert!(info_matches("*.js", "file.js"));
        assert!(info_matches("*.js", "path/to/file.js"));
        assert!(!info_matches("*.js", "file.css"));
    }

    #[test]
    fn test_info_matches_wildcard_suffix() {
        assert!(info_matches("debian/*", "debian/control"));
        assert!(info_matches("debian/*", "debian/rules"));
        assert!(!info_matches("debian/*", "other/file"));
    }

    #[test]
    fn test_info_matches_wildcard_middle() {
        assert!(info_matches(
            "[debian/copyright:*]",
            "[debian/copyright:31]"
        ));
        assert!(info_matches(
            "[debian/copyright:*]",
            "[debian/copyright:100]"
        ));
        assert!(!info_matches("[debian/copyright:*]", "[debian/rules:31]"));
        assert!(!info_matches("[debian/copyright:*]", "debian/copyright:31"));
    }

    #[test]
    fn test_info_matches_multiple_wildcards() {
        assert!(info_matches("*.html.*.js", "foo.html.bar.js"));
        assert!(info_matches("*.html.*.js", "foo.html.baz.qux.js"));
        assert!(!info_matches("*.html.*.js", "foo.css.bar.js"));
    }

    #[test]
    fn test_info_matches_wildcard_empty_parts() {
        assert!(info_matches("foo**bar", "foobar"));
        assert!(info_matches("foo**bar", "fooxyzbar"));
    }

    #[test]
    fn test_parse_simple_override() {
        let text = "some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(lines[0].info(), None);
    }

    #[test]
    fn test_parse_override_with_info() {
        let text = "some-tag some extra info\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(lines[0].info(), Some("some extra info".to_string()));
    }

    #[test]
    fn test_parse_package_override() {
        let text = "package-name: some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(
            lines[0].package_spec().unwrap().package_name().unwrap(),
            "package-name"
        );
    }

    #[test]
    fn test_parse_comment() {
        let text = "# This is a comment\nsome-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].is_comment());
        assert_eq!(lines[1].tag().unwrap().text(), "some-tag");
    }

    #[test]
    fn test_roundtrip() {
        let text = "# Comment\npackage: some-tag info\nanother-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        assert_eq!(overrides.text(), text);
    }

    #[test]
    fn test_builder() {
        let mut builder = LintianOverridesBuilder::new();
        builder.add_comment("# Test comment");
        builder.add_override(Some("mypackage"), "some-tag", Some("with info"));
        builder.add_override(None, "another-tag", None);
        let overrides = builder.finish();

        let text = overrides.text();
        assert!(text.contains("# Test comment"));
        assert!(text.contains("mypackage: some-tag with info"));
        assert!(text.contains("another-tag"));
    }

    #[test]
    fn test_parse_info_with_colon() {
        // Test that info fields containing colons are parsed correctly
        // This was a bug where "X-Python-Version: >= 2.5" would be misparsed
        let text = "ancient-python-version-field X-Python-Version: >= 2.5\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].tag().unwrap().text(),
            "ancient-python-version-field"
        );
        assert_eq!(
            lines[0].info(),
            Some("X-Python-Version: >= 2.5".to_string())
        );
        assert_eq!(lines[0].package_spec(), None);
    }

    #[test]
    fn test_parse_source_prefix_with_info_containing_colon() {
        // Test parsing with explicit "source:" prefix and info containing colon
        let text = "source: ancient-python-version-field X-Python-Version: >= 2.5\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].tag().unwrap().text(),
            "ancient-python-version-field"
        );
        assert_eq!(
            lines[0].info(),
            Some("X-Python-Version: >= 2.5".to_string())
        );
        assert_eq!(
            lines[0].package_spec().unwrap().package_name().unwrap(),
            "source"
        );
    }

    #[test]
    fn test_parse_two_word_non_package_spec() {
        // Test that two words before a colon that don't match package spec pattern
        // are not treated as a package spec
        let text = "some-tag field-name: value\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(lines[0].info(), Some("field-name: value".to_string()));
        assert_eq!(lines[0].package_spec(), None);
    }

    #[test]
    fn test_filter_overrides_preserves_newlines() {
        let text = "# Comment\ntag1\ntag2\ntag3\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.ok().unwrap();

        // Filter out tag2
        let filtered = filter_overrides(&overrides, |line| {
            if let Some(tag) = line.tag() {
                tag.text() != "tag2"
            } else {
                true // Keep comments and empty lines
            }
        });

        let result = filtered.to_string();
        let expected = "# Comment\ntag1\ntag3\n";

        assert_eq!(
            result, expected,
            "Newlines should be preserved after filtering"
        );
    }

    #[test]
    fn test_rename_tags_preserves_structure() {
        let text = "# keep this comment\npkg source: old-tag some info\nother-tag\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.ok().unwrap();
        let renamed = rename_tags(&overrides, |tag| match tag {
            "old-tag" => Some("new-tag".to_string()),
            _ => None,
        });
        assert_eq!(
            renamed.text(),
            "# keep this comment\npkg source: new-tag some info\nother-tag\n"
        );
    }

    #[test]
    fn test_rename_tags_no_match_no_change() {
        let text = "tag1\ntag2\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.ok().unwrap();
        let renamed = rename_tags(&overrides, |_| None);
        assert_eq!(renamed.text(), text);
    }

    #[test]
    fn test_filter_overrides_with_info() {
        let text = "pkg source: tag1\npkg source: tag2 info\npkg source: tag3\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.ok().unwrap();

        // Filter out tag2
        let filtered = filter_overrides(&overrides, |line| {
            if let Some(tag) = line.tag() {
                tag.text() != "tag2"
            } else {
                true
            }
        });

        let result = filtered.to_string();
        let expected = "pkg source: tag1\npkg source: tag3\n";

        assert_eq!(
            result, expected,
            "Newlines should be preserved with package specs and info"
        );
    }

    #[test]
    fn test_parse_round_trip_without_trailing_newline() {
        // Regression: the parser used to append a NEWLINE token for every
        // line including non-newline-terminated trailers, so the round-trip
        // gained an extra '\n'.
        for input in [
            "",
            "\x0b",
            "tag info",
            "tag info\n",
            "tag info\ntag2 info\n",
            "tag info\ntag2 info",
            // Tag alone, with trailing whitespace that must round-trip.
            " \x12  ",
            "tag  ",
            "tag\t\t",
            // Package-spec text with unusual interior whitespace that the
            // previous hard-coded " " between words used to mangle.
            "0]\x0b:",
            "foo\tsource:",
            "binary  foo:",
        ] {
            let parsed = LintianOverrides::parse(input).tree();
            assert_eq!(parsed.text(), input, "round-trip differs for {:?}", input);
        }
    }
}
