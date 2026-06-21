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
    /// Left bracket token for Architecture
    L_BRACKET,
    /// Right bracket token for Architecture
    R_BRACKET,
    /// Single architecture token
    ARCH,
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

/// The package type keywords valid in a lintian-overrides spec.
const PACKAGE_TYPES: &[&str] = &["source", "binary", "udeb"];

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
    error_offsets: Vec<rowan::TextSize>,
    _phantom: std::marker::PhantomData<T>,
}

// SAFETY: The only data Parse holds is the GreenNode (thread-safe
// — that's the whole point of rowan's red-green split) and the
// errors Vec. The PhantomData<T> contributes no runtime data.
unsafe impl<T> Send for Parse<T> {}
unsafe impl<T> Sync for Parse<T> {}

impl<T> Parse<T> {
    fn new(green: GreenNode, errors: Vec<(String, rowan::TextSize)>) -> Self {
        let (errors, error_offsets) = errors.into_iter().unzip();
        Parse {
            green,
            errors,
            error_offsets,
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

    /// Get the parse errors together with their byte offsets in the source.
    pub fn errors_with_offsets(&self) -> impl Iterator<Item = (&str, rowan::TextSize)> {
        self.errors
            .iter()
            .map(|s| s.as_str())
            .zip(self.error_offsets.iter().copied())
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

    /// Return the package name of the override line at the given offset, if any.
    pub fn package_name_at_offset(&self, offset: rowan::TextSize) -> Option<String> {
        self.lines()
            .find(|line| line.syntax().text_range().contains(offset))?
            .package_spec()
            .filter(|spec| spec.contains_offset(offset))?
            .package_name()
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

    /// Whether this line carries a spec-terminating colon.
    pub fn has_colon(&self) -> bool {
        self.package_spec().is_some()
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

    /// Get the package type from the package spec (e.g., "source", "binary", "udeb")
    /// The package spec can be in format "package-name type:", "package-name [ archlist ] type", or just "type:"
    pub fn package_type(&self) -> Option<String> {
        self.package_spec()?.package_type()
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
            // Match on package name if the override specifies one.
            if let Some(pkg_name) = pkg_spec.package_name() {
                if Some(pkg_name.as_str()) != issue_package {
                    return false;
                }
            }
            // Match on package type if the override specifies one.
            if let Some(pkg_type) = pkg_spec.package_type() {
                if Some(pkg_type.as_str()) != issue_package_type {
                    return false;
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

    /// Get all architecture tokens inside the bracket list.
    pub fn arch_list(&self) -> Vec<String> {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|it| it.kind() == ARCH)
            .map(|t| t.text().to_string())
            .collect()
    }

    /// Whether the given offset falls within this package spec's text range.
    pub fn contains_offset(&self, offset: rowan::TextSize) -> bool {
        self.syntax.text_range().contains(offset)
    }

    /// Whether this spec has an architecture restriction list.
    pub fn has_arch_list(&self) -> bool {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .any(|t| t.kind() == L_BRACKET)
    }

    /// Whether `offset` falls within the `[ ... ]` architecture list,
    pub fn arch_list_contains_offset(&self, offset: rowan::TextSize) -> bool {
        let open = self
            .syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == L_BRACKET);
        let close = self
            .syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == R_BRACKET);
        match (open, close) {
            (Some(o), Some(c)) => {
                offset > o.text_range().start() && offset <= c.text_range().start()
            }
            // Brackets are always closed once a spec is recognised, but stay
            // defensive: an open bracket alone still means "inside the list".
            (Some(o), None) => offset > o.text_range().start(),
            _ => false,
        }
    }

    /// The text range of the package-type keyword (`source` / `binary` /
    /// `udeb`), if present.
    pub fn package_type_range(&self) -> Option<rowan::TextRange> {
        self.syntax
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == PACKAGE_TYPE)
            .map(|t| t.text_range())
    }
}

/// Parse a lintian-overrides file.
///
/// Parsing is resilient: a syntax tree is always produced, and malformed lines
/// are recorded as errors (with their byte offset) and wrapped in an ERROR
/// node so callers can do partial processing of the rest of the file.
fn parse_lintian_overrides(text: &str) -> (GreenNode, Vec<(String, rowan::TextSize)>) {
    let mut builder = GreenNodeBuilder::new();
    let mut errors = Vec::new();
    let mut offset = rowan::TextSize::from(0);

    builder.start_node(ROOT.into());

    // split_inclusive keeps the trailing '\n' on each line if it was present,
    // so we only emit a NEWLINE token when the source actually had one and
    // round-trip back to exactly the input text.
    for raw_line in text.split_inclusive('\n') {
        let (line, has_newline) = match raw_line.strip_suffix('\n') {
            Some(stripped) => (stripped, true),
            None => (raw_line, false),
        };
        parse_line(&mut builder, line, offset, &mut errors);
        offset += rowan::TextSize::of(line);
        if has_newline {
            builder.token(NEWLINE.into(), "\n");
            offset += rowan::TextSize::from(1);
        }
    }

    builder.finish_node();
    (builder.finish(), errors)
}

/// Build the OVERRIDE_LINE node for a single line.
///
/// The line is lexed in one forward pass and the resulting tokens are grouped:
/// leading whitespace and everything from the tag onwards sit directly under
/// the line, while the package spec (up to and including its colon) is wrapped
/// in a PACKAGE_SPEC node. A non-empty, non-comment line that carries no tag,
/// or one with an unterminated arch bracket, is wrapped in an ERROR node and
/// recorded in `errors`.
fn parse_line(
    builder: &mut GreenNodeBuilder,
    line: &str,
    offset: rowan::TextSize,
    errors: &mut Vec<(String, rowan::TextSize)>,
) {
    builder.start_node(OVERRIDE_LINE.into());

    let lexed = lex_line(line);
    let has_content = lexed
        .tokens
        .iter()
        .any(|(kind, _)| !matches!(kind, WHITESPACE | COMMENT));
    let has_tag = lexed.tokens.iter().any(|(kind, _)| *kind == TAG);

    // A line with content but no tag, or one with a lexing error, is malformed:
    // record it and wrap its tokens in an ERROR node so the region is visible.
    let error = lexed
        .error
        .or_else(|| (has_content && !has_tag).then(|| "missing lintian tag".to_string()));
    let wrap_error = error.is_some();
    if let Some(msg) = error {
        errors.push((msg, offset));
    }

    // Leading whitespace stays directly under the override line.
    let tokens = &lexed.tokens;
    let mut start = 0;
    if let Some((WHITESPACE, ws)) = tokens.first() {
        builder.token(WHITESPACE.into(), ws);
        start = 1;
    }

    if wrap_error {
        builder.start_node(ERROR.into());
    }

    let body = &tokens[start..];
    match body.iter().position(|(kind, _)| *kind == COLON) {
        Some(colon) => {
            builder.start_node(PACKAGE_SPEC.into());
            for (kind, text) in &body[..=colon] {
                builder.token((*kind).into(), text);
            }
            builder.finish_node();
            for (kind, text) in &body[colon + 1..] {
                builder.token((*kind).into(), text);
            }
        }
        None => {
            for (kind, text) in body {
                builder.token((*kind).into(), text);
            }
        }
    }

    if wrap_error {
        builder.finish_node();
    }

    builder.finish_node();
}

/// The result of lexing a single line: its tokens and an optional error
/// message for a malformed line (e.g. an unterminated arch bracket).
struct LexedLine<'a> {
    tokens: Vec<(SyntaxKind, &'a str)>,
    error: Option<String>,
}

/// Lex a single line into a flat token stream in one forward pass.
///
/// The line grammar is `[ws] package-spec? [ws] tag [ws info]?`, where
/// `package-spec = [name] [ws] ["[" archlist "]"] [ws] [type] ":"`.
///
/// The colon is ambiguous: it only opens a package spec when the text up to it
/// is itself a valid spec (otherwise the colon belongs to the info, as in
/// `line 51:`). We resolve this by lexing the prefix speculatively; if the scan
/// reaches a terminating colon those tokens are kept, otherwise they are
/// dropped and the whole line is read as a bare tag plus info.
fn lex_line(line: &str) -> LexedLine<'_> {
    let mut tokens = Vec::new();

    let trimmed = line.trim_start();
    let leading_ws = &line[..line.len() - trimmed.len()];
    if !leading_ws.is_empty() {
        tokens.push((WHITESPACE, leading_ws));
    }

    if trimmed.is_empty() {
        return LexedLine {
            tokens,
            error: None,
        };
    }
    if trimmed.starts_with('#') {
        tokens.push((COMMENT, trimmed));
        return LexedLine {
            tokens,
            error: None,
        };
    }

    // Try a leading package spec; fall back to treating the line as tag + info.
    let mut error = None;
    let rest = match lex_package_spec(trimmed) {
        SpecScan::Spec(spec, after) => {
            tokens.extend(spec);
            after
        }
        SpecScan::Unterminated => {
            error = Some("unterminated architecture list".to_string());
            trimmed
        }
        SpecScan::None => trimmed,
    };

    // The remainder is [ws] tag [ws info]. Info may contain whitespace and
    // colons, so it is a single token spanning to the end of the line.
    let rest = push_ws(&mut tokens, rest);
    if !rest.is_empty() {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        tokens.push((TAG, &rest[..end]));
        let info = push_ws(&mut tokens, &rest[end..]);
        if !info.is_empty() {
            tokens.push((INFO, info));
        }
    }

    LexedLine { tokens, error }
}

/// The outcome of scanning a line for a leading package spec.
enum SpecScan<'a> {
    /// A valid spec: its tokens and the remainder after the colon.
    Spec(Vec<(SyntaxKind, &'a str)>, &'a str),
    /// A spec-like prefix with an opening `[` but no closing `]`.
    Unterminated,
    /// No package spec; the line is a bare tag plus info.
    None,
}

/// Lex a leading package spec terminated by `:`.
fn lex_package_spec(line: &str) -> SpecScan<'_> {
    let mut tokens = Vec::new();
    let mut rest = line;

    // Optional package name. A lone type keyword denotes the type, not a name,
    // so a bare type keyword directly before the colon is emitted as the type.
    if !rest.starts_with('[') {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '[' || c == ':')
            .unwrap_or(rest.len());
        let word = &rest[..end];
        let after = rest[end..].trim_start();
        if word.is_empty() {
            return SpecScan::None;
        }
        if PACKAGE_TYPES.contains(&word) && after.starts_with(':') {
            tokens.push((PACKAGE_TYPE, word));
            return finish_spec(tokens, after);
        }
        tokens.push((PACKAGE_NAME, word));
        rest = push_ws(&mut tokens, &rest[end..]);
    }

    // Optional architecture list enclosed in brackets. An opening bracket that
    // is part of a spec-like line but is never closed is a malformed spec, not
    // info that happens to contain a `[`.
    if rest.starts_with('[') {
        let Some(end) = rest.find(']') else {
            return SpecScan::Unterminated;
        };
        tokens.push((L_BRACKET, "["));
        let mut arch = &rest[1..end];
        loop {
            arch = push_ws(&mut tokens, arch);
            if arch.is_empty() {
                break;
            }
            let word_end = arch.find(char::is_whitespace).unwrap_or(arch.len());
            tokens.push((ARCH, &arch[..word_end]));
            arch = &arch[word_end..];
        }
        tokens.push((R_BRACKET, "]"));
        rest = push_ws(&mut tokens, &rest[end + 1..]);
    }

    // Optional trailing package type keyword.
    if !rest.starts_with(':') {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == ':')
            .unwrap_or(rest.len());
        let word = &rest[..end];
        if !PACKAGE_TYPES.contains(&word) {
            return SpecScan::None;
        }
        tokens.push((PACKAGE_TYPE, word));
        rest = push_ws(&mut tokens, &rest[end..]);
    }

    finish_spec(tokens, rest)
}

/// Commit the spec when `rest` starts with the terminating colon. A spec colon
/// is followed by whitespace or end of line; a colon glued to more text (e.g.
/// `foo:bar`) is info, not a spec terminator.
fn finish_spec<'a>(mut tokens: Vec<(SyntaxKind, &'a str)>, rest: &'a str) -> SpecScan<'a> {
    let Some(after) = rest.strip_prefix(':') else {
        return SpecScan::None;
    };
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return SpecScan::None;
    }
    tokens.push((COLON, ":"));
    SpecScan::Spec(tokens, after)
}

/// Split leading whitespace off `rest`, emit it as a WHITESPACE token, and
/// return the remainder.
fn push_ws<'a>(tokens: &mut Vec<(SyntaxKind, &'a str)>, rest: &'a str) -> &'a str {
    let trimmed = rest.trim_start();
    let ws = &rest[..rest.len() - trimmed.len()];
    if !ws.is_empty() {
        tokens.push((WHITESPACE, ws));
    }
    trimmed
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

/// Find all lintian-overrides files in a debian directory.
///
/// Returns the `debian/source/lintian-overrides` file (if present) followed
/// by every `debian/*.lintian-overrides` file. An I/O error reading the
/// `debian` directory is propagated rather than silently treated as "no
/// override files": a caller that uses overrides to suppress a fix must not
/// mistake an unreadable directory for an absent one.
pub fn try_find_override_files(
    base_path: &std::path::Path,
) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();

    let source_overrides = base_path.join("debian/source/lintian-overrides");
    if source_overrides.exists() {
        files.push(source_overrides);
    }

    let debian_dir = base_path.join("debian");
    match std::fs::read_dir(&debian_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if let Some(filename) = entry.file_name().to_str() {
                    if filename.ends_with(".lintian-overrides") {
                        files.push(entry.path());
                    }
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    Ok(files)
}

/// Collect all lintian override lines in a debian directory.
///
/// An I/O error finding or reading any override file is propagated. This
/// matters for callers that decide whether to apply a fix: silently
/// dropping an unreadable override would treat the issue as un-suppressed
/// and apply a change the maintainer had overridden. Parse errors within a
/// file are tolerated, since the resilient parser still yields every
/// well-formed line, so a malformed sibling line does not discard valid
/// overrides.
pub fn try_iter_overrides(base_path: &std::path::Path) -> std::io::Result<Vec<OverrideLine>> {
    let mut lines = Vec::new();
    for override_file in try_find_override_files(base_path)? {
        let content = std::fs::read_to_string(&override_file)?;
        let parsed = LintianOverrides::parse(&content);
        lines.extend(parsed.tree().lines());
    }
    Ok(lines)
}

/// Find all lintian-overrides files in a debian directory.
///
/// Deprecated: this silently discards any I/O error reading the `debian`
/// directory, so an unreadable directory is indistinguishable from an
/// absent one. Use [`try_find_override_files`] instead.
#[deprecated(
    since = "0.1.6",
    note = "silently swallows I/O errors; use try_find_override_files"
)]
pub fn find_override_files(base_path: &std::path::Path) -> Vec<std::path::PathBuf> {
    try_find_override_files(base_path).unwrap_or_default()
}

/// Iterate over all lintian override lines in a debian directory.
///
/// Deprecated: this silently drops any override file it cannot read, so a
/// caller relying on an override to suppress a fix may apply the change
/// anyway. Use [`try_iter_overrides`] instead.
#[deprecated(
    since = "0.1.6",
    note = "silently swallows I/O errors; use try_iter_overrides"
)]
pub fn iter_overrides(base_path: &std::path::Path) -> impl Iterator<Item = OverrideLine> {
    try_iter_overrides(base_path)
        .unwrap_or_default()
        .into_iter()
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

        // A bare "source" prefix is the package type, not a package name.
        assert_eq!(lines[0].package_spec().unwrap().package_name(), None);
        assert_eq!(
            lines[0].package_spec().unwrap().package_type().unwrap(),
            "source"
        );
    }

    #[test]
    fn test_bare_source_type_matches_source_issue() {
        // A "source: tag info" override has no package name; it must match a
        // source-type issue whose package is unspecified.
        let text = "source: debian-files-list-in-source debian/files\n";
        let overrides = LintianOverrides::parse(text).ok().unwrap();
        let line = overrides.lines().next().unwrap();

        let spec = line.package_spec().unwrap();
        assert_eq!(spec.package_name(), None);
        assert_eq!(spec.package_type().unwrap(), "source");

        assert!(line.matches(
            Some("debian-files-list-in-source"),
            None,
            Some("source"),
            Some("debian/files"),
        ));
    }

    #[test]
    fn test_named_source_package_spec() {
        // "foo source: tag" names package foo of type source.
        let text = "foo source: some-tag\n";
        let overrides = LintianOverrides::parse(text).ok().unwrap();
        let line = overrides.lines().next().unwrap();

        let spec = line.package_spec().unwrap();
        assert_eq!(spec.package_name().unwrap(), "foo");
        assert_eq!(spec.package_type().unwrap(), "source");
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

    #[test]
    fn test_parse_arch_list() {
        let text = "foo [amd64]: some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(
            lines[0].package_spec().unwrap().package_name().unwrap(),
            "foo"
        );
        assert_eq!(lines[0].package_spec().unwrap().arch_list(), vec!["amd64"]);
    }

    #[test]
    fn test_parse_arch_list_multiple() {
        let text = "foo [amd64 arm64]: some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].package_spec().unwrap().arch_list(),
            vec!["amd64", "arm64"]
        );
    }

    #[test]
    fn test_parse_arch_list_negation() {
        let text = "foo [!amd64]: some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        // The '!' is part of the ARCH token text.
        assert_eq!(lines[0].package_spec().unwrap().arch_list(), vec!["!amd64"]);
    }

    #[test]
    fn test_parse_arch_list_with_type() {
        let text = "foo [amd64] binary: some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].package_spec().unwrap().package_name().unwrap(),
            "foo"
        );
        assert_eq!(lines[0].package_spec().unwrap().arch_list(), vec!["amd64"]);
        assert_eq!(
            lines[0].package_spec().unwrap().package_type().unwrap(),
            "binary"
        );
    }

    #[test]
    fn test_parse_arch_list_only() {
        // No package name, just arch list.
        let text = "[linux-any]: some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        assert_eq!(
            lines[0].package_spec().unwrap().arch_list(),
            vec!["linux-any"]
        );
        assert_eq!(lines[0].package_spec().unwrap().package_name(), None);
    }

    #[test]
    fn test_parse_arch_list_no_arch() {
        // A line without an arch list returns an empty vec.
        let text = "foo: some-tag\n";
        let parsed = LintianOverrides::parse(text);
        assert!(parsed.errors().is_empty());

        let overrides = parsed.ok().unwrap();
        let lines: Vec<_> = overrides.lines().collect();

        assert_eq!(
            lines[0].package_spec().unwrap().arch_list(),
            vec![] as Vec<String>
        );
    }

    #[test]
    fn test_parse_arch_list_roundtrip() {
        // Arch list must survive a round-trip unchanged.
        for input in [
            "foo [amd64]: some-tag\n",
            "foo [amd64 arm64]: some-tag\n",
            "foo [!amd64]: some-tag\n",
            "foo [amd64] binary: some-tag\n",
            "[linux-any]: some-tag\n",
        ] {
            let parsed = LintianOverrides::parse(input).tree();
            assert_eq!(parsed.text(), input, "round-trip differs for {:?}", input);
        }
    }

    #[test]
    fn test_package_name_at_offset_on_name() {
        let text = "libcurl4: hardening-no-pie\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.tree();
        let offset = rowan::TextSize::from(3u32);
        assert_eq!(
            overrides.package_name_at_offset(offset),
            Some("libcurl4".to_string())
        );
    }

    #[test]
    fn test_package_name_at_offset_on_tag_returns_none() {
        let text = "libcurl4: hardening-no-pie\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.tree();
        let offset = rowan::TextSize::from(14u32);
        assert_eq!(overrides.package_name_at_offset(offset), None);
    }

    #[test]
    fn test_package_name_at_offset_type_keyword_returns_none() {
        let text = "source: hardening-no-pie\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.tree();
        let offset = rowan::TextSize::from(3u32);
        assert_eq!(overrides.package_name_at_offset(offset), None);
    }

    #[test]
    fn test_spec_without_tag_is_error() {
        let text = "libcurl4:\n";
        let parsed = LintianOverrides::parse(text);
        assert_eq!(
            parsed.errors_with_offsets().collect::<Vec<_>>(),
            vec![("missing lintian tag", rowan::TextSize::from(0))]
        );
        // The tree is still produced and round-trips exactly.
        assert_eq!(parsed.tree().text(), text);
    }

    #[test]
    fn test_unterminated_arch_list_is_error() {
        let text = "foo [amd64: tag\n";
        let parsed = LintianOverrides::parse(text);
        assert_eq!(
            parsed.errors_with_offsets().collect::<Vec<_>>(),
            vec![("unterminated architecture list", rowan::TextSize::from(0))]
        );
        assert_eq!(parsed.tree().text(), text);
    }

    #[test]
    fn test_partial_parse_reports_offset_of_bad_line() {
        // The first and third lines are well-formed; only the second is an
        // error, reported at its byte offset, and the tree round-trips whole.
        let text = "good: tag\nlibcurl4:\nalso-good: tag2\n";
        let parsed = LintianOverrides::parse(text);
        assert_eq!(
            parsed.errors_with_offsets().collect::<Vec<_>>(),
            vec![("missing lintian tag", rowan::TextSize::from(10))]
        );
        assert_eq!(parsed.tree().text(), text);

        let lines: Vec<_> = parsed.tree().lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].package().unwrap(), "good");
        assert_eq!(lines[0].tag().unwrap().text(), "tag");
        assert_eq!(lines[2].package().unwrap(), "also-good");
        assert_eq!(lines[2].tag().unwrap().text(), "tag2");
    }

    #[test]
    fn test_error_line_wrapped_in_error_node() {
        let text = "libcurl4:\n";
        let parsed = LintianOverrides::parse(text);
        let has_error_node = parsed
            .syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::ERROR);
        assert!(has_error_node);
    }
    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lintian-overrides-test-{}-{}",
            label,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_try_iter_overrides_missing_debian_dir() {
        // No debian directory at all is not an error: there are simply no
        // overrides.
        let base = unique_temp_dir("missing");
        assert_eq!(try_iter_overrides(&base).unwrap().len(), 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_try_iter_overrides_reads_source_overrides() {
        let base = unique_temp_dir("reads");
        std::fs::create_dir_all(base.join("debian/source")).unwrap();
        std::fs::write(
            base.join("debian/source/lintian-overrides"),
            "foo source: some-tag\n",
        )
        .unwrap();

        let lines = try_iter_overrides(&base).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag().unwrap().text(), "some-tag");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_try_iter_overrides_propagates_read_dir_error() {
        // A debian "directory" that is actually a file must surface the I/O
        // error rather than being silently treated as having no overrides:
        // callers rely on overrides to suppress fixes.
        let base = unique_temp_dir("notadir");
        std::fs::write(base.join("debian"), "not a directory").unwrap();

        let err = try_iter_overrides(&base).unwrap_err();
        assert_ne!(err.kind(), std::io::ErrorKind::NotFound);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
