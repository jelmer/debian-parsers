use crate::lex::lex;
use crate::types::{
    ComponentType, Compression, GitExport, GitMode, Mode, PgpMode, Pretty, SearchMode,
};
use crate::SyntaxKind;
use crate::SyntaxKind::*;
use crate::DEFAULT_VERSION;
use std::io::Read;
use std::marker::PhantomData;
use std::str::FromStr;

#[cfg(test)]
use crate::types::VersionPolicy;

/// Get the linebased option key name for a WatchOption variant
pub(crate) fn watch_option_to_key(option: &crate::types::WatchOption) -> &'static str {
    use crate::types::WatchOption;

    match option {
        WatchOption::Component(_) => "component",
        WatchOption::Compression(_) => "compression",
        WatchOption::UserAgent(_) => "user-agent",
        WatchOption::Pagemangle(_) => "pagemangle",
        WatchOption::Uversionmangle(_) => "uversionmangle",
        WatchOption::Dversionmangle(_) => "dversionmangle",
        WatchOption::Dirversionmangle(_) => "dirversionmangle",
        WatchOption::Oversionmangle(_) => "oversionmangle",
        WatchOption::Downloadurlmangle(_) => "downloadurlmangle",
        WatchOption::Pgpsigurlmangle(_) => "pgpsigurlmangle",
        WatchOption::Filenamemangle(_) => "filenamemangle",
        WatchOption::VersionPolicy(_) => "version-policy",
        WatchOption::Searchmode(_) => "searchmode",
        WatchOption::Mode(_) => "mode",
        WatchOption::Pgpmode(_) => "pgpmode",
        WatchOption::Gitexport(_) => "gitexport",
        WatchOption::Gitmode(_) => "gitmode",
        WatchOption::Pretty(_) => "pretty",
        WatchOption::Ctype(_) => "ctype",
        WatchOption::Repacksuffix(_) => "repacksuffix",
        WatchOption::Unzipopt(_) => "unzipopt",
        WatchOption::Script(_) => "script",
        WatchOption::Decompress => "decompress",
        WatchOption::Bare => "bare",
        WatchOption::Repack => "repack",
    }
}

/// Get the string value for a WatchOption variant
pub(crate) fn watch_option_to_value(option: &crate::types::WatchOption) -> String {
    use crate::types::WatchOption;

    match option {
        WatchOption::Component(v) => v.clone(),
        WatchOption::Compression(v) => v.to_string(),
        WatchOption::UserAgent(v) => v.clone(),
        WatchOption::Pagemangle(v) => v.clone(),
        WatchOption::Uversionmangle(v) => v.clone(),
        WatchOption::Dversionmangle(v) => v.clone(),
        WatchOption::Dirversionmangle(v) => v.clone(),
        WatchOption::Oversionmangle(v) => v.clone(),
        WatchOption::Downloadurlmangle(v) => v.clone(),
        WatchOption::Pgpsigurlmangle(v) => v.clone(),
        WatchOption::Filenamemangle(v) => v.clone(),
        WatchOption::VersionPolicy(v) => v.to_string(),
        WatchOption::Searchmode(v) => v.to_string(),
        WatchOption::Mode(v) => v.to_string(),
        WatchOption::Pgpmode(v) => v.to_string(),
        WatchOption::Gitexport(v) => v.to_string(),
        WatchOption::Gitmode(v) => v.to_string(),
        WatchOption::Pretty(v) => v.to_string(),
        WatchOption::Ctype(v) => v.to_string(),
        WatchOption::Repacksuffix(v) => v.clone(),
        WatchOption::Unzipopt(v) => v.clone(),
        WatchOption::Script(v) => v.clone(),
        WatchOption::Decompress => String::new(),
        WatchOption::Bare => String::new(),
        WatchOption::Repack => String::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Error type for parsing line-based watch files
pub struct ParseError(pub Vec<String>);

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
use rowan::GreenNode;

/// You can construct GreenNodes by hand, but a builder
/// is helpful for top-down parsers: it maintains a stack
/// of currently in-progress nodes
use rowan::GreenNodeBuilder;

/// Thread-safe parse result that can be stored in incremental computation systems like Salsa.
/// The type parameter T represents the root AST node type (e.g., WatchFile).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse<T> {
    /// The immutable green tree that can be shared across threads
    green: GreenNode,
    /// Parse errors encountered during parsing
    errors: Vec<String>,
    /// Phantom type to associate this parse result with a specific AST type
    _ty: PhantomData<T>,
}

impl<T> Parse<T> {
    /// Create a new parse result
    pub(crate) fn new(green: GreenNode, errors: Vec<String>) -> Self {
        Parse {
            green,
            errors,
            _ty: PhantomData,
        }
    }

    /// Get the green node
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// Get the parse errors
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Check if there were any parse errors
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

impl Parse<WatchFile> {
    /// Get the root WatchFile node
    pub fn tree(&self) -> WatchFile {
        WatchFile::cast(SyntaxNode::new_root_mut(self.green.clone()))
            .expect("root node should be a WatchFile")
    }
}

// Implement Send + Sync since GreenNode is thread-safe
// This allows Parse to be stored in Salsa databases
unsafe impl<T> Send for Parse<T> {}
unsafe impl<T> Sync for Parse<T> {}

// The internal parse result used during parsing
struct InternalParse {
    green_node: GreenNode,
    errors: Vec<String>,
}

/// Returns true if a token kind can be part of a whitespace-delimited entry
/// field (URL, matching pattern, version policy, or script).
///
/// Values may contain `=` (e.g. URLs with query strings like `?per_page=100`)
/// or `,` and quotes (regex patterns), so we accept any non-structural token.
fn is_field_token(kind: Option<SyntaxKind>) -> bool {
    matches!(kind, Some(KEY | VALUE | EQUALS | COMMA | QUOTE))
}

fn parse(text: &str) -> InternalParse {
    struct Parser {
        /// input tokens, including whitespace,
        /// in *reverse* order.
        tokens: Vec<(SyntaxKind, String)>,
        /// the in-progress tree.
        builder: GreenNodeBuilder<'static>,
        /// the list of syntax errors we've accumulated
        /// so far.
        errors: Vec<String>,
    }

    impl Parser {
        fn parse_version(&mut self) -> Option<u32> {
            let mut version = None;
            if self.tokens.last() == Some(&(KEY, "version".to_string())) {
                self.builder.start_node(VERSION.into());
                self.bump();
                self.skip_ws();
                if self.current() != Some(EQUALS) {
                    self.builder.start_node(ERROR.into());
                    self.errors.push("expected `=`".to_string());
                    self.bump();
                    self.builder.finish_node();
                } else {
                    self.bump();
                }
                if self.current() != Some(VALUE) {
                    self.builder.start_node(ERROR.into());
                    self.errors
                        .push(format!("expected value, got {:?}", self.current()));
                    self.bump();
                    self.builder.finish_node();
                } else if let Some((_, value)) = self.tokens.last() {
                    let version_str = value;
                    match version_str.parse() {
                        Ok(v) => {
                            version = Some(v);
                            self.bump();
                        }
                        Err(_) => {
                            self.builder.start_node(ERROR.into());
                            self.errors
                                .push(format!("invalid version: {}", version_str));
                            self.bump();
                            self.builder.finish_node();
                        }
                    }
                } else {
                    self.builder.start_node(ERROR.into());
                    self.errors.push("expected version value".to_string());
                    self.builder.finish_node();
                }
                if self.current() != Some(NEWLINE) {
                    self.builder.start_node(ERROR.into());
                    self.errors.push("expected newline".to_string());
                    self.bump();
                    self.builder.finish_node();
                } else {
                    self.bump();
                }
                self.builder.finish_node();
            }
            version
        }

        fn parse_watch_entry(&mut self) -> bool {
            // Skip whitespace, comments, and blank lines between entries
            loop {
                self.skip_ws();
                if self.current() == Some(NEWLINE) {
                    self.bump();
                } else {
                    break;
                }
            }
            if self.current().is_none() {
                return false;
            }
            self.builder.start_node(ENTRY.into());
            self.parse_options_list();
            for i in 0..4 {
                if self.current() == Some(NEWLINE) || self.current().is_none() {
                    break;
                }
                if self.current() == Some(CONTINUATION) {
                    self.bump();
                    self.skip_ws();
                    continue;
                }
                // A field has to start with a KEY or VALUE token; punctuation
                // like `=` or `,` on its own is not a valid URL/pattern.
                if !matches!(self.current(), Some(KEY | VALUE)) {
                    self.builder.start_node(ERROR.into());
                    self.errors.push(format!(
                        "expected value, got {:?} (i={})",
                        self.current(),
                        i
                    ));
                    if self.current().is_some() {
                        self.bump();
                    }
                    self.builder.finish_node();
                } else {
                    // Wrap each field in its appropriate node.
                    // Each field gobbles all consecutive non-whitespace tokens, so
                    // values like URLs containing '=' (e.g. "?per_page=100") or regex
                    // patterns containing '=' or ',' are kept intact.
                    let kind = match i {
                        0 => URL,
                        1 => MATCHING_PATTERN,
                        2 => VERSION_POLICY,
                        3 => SCRIPT,
                        _ => unreachable!(),
                    };
                    self.builder.start_node(kind.into());
                    while is_field_token(self.current()) {
                        self.bump();
                    }
                    self.builder.finish_node();
                }
                self.skip_ws();
            }
            if self.current() != Some(NEWLINE) && self.current().is_some() {
                self.builder.start_node(ERROR.into());
                self.errors
                    .push(format!("expected newline, not {:?}", self.current()));
                if self.current().is_some() {
                    self.bump();
                }
                self.builder.finish_node();
            } else if self.current().is_some() {
                // Consume the newline if present (but EOF is also okay)
                self.bump();
            }
            self.builder.finish_node();
            true
        }

        /// Parse a single option `key[=value]` inside an `opts=...` list.
        ///
        /// `quoted` controls the trailing-token rules: in unquoted mode the
        /// value stops at the first whitespace, while inside quotes a single
        /// space before the `,` separator is tolerated.
        fn parse_option(&mut self, quoted: bool) -> bool {
            if self.current().is_none() {
                return false;
            }
            while self.current() == Some(CONTINUATION) {
                self.bump();
            }
            if !quoted && self.current() == Some(WHITESPACE) {
                return false;
            }
            if quoted && self.current() == Some(QUOTE) {
                return false;
            }
            // In unquoted mode, anything that doesn't start a `key[=value]`
            // belongs to the next field (URL etc.) — don't consume it as a
            // bogus option. This keeps trailing-comma + line-continuation
            // patterns like `opts=k=v,\\\nhttps://...` parseable.
            if !quoted && self.current() != Some(KEY) {
                return false;
            }
            self.builder.start_node(OPTION.into());
            if self.current() != Some(KEY) {
                self.builder.start_node(ERROR.into());
                self.errors.push("expected key".to_string());
                self.bump();
                self.builder.finish_node();
            } else {
                self.bump();
            }
            if self.current() == Some(EQUALS) {
                self.bump();
                // The option value may itself be made up of several lexer
                // tokens — for example `s/.*ref=//` lexes as VALUE EQUALS VALUE
                // because `=` is an opts separator. Gobble until the next
                // option boundary so the value is preserved verbatim.
                let mut consumed_value = false;
                loop {
                    match self.current() {
                        Some(KEY) | Some(VALUE) => {
                            self.bump();
                            consumed_value = true;
                        }
                        Some(EQUALS) if consumed_value => self.bump(),
                        Some(WHITESPACE) if quoted => {
                            // Inside quotes, a space between value and the
                            // next separator (e.g. `"key=v , key2=v"`) is
                            // tolerated; the surrounding loop handles the
                            // following comma or closing quote.
                            break;
                        }
                        _ => break,
                    }
                }
                if !consumed_value {
                    self.builder.start_node(ERROR.into());
                    self.errors
                        .push(format!("expected value, got {:?}", self.current()));
                    if self.current().is_some() {
                        self.bump();
                    }
                    self.builder.finish_node();
                }
            } else if self.current() == Some(COMMA) {
            } else {
                self.builder.start_node(ERROR.into());
                self.errors.push("expected `=`".to_string());
                if self.current().is_some() {
                    self.bump();
                }
                self.builder.finish_node();
            }
            self.builder.finish_node();
            true
        }

        fn parse_options_list(&mut self) {
            self.skip_ws();
            if self.tokens.last() == Some(&(KEY, "opts".to_string()))
                || self.tokens.last() == Some(&(KEY, "options".to_string()))
            {
                self.builder.start_node(OPTS_LIST.into());
                self.bump();
                self.skip_ws();
                if self.current() != Some(EQUALS) {
                    self.builder.start_node(ERROR.into());
                    self.errors.push("expected `=`".to_string());
                    if self.current().is_some() {
                        self.bump();
                    }
                    self.builder.finish_node();
                } else {
                    self.bump();
                }
                let quoted = if self.current() == Some(QUOTE) {
                    self.bump();
                    true
                } else {
                    false
                };
                loop {
                    if quoted {
                        // Inside quotes, line continuations and surrounding
                        // whitespace around commas are common; consume them
                        // before checking for the closing quote so a trailing
                        // `,\` followed by `"` doesn't get parsed as another
                        // (empty) option.
                        self.skip_ws();
                        if self.current() == Some(QUOTE) {
                            self.bump();
                            break;
                        }
                    }
                    if !self.parse_option(quoted) {
                        break;
                    }
                    if quoted {
                        // Allow whitespace/continuation between value and the
                        // next comma in quoted opts, e.g. `"a=1 , b=2"`.
                        self.skip_ws();
                    }
                    if self.current() == Some(COMMA) {
                        self.builder.start_node(OPTION_SEPARATOR.into());
                        self.bump();
                        self.builder.finish_node();
                    } else if !quoted {
                        break;
                    }
                }
                self.builder.finish_node();
                self.skip_ws();
            }
        }

        fn parse(mut self) -> InternalParse {
            // Make sure that the root node covers all source
            self.builder.start_node(ROOT.into());
            // Skip any leading comments/whitespace/newlines before version
            while self.current() == Some(WHITESPACE)
                || self.current() == Some(CONTINUATION)
                || self.current() == Some(COMMENT)
                || self.current() == Some(NEWLINE)
            {
                self.bump();
            }
            if let Some(_v) = self.parse_version() {
                // Version is stored in the syntax tree, no need to track separately
            }
            // TODO: use version to influence parsing
            loop {
                if !self.parse_watch_entry() {
                    break;
                }
            }
            // Don't forget to eat *trailing* whitespace
            self.skip_ws();
            // Consume any remaining tokens that were not parsed, recording an error.
            // This ensures the CST always covers the full input.
            if self.current().is_some() {
                self.builder.start_node(ERROR.into());
                self.errors
                    .push("unexpected tokens after last entry".to_string());
                while self.current().is_some() {
                    self.bump();
                }
                self.builder.finish_node();
            }
            // Close the root node.
            self.builder.finish_node();

            // Turn the builder into a GreenNode
            InternalParse {
                green_node: self.builder.finish(),
                errors: self.errors,
            }
        }
        /// Advance one token, adding it to the current branch of the tree builder.
        fn bump(&mut self) {
            if let Some((kind, text)) = self.tokens.pop() {
                self.builder.token(kind.into(), text.as_str());
            }
        }
        /// Peek at the first unprocessed token
        fn current(&self) -> Option<SyntaxKind> {
            self.tokens.last().map(|(kind, _)| *kind)
        }
        fn skip_ws(&mut self) {
            while self.current() == Some(WHITESPACE)
                || self.current() == Some(CONTINUATION)
                || self.current() == Some(COMMENT)
            {
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
    }
    .parse()
}

/// To work with the parse results we need a view into the
/// green tree - the Syntax tree.
/// It is also immutable, like a GreenNode,
/// but it contains parent pointers, offsets, and
/// has identity semantics.
type SyntaxNode = rowan::SyntaxNode<Lang>;
#[allow(unused)]
type SyntaxToken = rowan::SyntaxToken<Lang>;
#[allow(unused)]
type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

impl InternalParse {
    fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root_mut(self.green_node.clone())
    }

    fn root(&self) -> WatchFile {
        WatchFile::cast(self.syntax()).expect("root node should be a WatchFile")
    }
}

/// Calculate line and column (both 0-indexed) for the given offset in the tree.
/// Column is measured in bytes from the start of the line.
fn line_col_at_offset(node: &SyntaxNode, offset: rowan::TextSize) -> (usize, usize) {
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
        /// A node in the syntax tree for $ast
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

            /// Byte range covered by this node in the source buffer.
            ///
            /// Useful for editors that need to map a logical concept
            /// (an option, a URL, a matching pattern) back to the
            /// exact span it occupies — e.g. to filter LSP diagnostics
            /// by which part of a watch entry was edited.
            pub fn text_range(&self) -> rowan::TextRange {
                self.0.text_range()
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
                write!(f, "{}", self.0.text())
            }
        }
    };
}

ast_node!(WatchFile, ROOT);
ast_node!(Version, VERSION);
ast_node!(Entry, ENTRY);
ast_node!(_Option, OPTION);
ast_node!(Url, URL);
ast_node!(MatchingPattern, MATCHING_PATTERN);
ast_node!(VersionPolicyNode, VERSION_POLICY);
ast_node!(ScriptNode, SCRIPT);

// OptionList is manually defined to have a custom Debug impl
#[derive(Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
/// A node in the syntax tree for OptionList
pub struct OptionList(SyntaxNode);

impl OptionList {
    #[allow(unused)]
    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == OPTS_LIST {
            Some(Self(node))
        } else {
            None
        }
    }

    /// Byte range covered by this option list in the source buffer.
    pub fn text_range(&self) -> rowan::TextRange {
        self.0.text_range()
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

impl std::fmt::Display for OptionList {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0.text())
    }
}

impl WatchFile {
    /// Access the underlying syntax node
    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    /// Create a new watch file with specified version
    pub fn new(version: Option<u32>) -> WatchFile {
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(ROOT.into());
        if let Some(version) = version {
            builder.start_node(VERSION.into());
            builder.token(KEY.into(), "version");
            builder.token(EQUALS.into(), "=");
            builder.token(VALUE.into(), version.to_string().as_str());
            builder.token(NEWLINE.into(), "\n");
            builder.finish_node();
        }
        builder.finish_node();
        WatchFile(SyntaxNode::new_root_mut(builder.finish()))
    }

    /// Returns the version AST node of the watch file.
    pub fn version_node(&self) -> Option<Version> {
        self.0.children().find_map(Version::cast)
    }

    /// Returns the version of the watch file.
    pub fn version(&self) -> u32 {
        self.version_node()
            .map(|it| it.version())
            .unwrap_or(DEFAULT_VERSION)
    }

    /// Returns an iterator over all entries in the watch file.
    pub fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        self.0.children().filter_map(Entry::cast)
    }

    /// Set the version of the watch file.
    pub fn set_version(&mut self, new_version: u32) {
        // Build the new version node
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(VERSION.into());
        builder.token(KEY.into(), "version");
        builder.token(EQUALS.into(), "=");
        builder.token(VALUE.into(), new_version.to_string().as_str());
        builder.token(NEWLINE.into(), "\n");
        builder.finish_node();
        let new_version_green = builder.finish();

        // Create a syntax node (splice_children will detach and reattach it)
        let new_version_node = SyntaxNode::new_root_mut(new_version_green);

        // Find existing version node if any
        let version_pos = self.0.children().position(|child| child.kind() == VERSION);

        if let Some(pos) = version_pos {
            // Replace existing version node
            self.0
                .splice_children(pos..pos + 1, vec![new_version_node.into()]);
        } else {
            // Insert version node at the beginning
            self.0.splice_children(0..0, vec![new_version_node.into()]);
        }
    }

    /// Discover releases for all entries in the watch file (async version)
    ///
    /// Fetches URLs and searches for version matches for all entries.
    /// Requires the 'discover' feature.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use debian_watch::WatchFile;
    /// # async fn example() {
    /// let wf: WatchFile = r#"version=4
    /// https://example.com/releases/ .*/v?(\d+\.\d+)\.tar\.gz
    /// "#.parse().unwrap();
    /// let all_releases = wf.uscan(|| "mypackage".to_string()).await.unwrap();
    /// for (entry_idx, releases) in all_releases.iter().enumerate() {
    ///     println!("Entry {}: {} releases found", entry_idx, releases.len());
    /// }
    /// # }
    /// ```
    #[cfg(feature = "discover")]
    pub async fn uscan(
        &self,
        package: impl Fn() -> String + Send + Sync,
    ) -> Result<Vec<Vec<crate::Release>>, Box<dyn std::error::Error>> {
        let mut all_releases = Vec::new();

        for entry in self.entries() {
            let parsed_entry = crate::parse::ParsedEntry::LineBased(entry);
            let releases = parsed_entry.discover(|| package()).await?;
            all_releases.push(releases);
        }

        Ok(all_releases)
    }

    /// Discover releases for all entries in the watch file (blocking version)
    ///
    /// Fetches URLs and searches for version matches for all entries.
    /// Requires both 'discover' and 'blocking' features.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use debian_watch::WatchFile;
    /// let wf: WatchFile = r#"version=4
    /// https://example.com/releases/ .*/v?(\d+\.\d+)\.tar\.gz
    /// "#.parse().unwrap();
    /// let all_releases = wf.uscan_blocking(|| "mypackage".to_string()).unwrap();
    /// for (entry_idx, releases) in all_releases.iter().enumerate() {
    ///     println!("Entry {}: {} releases found", entry_idx, releases.len());
    /// }
    /// ```
    #[cfg(all(feature = "discover", feature = "blocking"))]
    pub fn uscan_blocking(
        &self,
        package: impl Fn() -> String,
    ) -> Result<Vec<Vec<crate::Release>>, Box<dyn std::error::Error>> {
        let mut all_releases = Vec::new();

        for entry in self.entries() {
            let parsed_entry = crate::parse::ParsedEntry::LineBased(entry);
            let releases = parsed_entry.discover_blocking(|| package())?;
            all_releases.push(releases);
        }

        Ok(all_releases)
    }

    /// Add an entry to the watch file.
    ///
    /// Appends a new entry to the end of the watch file.
    ///
    /// # Examples
    ///
    /// ```
    /// use debian_watch::linebased::{WatchFile, EntryBuilder};
    ///
    /// let mut wf = WatchFile::new(Some(4));
    ///
    /// // Add an entry using EntryBuilder
    /// let entry = EntryBuilder::new("https://github.com/example/tags")
    ///     .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
    ///     .build();
    /// wf.add_entry(entry);
    ///
    /// // Or use the builder pattern directly
    /// wf.add_entry(
    ///     EntryBuilder::new("https://example.com/releases")
    ///         .matching_pattern(".*/(\\d+\\.\\d+)\\.tar\\.gz")
    ///         .opt("compression", "xz")
    ///         .version_policy("debian")
    ///         .build()
    /// );
    /// ```
    pub fn add_entry(&mut self, entry: Entry) -> Entry {
        // Find the position to insert (after the last entry or after version)
        let insert_pos = self.0.children_with_tokens().count();

        // Detach the entry node from its current parent and get its green node
        let entry_green = entry.0.green().into_owned();
        let new_entry_node = SyntaxNode::new_root_mut(entry_green);

        // Insert the entry at the end
        self.0
            .splice_children(insert_pos..insert_pos, vec![new_entry_node.into()]);

        // Get the entry we just inserted by indexing directly to the position
        Entry::cast(
            self.0
                .children()
                .nth(insert_pos)
                .expect("Entry was just inserted"),
        )
        .expect("Inserted node should be an Entry")
    }

    /// Read a watch file from a Read object.
    pub fn from_reader<R: std::io::Read>(reader: R) -> Result<WatchFile, ParseError> {
        let mut buf_reader = std::io::BufReader::new(reader);
        let mut content = String::new();
        buf_reader
            .read_to_string(&mut content)
            .map_err(|e| ParseError(vec![e.to_string()]))?;
        content.parse()
    }

    /// Read a watch file from a Read object, allowing syntax errors.
    pub fn from_reader_relaxed<R: std::io::Read>(mut r: R) -> Result<Self, std::io::Error> {
        let mut content = String::new();
        r.read_to_string(&mut content)?;
        let parsed = parse(&content);
        Ok(parsed.root())
    }

    /// Parse a debian watch file from a string, allowing syntax errors.
    pub fn from_str_relaxed(s: &str) -> Self {
        let parsed = parse(s);
        parsed.root()
    }
}

impl FromStr for WatchFile {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed = parse(s);
        if parsed.errors.is_empty() {
            Ok(parsed.root())
        } else {
            Err(ParseError(parsed.errors))
        }
    }
}

/// Parse a watch file and return a thread-safe parse result.
/// This can be stored in incremental computation systems like Salsa.
pub fn parse_watch_file(text: &str) -> Parse<WatchFile> {
    let parsed = parse(text);
    Parse::new(parsed.green_node, parsed.errors)
}

impl Version {
    /// Returns the version of the watch file.
    pub fn version(&self) -> u32 {
        self.0
            .children_with_tokens()
            .find_map(|it| match it {
                SyntaxElement::Token(token) => {
                    if token.kind() == VALUE {
                        token.text().parse().ok()
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .unwrap_or(DEFAULT_VERSION)
    }
}

/// Builder for creating new watchfile entries.
///
/// Provides a fluent API for constructing entries with various components.
///
/// # Examples
///
/// ```
/// use debian_watch::linebased::EntryBuilder;
///
/// // Minimal entry with just URL and pattern
/// let entry = EntryBuilder::new("https://github.com/example/tags")
///     .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
///     .build();
///
/// // Entry with options
/// let entry = EntryBuilder::new("https://github.com/example/tags")
///     .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
///     .opt("compression", "xz")
///     .flag("repack")
///     .version_policy("debian")
///     .script("uupdate")
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct EntryBuilder {
    url: Option<String>,
    matching_pattern: Option<String>,
    version_policy: Option<String>,
    script: Option<String>,
    opts: std::collections::HashMap<String, String>,
}

impl EntryBuilder {
    /// Create a new entry builder with the specified URL.
    pub fn new(url: impl Into<String>) -> Self {
        EntryBuilder {
            url: Some(url.into()),
            matching_pattern: None,
            version_policy: None,
            script: None,
            opts: std::collections::HashMap::new(),
        }
    }

    /// Set the matching pattern for the entry.
    pub fn matching_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.matching_pattern = Some(pattern.into());
        self
    }

    /// Set the version policy for the entry.
    pub fn version_policy(mut self, policy: impl Into<String>) -> Self {
        self.version_policy = Some(policy.into());
        self
    }

    /// Set the script for the entry.
    pub fn script(mut self, script: impl Into<String>) -> Self {
        self.script = Some(script.into());
        self
    }

    /// Add an option to the entry.
    pub fn opt(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.opts.insert(key.into(), value.into());
        self
    }

    /// Add a boolean flag option to the entry.
    ///
    /// Boolean options like "repack", "bare", "decompress" don't have values.
    pub fn flag(mut self, key: impl Into<String>) -> Self {
        self.opts.insert(key.into(), String::new());
        self
    }

    /// Build the entry.
    ///
    /// # Panics
    ///
    /// Panics if no URL was provided.
    pub fn build(self) -> Entry {
        let url = self.url.expect("URL is required for entry");

        let mut builder = GreenNodeBuilder::new();

        builder.start_node(ENTRY.into());

        // Add options list if provided
        if !self.opts.is_empty() {
            builder.start_node(OPTS_LIST.into());
            builder.token(KEY.into(), "opts");
            builder.token(EQUALS.into(), "=");

            let mut first = true;
            for (key, value) in self.opts.iter() {
                if !first {
                    builder.token(COMMA.into(), ",");
                }
                first = false;

                builder.start_node(OPTION.into());
                builder.token(KEY.into(), key);
                if !value.is_empty() {
                    builder.token(EQUALS.into(), "=");
                    builder.token(VALUE.into(), value);
                }
                builder.finish_node();
            }

            builder.finish_node();
            builder.token(WHITESPACE.into(), " ");
        }

        // Add URL (required)
        builder.start_node(URL.into());
        builder.token(VALUE.into(), &url);
        builder.finish_node();

        // Add matching pattern if provided
        if let Some(pattern) = self.matching_pattern {
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(MATCHING_PATTERN.into());
            builder.token(VALUE.into(), &pattern);
            builder.finish_node();
        }

        // Add version policy if provided
        if let Some(policy) = self.version_policy {
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(VERSION_POLICY.into());
            builder.token(VALUE.into(), &policy);
            builder.finish_node();
        }

        // Add script if provided
        if let Some(script_val) = self.script {
            builder.token(WHITESPACE.into(), " ");
            builder.start_node(SCRIPT.into());
            builder.token(VALUE.into(), &script_val);
            builder.finish_node();
        }

        builder.token(NEWLINE.into(), "\n");
        builder.finish_node();

        Entry(SyntaxNode::new_root_mut(builder.finish()))
    }
}

impl Entry {
    /// Access the underlying syntax node.
    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    /// Create a new entry builder.
    ///
    /// This is a convenience method that returns an `EntryBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use debian_watch::linebased::Entry;
    ///
    /// let entry = Entry::builder("https://github.com/example/tags")
    ///     .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
    ///     .build();
    /// ```
    pub fn builder(url: impl Into<String>) -> EntryBuilder {
        EntryBuilder::new(url)
    }

    /// List of options
    pub fn option_list(&self) -> Option<OptionList> {
        self.0.children().find_map(OptionList::cast)
    }

    /// Get the value of an option
    pub fn get_option(&self, key: &str) -> Option<String> {
        self.option_list().and_then(|ol| ol.get_option(key))
    }

    /// Check if an option is set
    pub fn has_option(&self, key: &str) -> bool {
        self.option_list().is_some_and(|ol| ol.has_option(key))
    }

    /// The name of the secondary source tarball
    pub fn component(&self) -> Option<String> {
        self.get_option("component")
    }

    /// Component type
    pub fn ctype(&self) -> Result<Option<ComponentType>, ()> {
        self.try_ctype().map_err(|_| ())
    }

    /// Component type with detailed error information
    pub fn try_ctype(&self) -> Result<Option<ComponentType>, crate::types::ParseError> {
        self.get_option("ctype").map(|s| s.parse()).transpose()
    }

    /// Compression method
    pub fn compression(&self) -> Result<Option<Compression>, ()> {
        self.try_compression().map_err(|_| ())
    }

    /// Compression method with detailed error information
    pub fn try_compression(&self) -> Result<Option<Compression>, crate::types::ParseError> {
        self.get_option("compression")
            .map(|s| s.parse())
            .transpose()
    }

    /// Repack the tarball
    pub fn repack(&self) -> bool {
        self.has_option("repack")
    }

    /// Repack suffix
    pub fn repacksuffix(&self) -> Option<String> {
        self.get_option("repacksuffix")
    }

    /// Retrieve the mode of the watch file entry.
    pub fn mode(&self) -> Result<Mode, ()> {
        self.try_mode().map_err(|_| ())
    }

    /// Retrieve the mode of the watch file entry with detailed error information.
    pub fn try_mode(&self) -> Result<Mode, crate::types::ParseError> {
        Ok(self
            .get_option("mode")
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or_default())
    }

    /// Return the git pretty mode
    pub fn pretty(&self) -> Result<Pretty, ()> {
        self.try_pretty().map_err(|_| ())
    }

    /// Return the git pretty mode with detailed error information
    pub fn try_pretty(&self) -> Result<Pretty, crate::types::ParseError> {
        Ok(self
            .get_option("pretty")
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or_default())
    }

    /// Set the date string used by the pretty option to an arbitrary format as an optional
    /// opts argument when the matching-pattern is HEAD or heads/branch for git mode.
    pub fn date(&self) -> String {
        self.get_option("date").unwrap_or_else(|| "%Y%m%d".into())
    }

    /// Return the git export mode
    pub fn gitexport(&self) -> Result<GitExport, ()> {
        self.try_gitexport().map_err(|_| ())
    }

    /// Return the git export mode with detailed error information
    pub fn try_gitexport(&self) -> Result<GitExport, crate::types::ParseError> {
        Ok(self
            .get_option("gitexport")
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or_default())
    }

    /// Return the git mode
    pub fn gitmode(&self) -> Result<GitMode, ()> {
        self.try_gitmode().map_err(|_| ())
    }

    /// Return the git mode with detailed error information
    pub fn try_gitmode(&self) -> Result<GitMode, crate::types::ParseError> {
        Ok(self
            .get_option("gitmode")
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or_default())
    }

    /// Return the pgp mode
    pub fn pgpmode(&self) -> Result<PgpMode, ()> {
        self.try_pgpmode().map_err(|_| ())
    }

    /// Return the pgp mode with detailed error information
    pub fn try_pgpmode(&self) -> Result<PgpMode, crate::types::ParseError> {
        Ok(self
            .get_option("pgpmode")
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or_default())
    }

    /// Return the search mode
    pub fn searchmode(&self) -> Result<SearchMode, ()> {
        self.try_searchmode().map_err(|_| ())
    }

    /// Return the search mode with detailed error information
    pub fn try_searchmode(&self) -> Result<SearchMode, crate::types::ParseError> {
        Ok(self
            .get_option("searchmode")
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or_default())
    }

    /// Return the decompression mode
    pub fn decompress(&self) -> bool {
        self.has_option("decompress")
    }

    /// Whether to disable all site specific special case code such as URL director uses and page
    /// content alterations.
    pub fn bare(&self) -> bool {
        self.has_option("bare")
    }

    /// Set the user-agent string used to contact the HTTP(S) server as user-agent-string. (persistent)
    pub fn user_agent(&self) -> Option<String> {
        self.get_option("user-agent")
    }

    /// Use PASV mode for the FTP connection.
    pub fn passive(&self) -> Option<bool> {
        if self.has_option("passive") || self.has_option("pasv") {
            Some(true)
        } else if self.has_option("active") || self.has_option("nopasv") {
            Some(false)
        } else {
            None
        }
    }

    /// Add the extra options to use with the unzip command, such as -a, -aa, and -b, when executed
    /// by mk-origtargz.
    pub fn unzipoptions(&self) -> Option<String> {
        self.get_option("unzipopt")
    }

    /// Normalize the downloaded web page string.
    pub fn dversionmangle(&self) -> Option<String> {
        self.get_option("dversionmangle")
            .or_else(|| self.get_option("versionmangle"))
    }

    /// Normalize the directory path string matching the regex in a set of parentheses of
    /// http://URL as the sortable version index string.  This is used
    /// as the directory path sorting index only.
    pub fn dirversionmangle(&self) -> Option<String> {
        self.get_option("dirversionmangle")
    }

    /// Normalize the downloaded web page string.
    pub fn pagemangle(&self) -> Option<String> {
        self.get_option("pagemangle")
    }

    /// Normalize the candidate upstream version strings extracted from hrefs in the
    /// source of the web page.  This is used as the version sorting index when selecting the
    /// latest upstream version.
    pub fn uversionmangle(&self) -> Option<String> {
        self.get_option("uversionmangle")
            .or_else(|| self.get_option("versionmangle"))
    }

    /// Syntactic shorthand for uversionmangle=rules, dversionmangle=rules
    pub fn versionmangle(&self) -> Option<String> {
        self.get_option("versionmangle")
    }

    /// Convert the selected upstream tarball href string from the percent-encoded hexadecimal
    /// string to the decoded normal URL  string  for  obfuscated
    /// web sites.  Only percent-encoding is available and it is decoded with
    /// s/%([A-Fa-f\d]{2})/chr hex $1/eg.
    pub fn hrefdecode(&self) -> bool {
        self.get_option("hrefdecode").is_some()
    }

    /// Convert the selected upstream tarball href string into the accessible URL for obfuscated
    /// web sites.  This is run after hrefdecode.
    pub fn downloadurlmangle(&self) -> Option<String> {
        self.get_option("downloadurlmangle")
    }

    /// Generate the upstream tarball filename from the selected href string if matching-pattern
    /// can extract the latest upstream version <uversion> from the  selected  href  string.
    /// Otherwise, generate the upstream tarball filename from its full URL string and set the
    /// missing <uversion> from the generated upstream tarball filename.
    ///
    /// Without this option, the default upstream tarball filename is generated by taking the last
    /// component of the URL and  removing everything  after any '?' or '#'.
    pub fn filenamemangle(&self) -> Option<String> {
        self.get_option("filenamemangle")
    }

    /// Generate the candidate upstream signature file URL string from the upstream tarball URL.
    pub fn pgpsigurlmangle(&self) -> Option<String> {
        self.get_option("pgpsigurlmangle")
    }

    /// Generate the version string <oversion> of the source tarball <spkg>_<oversion>.orig.tar.gz
    /// from <uversion>.  This should be used to add a suffix such as +dfsg to a MUT package.
    pub fn oversionmangle(&self) -> Option<String> {
        self.get_option("oversionmangle")
    }

    /// Apply uversionmangle to a version string
    ///
    /// # Examples
    ///
    /// ```
    /// # use debian_watch::linebased::WatchFile;
    /// let wf: WatchFile = r#"version=4
    /// opts=uversionmangle=s/\+ds// https://example.com/ .*
    /// "#.parse().unwrap();
    /// let entry = wf.entries().next().unwrap();
    /// assert_eq!(entry.apply_uversionmangle("1.0+ds").unwrap(), "1.0");
    /// ```
    pub fn apply_uversionmangle(
        &self,
        version: &str,
    ) -> Result<String, crate::mangle::MangleError> {
        if let Some(vm) = self.uversionmangle() {
            crate::mangle::apply_mangle(&vm, version)
        } else {
            Ok(version.to_string())
        }
    }

    /// Apply dversionmangle to a version string
    ///
    /// # Examples
    ///
    /// ```
    /// # use debian_watch::linebased::WatchFile;
    /// let wf: WatchFile = r#"version=4
    /// opts=dversionmangle=s/\+dfsg$// https://example.com/ .*
    /// "#.parse().unwrap();
    /// let entry = wf.entries().next().unwrap();
    /// assert_eq!(entry.apply_dversionmangle("1.0+dfsg").unwrap(), "1.0");
    /// ```
    pub fn apply_dversionmangle(
        &self,
        version: &str,
    ) -> Result<String, crate::mangle::MangleError> {
        if let Some(vm) = self.dversionmangle() {
            crate::mangle::apply_mangle(&vm, version)
        } else {
            Ok(version.to_string())
        }
    }

    /// Apply oversionmangle to a version string
    ///
    /// # Examples
    ///
    /// ```
    /// # use debian_watch::linebased::WatchFile;
    /// let wf: WatchFile = r#"version=4
    /// opts=oversionmangle=s/$/-1/ https://example.com/ .*
    /// "#.parse().unwrap();
    /// let entry = wf.entries().next().unwrap();
    /// assert_eq!(entry.apply_oversionmangle("1.0").unwrap(), "1.0-1");
    /// ```
    pub fn apply_oversionmangle(
        &self,
        version: &str,
    ) -> Result<String, crate::mangle::MangleError> {
        if let Some(vm) = self.oversionmangle() {
            crate::mangle::apply_mangle(&vm, version)
        } else {
            Ok(version.to_string())
        }
    }

    /// Apply dirversionmangle to a directory path string
    ///
    /// # Examples
    ///
    /// ```
    /// # use debian_watch::linebased::WatchFile;
    /// let wf: WatchFile = r#"version=4
    /// opts=dirversionmangle=s/v(\d)/$1/ https://example.com/ .*
    /// "#.parse().unwrap();
    /// let entry = wf.entries().next().unwrap();
    /// assert_eq!(entry.apply_dirversionmangle("v1.0").unwrap(), "1.0");
    /// ```
    pub fn apply_dirversionmangle(
        &self,
        version: &str,
    ) -> Result<String, crate::mangle::MangleError> {
        if let Some(vm) = self.dirversionmangle() {
            crate::mangle::apply_mangle(&vm, version)
        } else {
            Ok(version.to_string())
        }
    }

    /// Apply filenamemangle to a URL or filename string
    ///
    /// # Examples
    ///
    /// ```
    /// # use debian_watch::linebased::WatchFile;
    /// let wf: WatchFile = r#"version=4
    /// opts=filenamemangle=s/.+\/v?(\d\S+)\.tar\.gz/mypackage-$1.tar.gz/ https://example.com/ .*
    /// "#.parse().unwrap();
    /// let entry = wf.entries().next().unwrap();
    /// assert_eq!(
    ///     entry.apply_filenamemangle("https://example.com/v1.0.tar.gz").unwrap(),
    ///     "mypackage-1.0.tar.gz"
    /// );
    /// ```
    pub fn apply_filenamemangle(&self, url: &str) -> Result<String, crate::mangle::MangleError> {
        if let Some(vm) = self.filenamemangle() {
            crate::mangle::apply_mangle(&vm, url)
        } else {
            Ok(url.to_string())
        }
    }

    /// Apply pagemangle to page content bytes
    ///
    /// # Examples
    ///
    /// ```
    /// # use debian_watch::linebased::WatchFile;
    /// let wf: WatchFile = r#"version=4
    /// opts=pagemangle=s/&amp;/&/g https://example.com/ .*
    /// "#.parse().unwrap();
    /// let entry = wf.entries().next().unwrap();
    /// assert_eq!(
    ///     entry.apply_pagemangle(b"foo &amp; bar").unwrap(),
    ///     b"foo & bar"
    /// );
    /// ```
    pub fn apply_pagemangle(&self, page: &[u8]) -> Result<Vec<u8>, crate::mangle::MangleError> {
        if let Some(vm) = self.pagemangle() {
            let page_str = String::from_utf8_lossy(page);
            let mangled = crate::mangle::apply_mangle(&vm, &page_str)?;
            Ok(mangled.into_bytes())
        } else {
            Ok(page.to_vec())
        }
    }

    /// Apply downloadurlmangle to a URL string
    ///
    /// # Examples
    ///
    /// ```
    /// # use debian_watch::linebased::WatchFile;
    /// let wf: WatchFile = r#"version=4
    /// opts=downloadurlmangle=s|/archive/|/download/| https://example.com/ .*
    /// "#.parse().unwrap();
    /// let entry = wf.entries().next().unwrap();
    /// assert_eq!(
    ///     entry.apply_downloadurlmangle("https://example.com/archive/file.tar.gz").unwrap(),
    ///     "https://example.com/download/file.tar.gz"
    /// );
    /// ```
    pub fn apply_downloadurlmangle(&self, url: &str) -> Result<String, crate::mangle::MangleError> {
        if let Some(vm) = self.downloadurlmangle() {
            crate::mangle::apply_mangle(&vm, url)
        } else {
            Ok(url.to_string())
        }
    }

    /// Returns options set
    pub fn opts(&self) -> std::collections::HashMap<String, String> {
        let mut options = std::collections::HashMap::new();

        if let Some(ol) = self.option_list() {
            for opt in ol.options() {
                let key = opt.key();
                let value = opt.value();
                if let (Some(key), Some(value)) = (key, value) {
                    options.insert(key.to_string(), value.to_string());
                }
            }
        }

        options
    }

    fn items(&self) -> impl Iterator<Item = String> + '_ {
        self.0.children_with_tokens().filter_map(|it| match it {
            SyntaxElement::Token(token) => {
                if token.kind() == VALUE || token.kind() == KEY {
                    Some(token.text().to_string())
                } else {
                    None
                }
            }
            SyntaxElement::Node(node) => {
                // Extract values from entry field nodes
                match node.kind() {
                    URL => Url::cast(node).map(|n| n.url()),
                    MATCHING_PATTERN => MatchingPattern::cast(node).map(|n| n.pattern()),
                    VERSION_POLICY => VersionPolicyNode::cast(node).map(|n| n.policy()),
                    SCRIPT => ScriptNode::cast(node).map(|n| n.script()),
                    _ => None,
                }
            }
        })
    }

    /// Returns the URL AST node of the entry.
    pub fn url_node(&self) -> Option<Url> {
        self.0.children().find_map(Url::cast)
    }

    /// Returns the URL of the entry.
    pub fn url(&self) -> String {
        self.url_node()
            .map(|it| it.url())
            .or_else(|| self.items().next())
            .unwrap_or_default()
    }

    /// Returns the matching pattern AST node of the entry.
    pub fn matching_pattern_node(&self) -> Option<MatchingPattern> {
        self.0.children().find_map(MatchingPattern::cast)
    }

    /// Returns the matching pattern of the entry.
    pub fn matching_pattern(&self) -> Option<String> {
        self.matching_pattern_node()
            .map(|it| it.pattern())
            .or_else(|| {
                // Fallback for entries without MATCHING_PATTERN node
                self.items().nth(1)
            })
    }

    /// Returns the version policy AST node of the entry.
    pub fn version_node(&self) -> Option<VersionPolicyNode> {
        self.0.children().find_map(VersionPolicyNode::cast)
    }

    /// Returns the version policy
    pub fn version(&self) -> Result<Option<crate::VersionPolicy>, String> {
        self.version_node()
            .map(|it| it.policy().parse())
            .transpose()
            .map_err(|e: crate::types::ParseError| e.to_string())
            .or_else(|_e| {
                // Fallback for entries without VERSION_POLICY node
                self.items()
                    .nth(2)
                    .map(|it| it.parse())
                    .transpose()
                    .map_err(|e: crate::types::ParseError| e.to_string())
            })
    }

    /// Returns the script AST node of the entry.
    pub fn script_node(&self) -> Option<ScriptNode> {
        self.0.children().find_map(ScriptNode::cast)
    }

    /// Returns the script of the entry.
    pub fn script(&self) -> Option<String> {
        self.script_node().map(|it| it.script()).or_else(|| {
            // Fallback for entries without SCRIPT node
            self.items().nth(3)
        })
    }

    /// Replace all substitutions and return the resulting URL.
    pub fn format_url(
        &self,
        package: impl FnOnce() -> String,
        component: impl FnOnce() -> String,
    ) -> url::Url {
        crate::subst::subst(self.url().as_str(), package, component)
            .parse()
            .unwrap()
    }

    /// Set the URL of the entry.
    pub fn set_url(&mut self, new_url: &str) {
        // Build the new URL node
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(URL.into());
        builder.token(VALUE.into(), new_url);
        builder.finish_node();
        let new_url_green = builder.finish();

        // Create a syntax node (splice_children will detach and reattach it)
        let new_url_node = SyntaxNode::new_root_mut(new_url_green);

        // Find existing URL node position (need to use children_with_tokens for correct indexing)
        let url_pos = self
            .0
            .children_with_tokens()
            .position(|child| matches!(child, SyntaxElement::Node(node) if node.kind() == URL));

        if let Some(pos) = url_pos {
            // Replace existing URL node
            self.0
                .splice_children(pos..pos + 1, vec![new_url_node.into()]);
        }
    }

    /// Set the matching pattern of the entry.
    ///
    /// TODO: This currently only replaces an existing matching pattern.
    /// If the entry doesn't have a matching pattern, this method does nothing.
    /// Future implementation should insert the node at the correct position.
    pub fn set_matching_pattern(&mut self, new_pattern: &str) {
        // Build the new MATCHING_PATTERN node
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(MATCHING_PATTERN.into());
        builder.token(VALUE.into(), new_pattern);
        builder.finish_node();
        let new_pattern_green = builder.finish();

        // Create a syntax node (splice_children will detach and reattach it)
        let new_pattern_node = SyntaxNode::new_root_mut(new_pattern_green);

        // Find existing MATCHING_PATTERN node position
        let pattern_pos = self.0.children_with_tokens().position(
            |child| matches!(child, SyntaxElement::Node(node) if node.kind() == MATCHING_PATTERN),
        );

        if let Some(pos) = pattern_pos {
            // Replace existing MATCHING_PATTERN node
            self.0
                .splice_children(pos..pos + 1, vec![new_pattern_node.into()]);
        }
        // TODO: else insert new node after URL
    }

    /// Set the version policy of the entry.
    ///
    /// TODO: This currently only replaces an existing version policy.
    /// If the entry doesn't have a version policy, this method does nothing.
    /// Future implementation should insert the node at the correct position.
    pub fn set_version_policy(&mut self, new_policy: &str) {
        // Build the new VERSION_POLICY node
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(VERSION_POLICY.into());
        // Version policy can be KEY (e.g., "debian") or VALUE
        builder.token(VALUE.into(), new_policy);
        builder.finish_node();
        let new_policy_green = builder.finish();

        // Create a syntax node (splice_children will detach and reattach it)
        let new_policy_node = SyntaxNode::new_root_mut(new_policy_green);

        // Find existing VERSION_POLICY node position
        let policy_pos = self.0.children_with_tokens().position(
            |child| matches!(child, SyntaxElement::Node(node) if node.kind() == VERSION_POLICY),
        );

        if let Some(pos) = policy_pos {
            // Replace existing VERSION_POLICY node
            self.0
                .splice_children(pos..pos + 1, vec![new_policy_node.into()]);
        }
        // TODO: else insert new node after MATCHING_PATTERN (or URL if no pattern)
    }

    /// Set the script of the entry.
    ///
    /// TODO: This currently only replaces an existing script.
    /// If the entry doesn't have a script, this method does nothing.
    /// Future implementation should insert the node at the correct position.
    pub fn set_script(&mut self, new_script: &str) {
        // Build the new SCRIPT node
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SCRIPT.into());
        // Script can be KEY (e.g., "uupdate") or VALUE
        builder.token(VALUE.into(), new_script);
        builder.finish_node();
        let new_script_green = builder.finish();

        // Create a syntax node (splice_children will detach and reattach it)
        let new_script_node = SyntaxNode::new_root_mut(new_script_green);

        // Find existing SCRIPT node position
        let script_pos = self
            .0
            .children_with_tokens()
            .position(|child| matches!(child, SyntaxElement::Node(node) if node.kind() == SCRIPT));

        if let Some(pos) = script_pos {
            // Replace existing SCRIPT node
            self.0
                .splice_children(pos..pos + 1, vec![new_script_node.into()]);
        }
        // TODO: else insert new node after VERSION_POLICY (or MATCHING_PATTERN/URL if no policy)
    }

    /// Set or update an option value using a WatchOption enum.
    ///
    /// If the option already exists, it will be updated with the new value.
    /// If the option doesn't exist, it will be added to the options list.
    /// If there's no options list, one will be created.
    pub fn set_option(&mut self, option: crate::types::WatchOption) {
        let key = watch_option_to_key(&option);
        let value = watch_option_to_value(&option);
        self.set_opt(key, &value);
    }

    /// Set or update an option value using string key and value (for backward compatibility).
    ///
    /// If the option already exists, it will be updated with the new value.
    /// If the option doesn't exist, it will be added to the options list.
    /// If there's no options list, one will be created.
    pub fn set_opt(&mut self, key: &str, value: &str) {
        // Find the OPTS_LIST position in Entry
        let opts_pos = self.0.children_with_tokens().position(
            |child| matches!(child, SyntaxElement::Node(node) if node.kind() == OPTS_LIST),
        );

        if let Some(_opts_idx) = opts_pos {
            if let Some(mut ol) = self.option_list() {
                // Find if the option already exists
                if let Some(mut opt) = ol.find_option(key) {
                    // Update the existing option's value
                    opt.set_value(value);
                    // Mutations should propagate automatically - no need to replace
                } else {
                    // Add new option
                    ol.add_option(key, value);
                    // Mutations should propagate automatically - no need to replace
                }
            }
        } else {
            // Create a new options list
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(OPTS_LIST.into());
            builder.token(KEY.into(), "opts");
            builder.token(EQUALS.into(), "=");
            builder.start_node(OPTION.into());
            builder.token(KEY.into(), key);
            builder.token(EQUALS.into(), "=");
            builder.token(VALUE.into(), value);
            builder.finish_node();
            builder.finish_node();
            let new_opts_green = builder.finish();
            let new_opts_node = SyntaxNode::new_root_mut(new_opts_green);

            // Find position to insert (before URL if it exists, otherwise at start)
            let url_pos = self
                .0
                .children_with_tokens()
                .position(|child| matches!(child, SyntaxElement::Node(node) if node.kind() == URL));

            if let Some(url_idx) = url_pos {
                // Insert options list and a space before the URL
                // Build a parent node containing both space and whitespace to extract from
                let mut combined_builder = GreenNodeBuilder::new();
                combined_builder.start_node(ROOT.into()); // Temporary parent
                combined_builder.token(WHITESPACE.into(), " ");
                combined_builder.finish_node();
                let temp_green = combined_builder.finish();
                let temp_root = SyntaxNode::new_root_mut(temp_green);
                let space_element = temp_root.children_with_tokens().next().unwrap();

                self.0
                    .splice_children(url_idx..url_idx, vec![new_opts_node.into(), space_element]);
            } else {
                self.0.splice_children(0..0, vec![new_opts_node.into()]);
            }
        }
    }

    /// Delete an option using a WatchOption enum.
    ///
    /// Removes the option from the options list.
    /// If the option doesn't exist, this method does nothing.
    /// If deleting the option results in an empty options list, the entire
    /// opts= declaration is removed.
    pub fn del_opt(&mut self, option: crate::types::WatchOption) {
        let key = watch_option_to_key(&option);
        if let Some(mut ol) = self.option_list() {
            let option_count = ol.0.children().filter(|n| n.kind() == OPTION).count();

            if option_count == 1 && ol.has_option(key) {
                // This is the last option, remove the entire OPTS_LIST from Entry
                let opts_pos = self.0.children().position(|node| node.kind() == OPTS_LIST);

                if let Some(opts_idx) = opts_pos {
                    // Remove the OPTS_LIST
                    self.0.splice_children(opts_idx..opts_idx + 1, vec![]);

                    // Remove any leading whitespace/continuation that was after the OPTS_LIST
                    while self.0.children_with_tokens().next().is_some_and(|e| {
                        matches!(
                            e,
                            SyntaxElement::Token(t) if t.kind() == WHITESPACE || t.kind() == CONTINUATION
                        )
                    }) {
                        self.0.splice_children(0..1, vec![]);
                    }
                }
            } else {
                // Defer to OptionList to remove the option
                ol.remove_option(key);
            }
        }
    }

    /// Delete an option using a string key (for backward compatibility).
    ///
    /// Removes the option with the specified key from the options list.
    /// If the option doesn't exist, this method does nothing.
    /// If deleting the option results in an empty options list, the entire
    /// opts= declaration is removed.
    pub fn del_opt_str(&mut self, key: &str) {
        if let Some(mut ol) = self.option_list() {
            let option_count = ol.0.children().filter(|n| n.kind() == OPTION).count();

            if option_count == 1 && ol.has_option(key) {
                // This is the last option, remove the entire OPTS_LIST from Entry
                let opts_pos = self.0.children().position(|node| node.kind() == OPTS_LIST);

                if let Some(opts_idx) = opts_pos {
                    // Remove the OPTS_LIST
                    self.0.splice_children(opts_idx..opts_idx + 1, vec![]);

                    // Remove any leading whitespace/continuation that was after the OPTS_LIST
                    while self.0.children_with_tokens().next().is_some_and(|e| {
                        matches!(
                            e,
                            SyntaxElement::Token(t) if t.kind() == WHITESPACE || t.kind() == CONTINUATION
                        )
                    }) {
                        self.0.splice_children(0..1, vec![]);
                    }
                }
            } else {
                // Defer to OptionList to remove the option
                ol.remove_option(key);
            }
        }
    }
}

impl std::fmt::Debug for OptionList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OptionList")
            .field("text", &self.0.text().to_string())
            .finish()
    }
}

impl OptionList {
    /// Returns an iterator over all option nodes in the options list.
    pub fn options(&self) -> impl Iterator<Item = _Option> + '_ {
        self.0.children().filter_map(_Option::cast)
    }

    /// Find an option node by key.
    pub fn find_option(&self, key: &str) -> Option<_Option> {
        self.options().find(|opt| opt.key().as_deref() == Some(key))
    }

    /// Check if an option with the given key exists
    pub fn has_option(&self, key: &str) -> bool {
        self.options().any(|it| it.key().as_deref() == Some(key))
    }

    /// Returns an iterator over all options as (key, value) pairs.
    /// This is a convenience method for code that needs key-value tuples (used for conversion to deb822 format).
    #[cfg(feature = "deb822")]
    pub(crate) fn iter_key_values(&self) -> impl Iterator<Item = (String, String)> + '_ {
        self.options().filter_map(|opt| {
            if let (Some(key), Some(value)) = (opt.key(), opt.value()) {
                Some((key, value))
            } else {
                None
            }
        })
    }

    /// Get the value of an option by key
    pub fn get_option(&self, key: &str) -> Option<String> {
        for child in self.options() {
            if child.key().as_deref() == Some(key) {
                return child.value();
            }
        }
        None
    }

    /// Add a new option to the end of the options list.
    fn add_option(&mut self, key: &str, value: &str) {
        let option_count = self.0.children().filter(|n| n.kind() == OPTION).count();

        // Build a structure containing separator (if needed) + option wrapped in a temporary parent
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(ROOT.into()); // Temporary parent

        if option_count > 0 {
            builder.start_node(OPTION_SEPARATOR.into());
            builder.token(COMMA.into(), ",");
            builder.finish_node();
        }

        builder.start_node(OPTION.into());
        builder.token(KEY.into(), key);
        builder.token(EQUALS.into(), "=");
        builder.token(VALUE.into(), value);
        builder.finish_node();

        builder.finish_node(); // Close temporary parent
        let combined_green = builder.finish();

        // Create a temporary root to extract children from
        let temp_root = SyntaxNode::new_root_mut(combined_green);
        let new_children: Vec<_> = temp_root.children_with_tokens().collect();

        let insert_pos = self.0.children_with_tokens().count();
        self.0.splice_children(insert_pos..insert_pos, new_children);
    }

    /// Remove an option by key. Returns true if an option was removed.
    fn remove_option(&mut self, key: &str) -> bool {
        if let Some(mut opt) = self.find_option(key) {
            opt.remove();
            true
        } else {
            false
        }
    }
}

impl _Option {
    /// Returns the key of the option.
    pub fn key(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|it| match it {
            SyntaxElement::Token(token) => {
                if token.kind() == KEY {
                    Some(token.text().to_string())
                } else {
                    None
                }
            }
            _ => None,
        })
    }

    /// Returns the value of the option.
    pub fn value(&self) -> Option<String> {
        self.0
            .children_with_tokens()
            .filter_map(|it| match it {
                SyntaxElement::Token(token) => {
                    if token.kind() == VALUE || token.kind() == KEY {
                        Some(token.text().to_string())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .nth(1)
    }

    /// Set the value of the option.
    pub fn set_value(&mut self, new_value: &str) {
        let key = self.key().expect("Option must have a key");

        // Build a new OPTION node with the updated value
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(OPTION.into());
        builder.token(KEY.into(), &key);
        builder.token(EQUALS.into(), "=");
        builder.token(VALUE.into(), new_value);
        builder.finish_node();
        let new_option_green = builder.finish();
        let new_option_node = SyntaxNode::new_root_mut(new_option_green);

        // Replace this option in the parent OptionList
        if let Some(parent) = self.0.parent() {
            let idx = self.0.index();
            parent.splice_children(idx..idx + 1, vec![new_option_node.into()]);
        }
    }

    /// Remove this option and its associated separator from the parent OptionList.
    pub fn remove(&mut self) {
        // Find adjacent separator to remove before detaching this node
        let next_sep = self
            .0
            .next_sibling()
            .filter(|n| n.kind() == OPTION_SEPARATOR);
        let prev_sep = self
            .0
            .prev_sibling()
            .filter(|n| n.kind() == OPTION_SEPARATOR);

        // Detach separator first if it exists
        if let Some(sep) = next_sep {
            sep.detach();
        } else if let Some(sep) = prev_sep {
            sep.detach();
        }

        // Now detach the option itself
        self.0.detach();
    }
}

/// Concatenate every direct token child of `node` whose kind passes `keep`,
/// preserving the original input order. Used by entry-field accessors so that
/// values made up of several lexer tokens (e.g. a URL split around `=`) are
/// returned as a single string.
fn join_tokens(node: &SyntaxNode, keep: impl Fn(SyntaxKind) -> bool) -> String {
    let mut out = String::new();
    for it in node.children_with_tokens() {
        if let SyntaxElement::Token(token) = it {
            if keep(token.kind()) {
                out.push_str(token.text());
            }
        }
    }
    out
}

impl Url {
    /// Returns the URL string.
    pub fn url(&self) -> String {
        join_tokens(&self.0, |k| {
            matches!(k, KEY | VALUE | EQUALS | COMMA | QUOTE)
        })
    }
}

impl MatchingPattern {
    /// Returns the matching pattern string.
    pub fn pattern(&self) -> String {
        join_tokens(&self.0, |k| {
            matches!(k, KEY | VALUE | EQUALS | COMMA | QUOTE)
        })
    }
}

impl VersionPolicyNode {
    /// Returns the version policy string.
    pub fn policy(&self) -> String {
        join_tokens(&self.0, |k| {
            matches!(k, KEY | VALUE | EQUALS | COMMA | QUOTE)
        })
    }
}

impl ScriptNode {
    /// Returns the script string.
    pub fn script(&self) -> String {
        join_tokens(&self.0, |k| {
            matches!(k, KEY | VALUE | EQUALS | COMMA | QUOTE)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_node_structure() {
        // Test that entries properly use the new node types
        let wf: super::WatchFile = r#"version=4
opts=compression=xz https://example.com/releases (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#
        .parse()
        .unwrap();

        let entry = wf.entries().next().unwrap();

        // Verify URL node exists and works
        assert_eq!(entry.0.children().find(|n| n.kind() == URL).is_some(), true);
        assert_eq!(entry.url(), "https://example.com/releases");

        // Verify MATCHING_PATTERN node exists and works
        assert_eq!(
            entry
                .0
                .children()
                .find(|n| n.kind() == MATCHING_PATTERN)
                .is_some(),
            true
        );
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz".into())
        );

        // Verify VERSION_POLICY node exists and works
        assert_eq!(
            entry
                .0
                .children()
                .find(|n| n.kind() == VERSION_POLICY)
                .is_some(),
            true
        );
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Debian)));

        // Verify SCRIPT node exists and works
        assert_eq!(
            entry.0.children().find(|n| n.kind() == SCRIPT).is_some(),
            true
        );
        assert_eq!(entry.script(), Some("uupdate".into()));
    }

    #[test]
    fn test_entry_node_structure_partial() {
        // Test entry with only URL and pattern (no version or script)
        let wf: super::WatchFile = r#"version=4
https://github.com/example/tags .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let entry = wf.entries().next().unwrap();

        // Should have URL and MATCHING_PATTERN nodes
        assert_eq!(entry.0.children().find(|n| n.kind() == URL).is_some(), true);
        assert_eq!(
            entry
                .0
                .children()
                .find(|n| n.kind() == MATCHING_PATTERN)
                .is_some(),
            true
        );

        // Should NOT have VERSION_POLICY or SCRIPT nodes
        assert_eq!(
            entry
                .0
                .children()
                .find(|n| n.kind() == VERSION_POLICY)
                .is_some(),
            false
        );
        assert_eq!(
            entry.0.children().find(|n| n.kind() == SCRIPT).is_some(),
            false
        );

        // Verify accessors work correctly
        assert_eq!(entry.url(), "https://github.com/example/tags");
        assert_eq!(
            entry.matching_pattern(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz".into())
        );
        assert_eq!(entry.version(), Ok(None));
        assert_eq!(entry.script(), None);
    }

    #[test]
    fn test_parse_v1() {
        const WATCHV1: &str = r#"version=4
opts=bare,filenamemangle=s/.+\/v?(\d\S+)\.tar\.gz/syncthing-gtk-$1\.tar\.gz/ \
  https://github.com/syncthing/syncthing-gtk/tags .*/v?(\d\S+)\.tar\.gz
"#;
        let parsed = parse(WATCHV1);
        //assert_eq!(parsed.errors, Vec::<String>::new());
        let node = parsed.syntax();
        assert_eq!(
            format!("{:#?}", node),
            r#"ROOT@0..161
  VERSION@0..10
    KEY@0..7 "version"
    EQUALS@7..8 "="
    VALUE@8..9 "4"
    NEWLINE@9..10 "\n"
  ENTRY@10..161
    OPTS_LIST@10..86
      KEY@10..14 "opts"
      EQUALS@14..15 "="
      OPTION@15..19
        KEY@15..19 "bare"
      OPTION_SEPARATOR@19..20
        COMMA@19..20 ","
      OPTION@20..86
        KEY@20..34 "filenamemangle"
        EQUALS@34..35 "="
        VALUE@35..86 "s/.+\\/v?(\\d\\S+)\\.tar\\ ..."
    WHITESPACE@86..87 " "
    CONTINUATION@87..89 "\\\n"
    WHITESPACE@89..91 "  "
    URL@91..138
      VALUE@91..138 "https://github.com/sy ..."
    WHITESPACE@138..139 " "
    MATCHING_PATTERN@139..160
      VALUE@139..160 ".*/v?(\\d\\S+)\\.tar\\.gz"
    NEWLINE@160..161 "\n"
"#
        );

        let root = parsed.root();
        assert_eq!(root.version(), 4);
        let entries = root.entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(
            entry.url(),
            "https://github.com/syncthing/syncthing-gtk/tags"
        );
        assert_eq!(
            entry.matching_pattern(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz".into())
        );
        assert_eq!(entry.version(), Ok(None));
        assert_eq!(entry.script(), None);

        assert_eq!(node.text(), WATCHV1);
    }

    #[test]
    fn test_parse_v2() {
        let parsed = parse(
            r#"version=4
https://github.com/syncthing/syncthing-gtk/tags .*/v?(\d\S+)\.tar\.gz
# comment
"#,
        );
        assert_eq!(parsed.errors, Vec::<String>::new());
        let node = parsed.syntax();
        assert_eq!(
            format!("{:#?}", node),
            r###"ROOT@0..90
  VERSION@0..10
    KEY@0..7 "version"
    EQUALS@7..8 "="
    VALUE@8..9 "4"
    NEWLINE@9..10 "\n"
  ENTRY@10..80
    URL@10..57
      VALUE@10..57 "https://github.com/sy ..."
    WHITESPACE@57..58 " "
    MATCHING_PATTERN@58..79
      VALUE@58..79 ".*/v?(\\d\\S+)\\.tar\\.gz"
    NEWLINE@79..80 "\n"
  COMMENT@80..89 "# comment"
  NEWLINE@89..90 "\n"
"###
        );

        let root = parsed.root();
        assert_eq!(root.version(), 4);
        let entries = root.entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(
            entry.url(),
            "https://github.com/syncthing/syncthing-gtk/tags"
        );
        assert_eq!(
            entry.format_url(|| "syncthing-gtk".to_string(), || String::new()),
            "https://github.com/syncthing/syncthing-gtk/tags"
                .parse()
                .unwrap()
        );
    }

    #[test]
    fn test_parse_v3() {
        let parsed = parse(
            r#"version=4
https://github.com/syncthing/@PACKAGE@/tags .*/v?(\d\S+)\.tar\.gz
# comment
"#,
        );
        assert_eq!(parsed.errors, Vec::<String>::new());
        let root = parsed.root();
        assert_eq!(root.version(), 4);
        let entries = root.entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.url(), "https://github.com/syncthing/@PACKAGE@/tags");
        assert_eq!(
            entry.format_url(|| "syncthing-gtk".to_string(), || String::new()),
            "https://github.com/syncthing/syncthing-gtk/tags"
                .parse()
                .unwrap()
        );
    }

    #[test]
    fn test_thread_safe_parsing() {
        let text = r#"version=4
https://github.com/example/example/tags example-(.*)\.tar\.gz
"#;

        let parsed = parse_watch_file(text);
        assert!(parsed.is_ok());
        assert_eq!(parsed.errors().len(), 0);

        // Test that we can get the AST from the parse result
        let watchfile = parsed.tree();
        assert_eq!(watchfile.version(), 4);
        let entries: Vec<_> = watchfile.entries().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_parse_clone_and_eq() {
        let text = r#"version=4
https://github.com/example/example/tags example-(.*)\.tar\.gz
"#;

        let parsed1 = parse_watch_file(text);
        let parsed2 = parsed1.clone();

        // Test that cloned parse results are equal
        assert_eq!(parsed1, parsed2);

        // Test that the AST nodes are also cloneable
        let watchfile1 = parsed1.tree();
        let watchfile2 = watchfile1.clone();
        assert_eq!(watchfile1, watchfile2);
    }

    #[test]
    fn test_parse_v4() {
        let cl: super::WatchFile = r#"version=4
opts=repack,compression=xz,dversionmangle=s/\+ds//,repacksuffix=+ds \
    https://github.com/example/example-cat/tags \
        (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#
        .parse()
        .unwrap();
        assert_eq!(cl.version(), 4);
        let entries = cl.entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.url(), "https://github.com/example/example-cat/tags");
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz".into())
        );
        assert!(entry.repack());
        assert_eq!(entry.compression(), Ok(Some(Compression::Xz)));
        assert_eq!(entry.dversionmangle(), Some("s/\\+ds//".into()));
        assert_eq!(entry.repacksuffix(), Some("+ds".into()));
        assert_eq!(entry.script(), Some("uupdate".into()));
        assert_eq!(
            entry.format_url(|| "example-cat".to_string(), || String::new()),
            "https://github.com/example/example-cat/tags"
                .parse()
                .unwrap()
        );
        assert_eq!(entry.version(), Ok(Some(VersionPolicy::Debian)));
    }

    #[test]
    fn test_git_mode() {
        let text = r#"version=3
opts="mode=git, gitmode=shallow, pgpmode=gittag" \
https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git \
refs/tags/(.*) debian
"#;
        let parsed = parse(text);
        assert_eq!(parsed.errors, Vec::<String>::new());
        let cl = parsed.root();
        assert_eq!(cl.version(), 3);
        let entries = cl.entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(
            entry.url(),
            "https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git"
        );
        assert_eq!(entry.matching_pattern(), Some("refs/tags/(.*)".into()));
        assert_eq!(entry.version(), Ok(Some(VersionPolicy::Debian)));
        assert_eq!(entry.script(), None);
        assert_eq!(entry.gitmode(), Ok(GitMode::Shallow));
        assert_eq!(entry.pgpmode(), Ok(PgpMode::GitTag));
        assert_eq!(entry.mode(), Ok(Mode::Git));
    }

    #[test]
    fn test_parse_quoted() {
        const WATCHV1: &str = r#"version=4
opts="bare, filenamemangle=blah" \
  https://github.com/syncthing/syncthing-gtk/tags .*/v?(\d\S+)\.tar\.gz
"#;
        let parsed = parse(WATCHV1);
        //assert_eq!(parsed.errors, Vec::<String>::new());
        let node = parsed.syntax();

        let root = parsed.root();
        assert_eq!(root.version(), 4);
        let entries = root.entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];

        assert_eq!(
            entry.url(),
            "https://github.com/syncthing/syncthing-gtk/tags"
        );
        assert_eq!(
            entry.matching_pattern(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz".into())
        );
        assert_eq!(entry.version(), Ok(None));
        assert_eq!(entry.script(), None);

        assert_eq!(node.text(), WATCHV1);
    }

    #[test]
    fn test_set_url() {
        // Test setting URL on a simple entry without options
        let wf: super::WatchFile = r#"version=4
https://github.com/syncthing/syncthing-gtk/tags .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(
            entry.url(),
            "https://github.com/syncthing/syncthing-gtk/tags"
        );

        entry.set_url("https://newurl.example.org/path");
        assert_eq!(entry.url(), "https://newurl.example.org/path");
        assert_eq!(
            entry.matching_pattern(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz".into())
        );

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "https://newurl.example.org/path .*/v?(\\d\\S+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_set_url_with_options() {
        // Test setting URL on an entry with options
        let wf: super::WatchFile = r#"version=4
opts=foo=blah https://foo.com/bar .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.url(), "https://foo.com/bar");
        assert_eq!(entry.get_option("foo"), Some("blah".to_string()));

        entry.set_url("https://example.com/baz");
        assert_eq!(entry.url(), "https://example.com/baz");

        // Verify options are preserved
        assert_eq!(entry.get_option("foo"), Some("blah".to_string()));
        assert_eq!(
            entry.matching_pattern(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz".into())
        );

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "opts=foo=blah https://example.com/baz .*/v?(\\d\\S+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_set_url_complex() {
        // Test with a complex watch file with multiple options and continuation
        let wf: super::WatchFile = r#"version=4
opts=bare,filenamemangle=s/.+\/v?(\d\S+)\.tar\.gz/syncthing-gtk-$1\.tar\.gz/ \
  https://github.com/syncthing/syncthing-gtk/tags .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(
            entry.url(),
            "https://github.com/syncthing/syncthing-gtk/tags"
        );

        entry.set_url("https://gitlab.com/newproject/tags");
        assert_eq!(entry.url(), "https://gitlab.com/newproject/tags");

        // Verify all options are preserved
        assert!(entry.bare());
        assert_eq!(
            entry.filenamemangle(),
            Some("s/.+\\/v?(\\d\\S+)\\.tar\\.gz/syncthing-gtk-$1\\.tar\\.gz/".into())
        );
        assert_eq!(
            entry.matching_pattern(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz".into())
        );

        // Verify the exact serialized output preserves structure
        assert_eq!(
            entry.to_string(),
            r#"opts=bare,filenamemangle=s/.+\/v?(\d\S+)\.tar\.gz/syncthing-gtk-$1\.tar\.gz/ \
  https://gitlab.com/newproject/tags .*/v?(\d\S+)\.tar\.gz
"#
        );
    }

    #[test]
    fn test_set_url_with_all_fields() {
        // Test with all fields: options, URL, matching pattern, version, and script
        let wf: super::WatchFile = r#"version=4
opts=repack,compression=xz,dversionmangle=s/\+ds//,repacksuffix=+ds \
    https://github.com/example/example-cat/tags \
        (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.url(), "https://github.com/example/example-cat/tags");
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz".into())
        );
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Debian)));
        assert_eq!(entry.script(), Some("uupdate".into()));

        entry.set_url("https://gitlab.example.org/project/releases");
        assert_eq!(entry.url(), "https://gitlab.example.org/project/releases");

        // Verify all other fields are preserved
        assert!(entry.repack());
        assert_eq!(entry.compression(), Ok(Some(super::Compression::Xz)));
        assert_eq!(entry.dversionmangle(), Some("s/\\+ds//".into()));
        assert_eq!(entry.repacksuffix(), Some("+ds".into()));
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz".into())
        );
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Debian)));
        assert_eq!(entry.script(), Some("uupdate".into()));

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            r#"opts=repack,compression=xz,dversionmangle=s/\+ds//,repacksuffix=+ds \
    https://gitlab.example.org/project/releases \
        (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#
        );
    }

    #[test]
    fn test_set_url_quoted_options() {
        // Test with quoted options
        let wf: super::WatchFile = r#"version=4
opts="bare, filenamemangle=blah" \
  https://github.com/syncthing/syncthing-gtk/tags .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(
            entry.url(),
            "https://github.com/syncthing/syncthing-gtk/tags"
        );

        entry.set_url("https://example.org/new/path");
        assert_eq!(entry.url(), "https://example.org/new/path");

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            r#"opts="bare, filenamemangle=blah" \
  https://example.org/new/path .*/v?(\d\S+)\.tar\.gz
"#
        );
    }

    #[test]
    fn test_set_opt_update_existing() {
        // Test updating an existing option
        let wf: super::WatchFile = r#"version=4
opts=foo=blah,bar=baz https://example.com/releases .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.get_option("foo"), Some("blah".to_string()));
        assert_eq!(entry.get_option("bar"), Some("baz".to_string()));

        entry.set_opt("foo", "updated");
        assert_eq!(entry.get_option("foo"), Some("updated".to_string()));
        assert_eq!(entry.get_option("bar"), Some("baz".to_string()));

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "opts=foo=updated,bar=baz https://example.com/releases .*/v?(\\d\\S+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_set_opt_add_new() {
        // Test adding a new option to existing options
        let wf: super::WatchFile = r#"version=4
opts=foo=blah https://example.com/releases .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.get_option("foo"), Some("blah".to_string()));
        assert_eq!(entry.get_option("bar"), None);

        entry.set_opt("bar", "baz");
        assert_eq!(entry.get_option("foo"), Some("blah".to_string()));
        assert_eq!(entry.get_option("bar"), Some("baz".to_string()));

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "opts=foo=blah,bar=baz https://example.com/releases .*/v?(\\d\\S+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_set_opt_create_options_list() {
        // Test creating a new options list when none exists
        let wf: super::WatchFile = r#"version=4
https://example.com/releases .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.option_list(), None);

        entry.set_opt("compression", "xz");
        assert_eq!(entry.get_option("compression"), Some("xz".to_string()));

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "opts=compression=xz https://example.com/releases .*/v?(\\d\\S+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_del_opt_remove_single() {
        // Test removing a single option from multiple options
        let wf: super::WatchFile = r#"version=4
opts=foo=blah,bar=baz,qux=quux https://example.com/releases .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.get_option("foo"), Some("blah".to_string()));
        assert_eq!(entry.get_option("bar"), Some("baz".to_string()));
        assert_eq!(entry.get_option("qux"), Some("quux".to_string()));

        entry.del_opt_str("bar");
        assert_eq!(entry.get_option("foo"), Some("blah".to_string()));
        assert_eq!(entry.get_option("bar"), None);
        assert_eq!(entry.get_option("qux"), Some("quux".to_string()));

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "opts=foo=blah,qux=quux https://example.com/releases .*/v?(\\d\\S+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_del_opt_remove_first() {
        // Test removing the first option
        let wf: super::WatchFile = r#"version=4
opts=foo=blah,bar=baz https://example.com/releases .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        entry.del_opt_str("foo");
        assert_eq!(entry.get_option("foo"), None);
        assert_eq!(entry.get_option("bar"), Some("baz".to_string()));

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "opts=bar=baz https://example.com/releases .*/v?(\\d\\S+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_del_opt_remove_last() {
        // Test removing the last option
        let wf: super::WatchFile = r#"version=4
opts=foo=blah,bar=baz https://example.com/releases .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        entry.del_opt_str("bar");
        assert_eq!(entry.get_option("foo"), Some("blah".to_string()));
        assert_eq!(entry.get_option("bar"), None);

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "opts=foo=blah https://example.com/releases .*/v?(\\d\\S+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_del_opt_remove_only_option() {
        // Test removing the only option (should remove entire opts list)
        let wf: super::WatchFile = r#"version=4
opts=foo=blah https://example.com/releases .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.get_option("foo"), Some("blah".to_string()));

        entry.del_opt_str("foo");
        assert_eq!(entry.get_option("foo"), None);
        assert_eq!(entry.option_list(), None);

        // Verify the exact serialized output (opts should be gone)
        assert_eq!(
            entry.to_string(),
            "https://example.com/releases .*/v?(\\d\\S+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_del_opt_nonexistent() {
        // Test deleting a non-existent option (should do nothing)
        let wf: super::WatchFile = r#"version=4
opts=foo=blah https://example.com/releases .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        let original = entry.to_string();

        entry.del_opt_str("nonexistent");
        assert_eq!(entry.to_string(), original);
    }

    #[test]
    fn test_set_opt_multiple_operations() {
        // Test multiple set_opt operations
        let wf: super::WatchFile = r#"version=4
https://example.com/releases .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();

        entry.set_opt("compression", "xz");
        entry.set_opt("repack", "");
        entry.set_opt("dversionmangle", "s/\\+ds//");

        assert_eq!(entry.get_option("compression"), Some("xz".to_string()));
        assert_eq!(
            entry.get_option("dversionmangle"),
            Some("s/\\+ds//".to_string())
        );
    }

    #[test]
    fn test_set_matching_pattern() {
        // Test setting matching pattern on a simple entry
        let wf: super::WatchFile = r#"version=4
https://github.com/example/tags .*/v?(\d\S+)\.tar\.gz
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(
            entry.matching_pattern(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz".into())
        );

        entry.set_matching_pattern("(?:.*?/)?v?([\\d.]+)\\.tar\\.gz");
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?([\\d.]+)\\.tar\\.gz".into())
        );

        // Verify URL is preserved
        assert_eq!(entry.url(), "https://github.com/example/tags");

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "https://github.com/example/tags (?:.*?/)?v?([\\d.]+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn test_set_matching_pattern_with_all_fields() {
        // Test with all fields present
        let wf: super::WatchFile = r#"version=4
opts=compression=xz https://example.com/releases (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz".into())
        );

        entry.set_matching_pattern(".*/version-([\\d.]+)\\.tar\\.xz");
        assert_eq!(
            entry.matching_pattern(),
            Some(".*/version-([\\d.]+)\\.tar\\.xz".into())
        );

        // Verify all other fields are preserved
        assert_eq!(entry.url(), "https://example.com/releases");
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Debian)));
        assert_eq!(entry.script(), Some("uupdate".into()));
        assert_eq!(entry.compression(), Ok(Some(super::Compression::Xz)));

        // Verify the exact serialized output
        assert_eq!(
        entry.to_string(),
        "opts=compression=xz https://example.com/releases .*/version-([\\d.]+)\\.tar\\.xz debian uupdate\n"
    );
    }

    #[test]
    fn test_set_version_policy() {
        // Test setting version policy
        let wf: super::WatchFile = r#"version=4
https://example.com/releases (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Debian)));

        entry.set_version_policy("previous");
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Previous)));

        // Verify all other fields are preserved
        assert_eq!(entry.url(), "https://example.com/releases");
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz".into())
        );
        assert_eq!(entry.script(), Some("uupdate".into()));

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "https://example.com/releases (?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz previous uupdate\n"
        );
    }

    #[test]
    fn test_set_version_policy_with_options() {
        // Test with options and continuation
        let wf: super::WatchFile = r#"version=4
opts=repack,compression=xz \
    https://github.com/example/example-cat/tags \
        (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Debian)));

        entry.set_version_policy("ignore");
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Ignore)));

        // Verify all other fields are preserved
        assert_eq!(entry.url(), "https://github.com/example/example-cat/tags");
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz".into())
        );
        assert_eq!(entry.script(), Some("uupdate".into()));
        assert!(entry.repack());

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            r#"opts=repack,compression=xz \
    https://github.com/example/example-cat/tags \
        (?:.*?/)?v?(\d[\d.]*)\.tar\.gz ignore uupdate
"#
        );
    }

    #[test]
    fn test_set_script() {
        // Test setting script
        let wf: super::WatchFile = r#"version=4
https://example.com/releases (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.script(), Some("uupdate".into()));

        entry.set_script("uscan");
        assert_eq!(entry.script(), Some("uscan".into()));

        // Verify all other fields are preserved
        assert_eq!(entry.url(), "https://example.com/releases");
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz".into())
        );
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Debian)));

        // Verify the exact serialized output
        assert_eq!(
            entry.to_string(),
            "https://example.com/releases (?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz debian uscan\n"
        );
    }

    #[test]
    fn test_set_script_with_options() {
        // Test with options
        let wf: super::WatchFile = r#"version=4
opts=compression=xz https://example.com/releases (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#
        .parse()
        .unwrap();

        let mut entry = wf.entries().next().unwrap();
        assert_eq!(entry.script(), Some("uupdate".into()));

        entry.set_script("custom-script.sh");
        assert_eq!(entry.script(), Some("custom-script.sh".into()));

        // Verify all other fields are preserved
        assert_eq!(entry.url(), "https://example.com/releases");
        assert_eq!(
            entry.matching_pattern(),
            Some("(?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz".into())
        );
        assert_eq!(entry.version(), Ok(Some(super::VersionPolicy::Debian)));
        assert_eq!(entry.compression(), Ok(Some(super::Compression::Xz)));

        // Verify the exact serialized output
        assert_eq!(
        entry.to_string(),
        "opts=compression=xz https://example.com/releases (?:.*?/)?v?(\\d[\\d.]*)\\.tar\\.gz debian custom-script.sh\n"
    );
    }

    #[test]
    fn test_apply_dversionmangle() {
        // Test basic dversionmangle
        let wf: super::WatchFile = r#"version=4
opts=dversionmangle=s/\+dfsg$// https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_dversionmangle("1.0+dfsg").unwrap(), "1.0");
        assert_eq!(entry.apply_dversionmangle("1.0").unwrap(), "1.0");

        // Test with versionmangle (fallback)
        let wf: super::WatchFile = r#"version=4
opts=versionmangle=s/^v// https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_dversionmangle("v1.0").unwrap(), "1.0");

        // Test with both dversionmangle and versionmangle (dversionmangle takes precedence)
        let wf: super::WatchFile = r#"version=4
opts=dversionmangle=s/\+ds//,versionmangle=s/^v// https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_dversionmangle("1.0+ds").unwrap(), "1.0");

        // Test without any mangle options
        let wf: super::WatchFile = r#"version=4
https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_dversionmangle("1.0+dfsg").unwrap(), "1.0+dfsg");
    }

    #[test]
    fn test_apply_oversionmangle() {
        // Test basic oversionmangle - adding suffix
        let wf: super::WatchFile = r#"version=4
opts=oversionmangle=s/$/-1/ https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_oversionmangle("1.0").unwrap(), "1.0-1");
        assert_eq!(entry.apply_oversionmangle("2.5.3").unwrap(), "2.5.3-1");

        // Test oversionmangle for adding +dfsg suffix
        let wf: super::WatchFile = r#"version=4
opts=oversionmangle=s/$/.dfsg/ https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_oversionmangle("1.0").unwrap(), "1.0.dfsg");

        // Test without any mangle options
        let wf: super::WatchFile = r#"version=4
https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_oversionmangle("1.0").unwrap(), "1.0");
    }

    #[test]
    fn test_apply_dirversionmangle() {
        // Test basic dirversionmangle - removing 'v' prefix
        let wf: super::WatchFile = r#"version=4
opts=dirversionmangle=s/^v// https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_dirversionmangle("v1.0").unwrap(), "1.0");
        assert_eq!(entry.apply_dirversionmangle("v2.5.3").unwrap(), "2.5.3");

        // Test dirversionmangle with capture groups
        let wf: super::WatchFile = r#"version=4
opts=dirversionmangle=s/v(\d)/$1/ https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_dirversionmangle("v1.0").unwrap(), "1.0");

        // Test without any mangle options
        let wf: super::WatchFile = r#"version=4
https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_dirversionmangle("v1.0").unwrap(), "v1.0");
    }

    #[test]
    fn test_apply_filenamemangle() {
        // Test filenamemangle to generate tarball filename
        let wf: super::WatchFile = r#"version=4
opts=filenamemangle=s/.+\/v?(\d\S+)\.tar\.gz/mypackage-$1.tar.gz/ https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(
            entry
                .apply_filenamemangle("https://example.com/v1.0.tar.gz")
                .unwrap(),
            "mypackage-1.0.tar.gz"
        );
        assert_eq!(
            entry
                .apply_filenamemangle("https://example.com/2.5.3.tar.gz")
                .unwrap(),
            "mypackage-2.5.3.tar.gz"
        );

        // Test filenamemangle with different pattern
        let wf: super::WatchFile = r#"version=4
opts=filenamemangle=s/.*\/(.*)/$1/ https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(
            entry
                .apply_filenamemangle("https://example.com/path/to/file.tar.gz")
                .unwrap(),
            "file.tar.gz"
        );

        // Test without any mangle options
        let wf: super::WatchFile = r#"version=4
https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(
            entry
                .apply_filenamemangle("https://example.com/file.tar.gz")
                .unwrap(),
            "https://example.com/file.tar.gz"
        );
    }

    #[test]
    fn test_apply_pagemangle() {
        // Test pagemangle to decode HTML entities
        let wf: super::WatchFile = r#"version=4
opts=pagemangle=s/&amp;/&/g https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(
            entry.apply_pagemangle(b"foo &amp; bar").unwrap(),
            b"foo & bar"
        );
        assert_eq!(
            entry
                .apply_pagemangle(b"&amp; foo &amp; bar &amp;")
                .unwrap(),
            b"& foo & bar &"
        );

        // Test pagemangle with different pattern
        let wf: super::WatchFile = r#"version=4
opts=pagemangle=s/<[^>]+>//g https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.apply_pagemangle(b"<div>text</div>").unwrap(), b"text");

        // Test without any mangle options
        let wf: super::WatchFile = r#"version=4
https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(
            entry.apply_pagemangle(b"foo &amp; bar").unwrap(),
            b"foo &amp; bar"
        );
    }

    #[test]
    fn test_apply_downloadurlmangle() {
        // Test downloadurlmangle to change URL path
        let wf: super::WatchFile = r#"version=4
opts=downloadurlmangle=s|/archive/|/download/| https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(
            entry
                .apply_downloadurlmangle("https://example.com/archive/file.tar.gz")
                .unwrap(),
            "https://example.com/download/file.tar.gz"
        );

        // Test downloadurlmangle with different pattern
        let wf: super::WatchFile = r#"version=4
opts=downloadurlmangle=s/github\.com/raw.githubusercontent.com/ https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(
            entry
                .apply_downloadurlmangle("https://github.com/user/repo/file.tar.gz")
                .unwrap(),
            "https://raw.githubusercontent.com/user/repo/file.tar.gz"
        );

        // Test without any mangle options
        let wf: super::WatchFile = r#"version=4
https://example.com/ .*
"#
        .parse()
        .unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(
            entry
                .apply_downloadurlmangle("https://example.com/archive/file.tar.gz")
                .unwrap(),
            "https://example.com/archive/file.tar.gz"
        );
    }

    #[test]
    fn test_entry_builder_minimal() {
        // Test creating a minimal entry with just URL and pattern
        let entry = super::EntryBuilder::new("https://github.com/example/tags")
            .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
            .build();

        assert_eq!(entry.url(), "https://github.com/example/tags");
        assert_eq!(
            entry.matching_pattern().as_deref(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz")
        );
        assert_eq!(entry.version(), Ok(None));
        assert_eq!(entry.script(), None);
        assert!(entry.opts().is_empty());
    }

    #[test]
    fn test_entry_builder_url_only() {
        // Test creating an entry with just URL
        let entry = super::EntryBuilder::new("https://example.com/releases").build();

        assert_eq!(entry.url(), "https://example.com/releases");
        assert_eq!(entry.matching_pattern(), None);
        assert_eq!(entry.version(), Ok(None));
        assert_eq!(entry.script(), None);
        assert!(entry.opts().is_empty());
    }

    #[test]
    fn test_entry_builder_with_all_fields() {
        // Test creating an entry with all fields
        let entry = super::EntryBuilder::new("https://github.com/example/tags")
            .matching_pattern(".*/v?(\\d[\\d.]*)\\.tar\\.gz")
            .version_policy("debian")
            .script("uupdate")
            .opt("compression", "xz")
            .flag("repack")
            .build();

        assert_eq!(entry.url(), "https://github.com/example/tags");
        assert_eq!(
            entry.matching_pattern().as_deref(),
            Some(".*/v?(\\d[\\d.]*)\\.tar\\.gz")
        );
        assert_eq!(entry.version(), Ok(Some(VersionPolicy::Debian)));
        assert_eq!(entry.script(), Some("uupdate".into()));
        assert_eq!(entry.get_option("compression"), Some("xz".to_string()));
        assert!(entry.has_option("repack"));
        assert!(entry.repack());
    }

    #[test]
    fn test_entry_builder_multiple_options() {
        // Test creating an entry with multiple options
        let entry = super::EntryBuilder::new("https://example.com/tags")
            .matching_pattern(".*/v?(\\d+\\.\\d+)\\.tar\\.gz")
            .opt("compression", "xz")
            .opt("dversionmangle", "s/\\+ds//")
            .opt("repacksuffix", "+ds")
            .build();

        assert_eq!(entry.get_option("compression"), Some("xz".to_string()));
        assert_eq!(
            entry.get_option("dversionmangle"),
            Some("s/\\+ds//".to_string())
        );
        assert_eq!(entry.get_option("repacksuffix"), Some("+ds".to_string()));
    }

    #[test]
    fn test_entry_builder_via_entry() {
        // Test using Entry::builder() convenience method
        let entry = super::Entry::builder("https://github.com/example/tags")
            .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
            .version_policy("debian")
            .build();

        assert_eq!(entry.url(), "https://github.com/example/tags");
        assert_eq!(
            entry.matching_pattern().as_deref(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz")
        );
        assert_eq!(entry.version(), Ok(Some(VersionPolicy::Debian)));
    }

    #[test]
    fn test_watchfile_add_entry_to_empty() {
        // Test adding an entry to an empty watchfile
        let mut wf = super::WatchFile::new(Some(4));

        let entry = super::EntryBuilder::new("https://github.com/example/tags")
            .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
            .build();

        wf.add_entry(entry);

        assert_eq!(wf.version(), 4);
        assert_eq!(wf.entries().count(), 1);

        let added_entry = wf.entries().next().unwrap();
        assert_eq!(added_entry.url(), "https://github.com/example/tags");
        assert_eq!(
            added_entry.matching_pattern().as_deref(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz")
        );
    }

    #[test]
    fn test_watchfile_add_multiple_entries() {
        // Test adding multiple entries to a watchfile
        let mut wf = super::WatchFile::new(Some(4));

        wf.add_entry(
            super::EntryBuilder::new("https://github.com/example1/tags")
                .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
                .build(),
        );

        wf.add_entry(
            super::EntryBuilder::new("https://github.com/example2/releases")
                .matching_pattern(".*/(\\d+\\.\\d+)\\.tar\\.gz")
                .opt("compression", "xz")
                .build(),
        );

        assert_eq!(wf.entries().count(), 2);

        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries[0].url(), "https://github.com/example1/tags");
        assert_eq!(entries[1].url(), "https://github.com/example2/releases");
        assert_eq!(entries[1].get_option("compression"), Some("xz".to_string()));
    }

    #[test]
    fn test_watchfile_add_entry_to_existing() {
        // Test adding an entry to a watchfile that already has entries
        let mut wf: super::WatchFile = r#"version=4
https://example.com/old .*/v?(\\d\\S+)\\.tar\\.gz
"#
        .parse()
        .unwrap();

        assert_eq!(wf.entries().count(), 1);

        wf.add_entry(
            super::EntryBuilder::new("https://github.com/example/new")
                .matching_pattern(".*/v?(\\d+\\.\\d+)\\.tar\\.gz")
                .opt("compression", "xz")
                .version_policy("debian")
                .build(),
        );

        assert_eq!(wf.entries().count(), 2);

        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries[0].url(), "https://example.com/old");
        assert_eq!(entries[1].url(), "https://github.com/example/new");
        assert_eq!(entries[1].version(), Ok(Some(VersionPolicy::Debian)));
    }

    #[test]
    fn test_entry_builder_formatting() {
        // Test that the builder produces correctly formatted entries
        let entry = super::EntryBuilder::new("https://github.com/example/tags")
            .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
            .opt("compression", "xz")
            .flag("repack")
            .version_policy("debian")
            .script("uupdate")
            .build();

        let entry_str = entry.to_string();

        // Should start with opts=
        assert!(entry_str.starts_with("opts="));
        // Should contain the URL
        assert!(entry_str.contains("https://github.com/example/tags"));
        // Should contain the pattern
        assert!(entry_str.contains(".*/v?(\\d\\S+)\\.tar\\.gz"));
        // Should contain version policy
        assert!(entry_str.contains("debian"));
        // Should contain script
        assert!(entry_str.contains("uupdate"));
        // Should end with newline
        assert!(entry_str.ends_with('\n'));
    }

    #[test]
    fn test_watchfile_add_entry_preserves_format() {
        // Test that adding entries preserves the watchfile format
        let mut wf = super::WatchFile::new(Some(4));

        wf.add_entry(
            super::EntryBuilder::new("https://github.com/example/tags")
                .matching_pattern(".*/v?(\\d\\S+)\\.tar\\.gz")
                .build(),
        );

        let wf_str = wf.to_string();

        // Should have version line
        assert!(wf_str.starts_with("version=4\n"));
        // Should have the entry
        assert!(wf_str.contains("https://github.com/example/tags"));

        // Parse it back and ensure it's still valid
        let reparsed: super::WatchFile = wf_str.parse().unwrap();
        assert_eq!(reparsed.version(), 4);
        assert_eq!(reparsed.entries().count(), 1);
    }

    #[test]
    fn test_line_col() {
        let text = r#"version=4
opts=compression=xz https://example.com/releases (?:.*?/)?v?(\d[\d.]*)\.tar\.gz debian uupdate
"#;
        let wf = text.parse::<super::WatchFile>().unwrap();

        // Test version line position
        let version_node = wf.version_node().unwrap();
        assert_eq!(version_node.line(), 0);
        assert_eq!(version_node.column(), 0);
        assert_eq!(version_node.line_col(), (0, 0));

        // Test entry line numbers
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        // Entry starts at line 1
        assert_eq!(entries[0].line(), 1);
        assert_eq!(entries[0].column(), 0);
        assert_eq!(entries[0].line_col(), (1, 0));

        // Test node accessors
        let option_list = entries[0].option_list().unwrap();
        assert_eq!(option_list.line(), 1); // Option list is on line 1

        let url_node = entries[0].url_node().unwrap();
        assert_eq!(url_node.line(), 1); // URL is on line 1

        let pattern_node = entries[0].matching_pattern_node().unwrap();
        assert_eq!(pattern_node.line(), 1); // Pattern is on line 1

        let version_policy_node = entries[0].version_node().unwrap();
        assert_eq!(version_policy_node.line(), 1); // Version policy is on line 1

        let script_node = entries[0].script_node().unwrap();
        assert_eq!(script_node.line(), 1); // Script is on line 1

        // Test individual option nodes
        let options: Vec<_> = option_list.options().collect();
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].key(), Some("compression".to_string()));
        assert_eq!(options[0].value(), Some("xz".to_string()));
        assert_eq!(options[0].line(), 1); // Option is on line 1

        // Test find_option
        let compression_opt = option_list.find_option("compression").unwrap();
        assert_eq!(compression_opt.line(), 1);
        assert_eq!(compression_opt.column(), 5); // After "opts="
        assert_eq!(compression_opt.line_col(), (1, 5));
    }

    #[test]
    fn test_parse_str_relaxed() {
        let wf: super::WatchFile = super::WatchFile::from_str_relaxed(
            r#"version=4
ERRORS IN THIS LINE
opts=compression=xz https://example.com/releases (?:.*?/)?v?(\d
"#,
        );
        assert_eq!(wf.version(), 4);
        assert_eq!(wf.entries().count(), 2);

        let entries = wf.entries().collect::<Vec<_>>();

        let entry = &entries[0];
        assert_eq!(entry.url(), "ERRORS");

        let entry = &entries[1];
        assert_eq!(entry.url(), "https://example.com/releases");
        assert_eq!(entry.matching_pattern().as_deref(), Some("(?:.*?/)?v?(\\d"));
        assert_eq!(entry.get_option("compression"), Some("xz".to_string()));
    }

    #[test]
    fn test_parse_entry_with_comment_before() {
        // Regression test for https://bugs.debian.org/1128319:
        // A comment line before an entry with a continuation line was not parsed correctly
        // - the entry was silently dropped.
        let input = concat!(
            "version=4\n",
            "# try also https://pypi.debian.net/tomoscan/watch\n",
            "opts=uversionmangle=s/(rc|a|b|c)/~$1/;s/\\.dev/~dev/ \\\n",
            "https://pypi.debian.net/tomoscan/tomoscan-(.+)\\.(?:zip|tgz|tbz|txz|(?:tar\\.(?:gz|bz2|xz)))\n"
        );
        let wf: super::WatchFile = input.parse().unwrap();
        // The CST must cover the full input (round-trip invariant)
        assert_eq!(wf.to_string(), input);
        assert_eq!(wf.entries().count(), 1);
        let entry = wf.entries().next().unwrap();
        assert_eq!(
            entry.url(),
            "https://pypi.debian.net/tomoscan/tomoscan-(.+)\\.(?:zip|tgz|tbz|txz|(?:tar\\.(?:gz|bz2|xz)))"
        );
        assert_eq!(
            entry.get_option("uversionmangle"),
            Some("s/(rc|a|b|c)/~$1/;s/\\.dev/~dev/".to_string())
        );
    }

    #[test]
    fn test_parse_multiple_comments_before_entry() {
        // Multiple consecutive comment lines before an entry should all be preserved
        // and the entry should still be parsed correctly.
        let input = concat!(
            "version=4\n",
            "# first comment\n",
            "# second comment\n",
            "# third comment\n",
            "https://example.com/foo foo-(.*).tar.gz\n",
        );
        let wf: super::WatchFile = input.parse().unwrap();
        assert_eq!(wf.to_string(), input);
        assert_eq!(wf.entries().count(), 1);
        assert_eq!(
            wf.entries().next().unwrap().url(),
            "https://example.com/foo"
        );
    }

    #[test]
    fn test_parse_blank_lines_between_entries() {
        // Blank lines between entries should be preserved and all entries parsed.
        let input = concat!(
            "version=4\n",
            "https://example.com/foo .*/foo-(\\d+)\\.tar\\.gz\n",
            "\n",
            "https://example.com/bar .*/bar-(\\d+)\\.tar\\.gz\n",
        );
        let wf: super::WatchFile = input.parse().unwrap();
        assert_eq!(wf.to_string(), input);
        assert_eq!(wf.entries().count(), 2);
    }

    #[test]
    fn test_parse_trailing_unparseable_tokens_produce_error() {
        // Any tokens that remain after all entries are parsed should be captured
        // in an ERROR node so the CST covers the full input, and an error is reported.
        let input = "version=4\nhttps://example.com/foo foo-(.*).tar.gz\n=garbage\n";
        let result = input.parse::<super::WatchFile>();
        assert!(result.is_err(), "expected parse error for trailing garbage");
        // Verify the round-trip via from_str_relaxed: the CST must cover all input.
        let wf = super::WatchFile::from_str_relaxed(input);
        assert_eq!(wf.to_string(), input);
    }

    #[test]
    fn test_parse_roundtrip_full_file() {
        // The CST must always cover the full input, so to_string() == original input.
        let inputs = [
            "version=4\nhttps://example.com/foo foo-(.*).tar.gz\n",
            "version=4\n# a comment\nhttps://example.com/foo foo-(.*).tar.gz\n",
            concat!(
                "version=4\n",
                "opts=uversionmangle=s/rc/~rc/ \\\n",
                "  https://example.com/foo foo-(.*).tar.gz\n",
            ),
            concat!(
                "version=4\n",
                "# comment before entry\n",
                "opts=uversionmangle=s/rc/~rc/ \\\n",
                "https://example.com/foo foo-(.*).tar.gz\n",
                "# comment between entries\n",
                "https://example.com/bar bar-(.*).tar.gz\n",
            ),
        ];
        for input in &inputs {
            let wf: super::WatchFile = input.parse().unwrap();
            assert_eq!(
                wf.to_string(),
                *input,
                "round-trip failed for input: {:?}",
                input
            );
        }
    }

    #[test]
    fn test_parse_url_with_equals_in_query_string() {
        // Regression: URLs with query strings like `?per_page=100` were lexed
        // as VALUE EQUALS VALUE and tripped up the entry-field parser.
        let input = concat!(
            "version=4\n",
            "https://api.github.com/repos/x/releases?per_page=100 \\\n",
            "  https://github.com/x/v[^/]+/x.tar.gz\n",
        );
        let wf: super::WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].url(),
            "https://api.github.com/repos/x/releases?per_page=100"
        );
        assert_eq!(
            entries[0].matching_pattern().as_deref(),
            Some("https://github.com/x/v[^/]+/x.tar.gz"),
        );
        assert_eq!(wf.to_string(), input);
    }

    #[test]
    fn test_entry_url_does_not_panic_when_empty() {
        // Pathological entries that come out of the parser without a URL
        // node must not panic on `Entry::url()` — return an empty string.
        let input = "version=4\n=garbage\n";
        let wf = super::WatchFile::from_str_relaxed(input);
        for entry in wf.entries() {
            let _ = entry.url();
        }
    }

    #[test]
    fn test_parse_url_node_with_equals_join_tokens() {
        // Even if the lexer split the URL across tokens, the URL accessor
        // should reassemble them.
        let input = "version=4\nhttps://example.com/x?y=1&z=2 .*tar.gz\n";
        let wf: super::WatchFile = input.parse().unwrap();
        let entry = wf.entries().next().unwrap();
        assert_eq!(entry.url(), "https://example.com/x?y=1&z=2");
    }

    #[test]
    fn test_parse_quoted_opts_with_trailing_comma_continuation() {
        // Regression (golang-github-varlink-go style): each option line ends
        // with `,\` and the closing quote sits on its own line. The parser
        // must skip the whitespace/continuation before checking for the
        // closing quote so the trailing comma doesn't kick off another
        // (empty) option.
        let input = concat!(
            "version=4\n\n",
            "opts=\"\\\n",
            "pgpmode=none,\\\n",
            "repack,compression=xz,repacksuffix=+dfsg,\\\n",
            "dversionmangle=s{[+~]dfsg\\d*}{},\\\n",
            "\" https://github.com/varlink/go/releases \\\n",
            "  .*/archive/v?(\\d[\\d\\.]+)\\.tar\\.gz\n",
        );
        let wf: super::WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url(), "https://github.com/varlink/go/releases");
        assert_eq!(
            entries[0].matching_pattern().as_deref(),
            Some(".*/archive/v?(\\d[\\d\\.]+)\\.tar\\.gz"),
        );
        assert_eq!(wf.to_string(), input);
    }

    #[test]
    fn test_parse_quoted_opts_with_spaces_around_comma() {
        // Regression (libiio style): `opts="a=1 , b=2"` with whitespace
        // around the comma inside quotes.
        let input = concat!(
            "version=4\n",
            "opts=\"filenamemangle=s/.+\\/v?(\\d\\S*)\\.tar\\.gz/v$1.tar.gz/ , uversionmangle=tr%-rc%~rc%\" \\\n",
            "  https://github.com/analogdevicesinc/libiio/tags .*/v(\\d\\S*)\\.tar\\.gz\n",
        );
        let wf: super::WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].url(),
            "https://github.com/analogdevicesinc/libiio/tags",
        );
        assert_eq!(wf.to_string(), input);
    }

    #[test]
    fn test_parse_unquoted_opts_trailing_comma_then_url() {
        // Regression (rally-openstack style): opts ends with `,\` and the URL
        // begins on the next physical line. The trailing comma should not
        // make the parser eat the URL as a malformed option.
        let input = concat!(
            "version=3\n",
            "opts=uversionmangle=s/(rc|a|b|c)/~$1/,\\\n",
            "https://github.com/openstack/rally/tags .*/(\\d\\S*)\\.tar\\.gz\n",
        );
        let wf: super::WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url(), "https://github.com/openstack/rally/tags");
        assert_eq!(
            entries[0].matching_pattern().as_deref(),
            Some(".*/(\\d\\S*)\\.tar\\.gz"),
        );
        assert_eq!(wf.to_string(), input);
    }

    #[test]
    fn test_parse_unquoted_opts_value_with_equals() {
        // Regression: `s/.*ref=//` in an option value contains `=` which the
        // lexer treats as a separator. The option-value loop must keep
        // gobbling until it hits a real option boundary.
        let input = concat!(
            "version=4\n",
            "opts=dversionmangle=s/\\~dfsg//,downloadurlmangle=s/.*ref=//,pgpsigurlmangle=s/$/.asc/ \\\n",
            "\thttps://downloads.asterisk.org/pub/telephony/libpri/releases/ libpri-([0-9.]*)\\.tar\\.gz debian uupdate\n",
        );
        let wf: super::WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].url(),
            "https://downloads.asterisk.org/pub/telephony/libpri/releases/"
        );
        assert_eq!(
            entries[0].matching_pattern().as_deref(),
            Some("libpri-([0-9.]*)\\.tar\\.gz"),
        );
        assert_eq!(wf.to_string(), input);
    }
}
