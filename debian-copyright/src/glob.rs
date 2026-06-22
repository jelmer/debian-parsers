/// A compiled DEP-5 glob pattern that can efficiently match many paths.
///
/// The pattern uses the glob syntax defined by the DEP-5 specification:
/// `*` matches any sequence of characters, `?` matches a single character,
/// and backslash escapes `*`, `?` and `\`.
///
/// # Examples
///
/// ```
/// let pat = debian_copyright::GlobPattern::new("src/*.rs");
/// assert!(pat.is_match("src/main.rs"));
/// assert!(!pat.is_match("lib/main.rs"));
/// ```
pub struct GlobPattern(regex::Regex);

impl GlobPattern {
    /// Compile a DEP-5 glob pattern.
    pub fn new(pattern: &str) -> Self {
        Self(glob_to_regex_inner(pattern))
    }

    /// Check whether a path matches this pattern.
    pub fn is_match(&self, path: &str) -> bool {
        self.0.is_match(path)
    }
}

/// Decode a DEP-5 file pattern that names a single literal path.
///
/// A pattern is literal when it has no unescaped `*` or `?` wildcard. Escapes
/// (`\*`, `\?`, `\\`) are resolved to the characters they stand for, so the
/// returned string is the actual filename the pattern designates. Patterns that
/// contain a wildcard (and therefore match many paths) return `None`.
///
/// # Examples
///
/// ```
/// assert_eq!(debian_copyright::glob::literal_path("src/main.rs").as_deref(), Some("src/main.rs"));
/// assert_eq!(debian_copyright::glob::literal_path(r"\*.txt").as_deref(), Some("*.txt"));
/// assert_eq!(debian_copyright::glob::literal_path("src/*.rs"), None);
/// ```
///
/// A malformed trailing or invalid escape returns `None` rather than panicking,
/// since callers decode arbitrary file contents.
pub fn literal_path(pattern: &str) -> Option<String> {
    let mut out = String::with_capacity(pattern.len());
    let mut it = pattern.chars();
    while let Some(c) = it.next() {
        match c {
            '*' | '?' => return None,
            '\\' => match it.next() {
                Some(esc @ ('*' | '?' | '\\')) => out.push(esc),
                _ => return None,
            },
            c => out.push(c),
        }
    }
    Some(out)
}

/// Whether a DEP-5 file pattern contains a wildcard, i.e. can match more than
/// one path. The inverse of [`literal_path`] returning `Some`.
pub fn is_glob(pattern: &str) -> bool {
    literal_path(pattern).is_none()
}

/// Convert a glob pattern to a regular expression.
#[deprecated(since = "0.1.46", note = "use GlobPattern instead")]
pub fn glob_to_regex(glob: &str) -> regex::Regex {
    glob_to_regex_inner(glob)
}

fn glob_to_regex_inner(glob: &str) -> regex::Regex {
    let mut it = glob.chars();
    let mut r = "^".to_string();

    while let Some(c) = it.next() {
        match c {
            '*' => r.push_str(".*"),
            '?' => r.push('.'),
            '\\' => {
                let c = it.next();
                match c {
                    Some('?') | Some('*') | Some('\\') => {
                        let escaped = regex::escape(&c.unwrap().to_string());
                        r.push_str(&escaped);
                    }
                    Some(x) => {
                        panic!("invalid escape sequence: \\{}", x);
                    }
                    None => {
                        panic!("invalid escape sequence: \\");
                    }
                }
            }
            c => {
                let escaped = regex::escape(&c.to_string());
                r.push_str(&escaped);
            }
        }
    }

    r.push('$');

    regex::Regex::new(r.as_str()).unwrap()
}

#[cfg(test)]
mod tests {
    #[allow(deprecated)]
    #[test]
    fn test_simple() {
        let r = super::glob_to_regex("*.rs");
        assert!(r.is_match("foo.rs"));
        assert!(r.is_match("bar.rs"));
        assert!(!r.is_match("foo.rs.bak"));
        assert!(!r.is_match("foo"));
    }

    #[allow(deprecated)]
    #[test]
    fn test_single_char() {
        let r = super::glob_to_regex("?.rs");
        assert!(r.is_match("a.rs"));
        assert!(r.is_match("b.rs"));
        assert!(!r.is_match("foo.rs"));
        assert!(!r.is_match("foo"));
    }

    #[allow(deprecated)]
    #[test]
    fn test_escape() {
        let r = super::glob_to_regex(r"\?.rs");
        assert!(r.is_match("?.rs"));
        assert!(!r.is_match("a.rs"));
        assert!(!r.is_match("b.rs"));

        let r = super::glob_to_regex(r"\*.rs");
        assert!(r.is_match("*.rs"));
        assert!(!r.is_match("a.rs"));
        assert!(!r.is_match("b.rs"));

        let r = super::glob_to_regex(r"\\?.rs");
        assert!(r.is_match("\\a.rs"));
        assert!(r.is_match("\\b.rs"));
        assert!(!r.is_match("a.rs"));
    }

    #[allow(deprecated)]
    #[should_panic]
    #[test]
    fn test_invalid_escape() {
        super::glob_to_regex(r"\x.rs");
    }

    #[allow(deprecated)]
    #[should_panic]
    #[test]
    fn test_invalid_escape2() {
        super::glob_to_regex(r"\");
    }

    #[test]
    fn test_glob_pattern_wildcard() {
        let pat = super::GlobPattern::new("src/*.rs");
        assert!(pat.is_match("src/main.rs"));
        assert!(pat.is_match("src/lib.rs"));
        assert!(!pat.is_match("lib/main.rs"));
        assert!(!pat.is_match("src/main.rs.bak"));
    }

    #[test]
    fn test_glob_pattern_deep_wildcard() {
        let pat = super::GlobPattern::new("src/*");
        assert!(pat.is_match("src/foo"));
        assert!(pat.is_match("src/foo/bar.rs"));
        assert!(!pat.is_match("lib/foo"));
    }

    #[test]
    fn test_glob_pattern_question_mark() {
        let pat = super::GlobPattern::new("file?.txt");
        assert!(pat.is_match("file1.txt"));
        assert!(pat.is_match("fileA.txt"));
        assert!(!pat.is_match("file10.txt"));
        assert!(!pat.is_match("file.txt"));
    }

    #[test]
    fn test_glob_pattern_literal() {
        let pat = super::GlobPattern::new("LICENSE");
        assert!(pat.is_match("LICENSE"));
        assert!(!pat.is_match("LICENSE.md"));
        assert!(!pat.is_match("NOLICENSE"));
    }

    #[test]
    fn test_glob_pattern_escaped_star() {
        let pat = super::GlobPattern::new(r"\*.txt");
        assert!(pat.is_match("*.txt"));
        assert!(!pat.is_match("foo.txt"));
    }

    #[test]
    fn test_glob_pattern_escaped_question() {
        let pat = super::GlobPattern::new(r"\?.txt");
        assert!(pat.is_match("?.txt"));
        assert!(!pat.is_match("a.txt"));
    }

    #[test]
    fn test_glob_pattern_escaped_backslash() {
        let pat = super::GlobPattern::new(r"\\foo");
        assert!(pat.is_match(r"\foo"));
        assert!(!pat.is_match("foo"));
    }

    #[should_panic]
    #[test]
    fn test_glob_pattern_invalid_escape() {
        super::GlobPattern::new(r"\x");
    }

    #[should_panic]
    #[test]
    fn test_glob_pattern_trailing_backslash() {
        super::GlobPattern::new(r"\");
    }

    #[test]
    fn test_glob_pattern_regex_special_chars() {
        let pat = super::GlobPattern::new("file(1).txt");
        assert!(pat.is_match("file(1).txt"));
        assert!(!pat.is_match("file1.txt"));
    }

    #[test]
    fn test_literal_path_plain() {
        assert_eq!(
            super::literal_path("src/main.rs").as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(super::literal_path("LICENSE").as_deref(), Some("LICENSE"));
    }

    #[test]
    fn test_literal_path_unescapes() {
        assert_eq!(super::literal_path(r"\*.txt").as_deref(), Some("*.txt"));
        assert_eq!(super::literal_path(r"\?.txt").as_deref(), Some("?.txt"));
        assert_eq!(super::literal_path(r"a\\b").as_deref(), Some(r"a\b"));
    }

    #[test]
    fn test_literal_path_rejects_globs() {
        assert_eq!(super::literal_path("src/*.rs"), None);
        assert_eq!(super::literal_path("file?.txt"), None);
    }

    #[test]
    fn test_literal_path_rejects_invalid_escape() {
        // A trailing or invalid escape is not a valid literal; return None
        // rather than panicking on arbitrary input.
        assert_eq!(super::literal_path(r"foo\"), None);
        assert_eq!(super::literal_path(r"foo\x"), None);
    }

    #[test]
    fn test_is_glob() {
        assert!(super::is_glob("src/*.rs"));
        assert!(super::is_glob("file?.txt"));
        assert!(!super::is_glob("src/main.rs"));
        assert!(!super::is_glob(r"\*.txt"));
    }
}
