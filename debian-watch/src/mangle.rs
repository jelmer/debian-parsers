//! Functions for parsing and applying version and URL mangling expressions.
//!
//! Debian watch files use sed-style expressions for transforming versions and URLs.

use regex::Regex;

/// Error type for mangling expression parsing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MangleError {
    /// Not a substitution or translation expression
    NotMangleExpr(String),
    /// Invalid substitution expression
    InvalidSubstExpr(String),
    /// Invalid translation expression
    InvalidTranslExpr(String),
    /// Regex compilation error
    RegexError(String),
}

impl std::fmt::Display for MangleError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            MangleError::NotMangleExpr(s) => {
                write!(f, "not a substitution or translation expression: {}", s)
            }
            MangleError::InvalidSubstExpr(s) => write!(f, "invalid substitution expression: {}", s),
            MangleError::InvalidTranslExpr(s) => write!(f, "invalid translation expression: {}", s),
            MangleError::RegexError(s) => write!(f, "regex error: {}", s),
        }
    }
}

impl std::error::Error for MangleError {}

/// Type of mangling expression
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MangleExprKind {
    /// Substitution (s/pattern/replacement/flags)
    Subst,
    /// Translation (tr/pattern/replacement/flags or y/pattern/replacement/flags)
    Transl,
}

/// A parsed mangling expression
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MangleExpr {
    /// The kind of expression
    pub kind: MangleExprKind,
    /// The pattern to match
    pub pattern: String,
    /// The replacement string
    pub replacement: String,
    /// Optional flags
    pub flags: Option<String>,
}

/// Parse a mangling expression
///
/// # Examples
///
/// ```
/// use debian_watch::mangle::parse_mangle_expr;
///
/// let expr = parse_mangle_expr("s/foo/bar/g").unwrap();
/// assert_eq!(expr.pattern, "foo");
/// assert_eq!(expr.replacement, "bar");
/// assert_eq!(expr.flags.as_deref(), Some("g"));
/// ```
pub fn parse_mangle_expr(vm: &str) -> Result<MangleExpr, MangleError> {
    if vm.starts_with('s') {
        parse_subst_expr(vm)
    } else if vm.starts_with("tr") {
        parse_transl_expr(vm)
    } else if vm.starts_with('y') {
        parse_transl_expr(vm)
    } else {
        Err(MangleError::NotMangleExpr(vm.to_string()))
    }
}

/// Parse a substitution expression (s/pattern/replacement/flags)
///
/// # Examples
///
/// ```
/// use debian_watch::mangle::parse_subst_expr;
///
/// let expr = parse_subst_expr("s/foo/bar/g").unwrap();
/// assert_eq!(expr.pattern, "foo");
/// assert_eq!(expr.replacement, "bar");
/// assert_eq!(expr.flags.as_deref(), Some("g"));
///
/// let expr = parse_subst_expr("s|foo|bar|").unwrap();
/// assert_eq!(expr.pattern, "foo");
/// assert_eq!(expr.replacement, "bar");
/// ```
pub fn parse_subst_expr(vm: &str) -> Result<MangleExpr, MangleError> {
    if !vm.starts_with('s') {
        return Err(MangleError::InvalidSubstExpr(
            "not a substitution expression".to_string(),
        ));
    }

    if vm.len() < 2 {
        return Err(MangleError::InvalidSubstExpr(
            "expression too short".to_string(),
        ));
    }

    let delimiter = vm.chars().nth(1).unwrap();
    let rest = &vm[2..];

    // Split by unescaped delimiter
    let parts = split_by_unescaped_delimiter(rest, delimiter);

    if parts.len() < 2 {
        return Err(MangleError::InvalidSubstExpr(
            "not enough parts".to_string(),
        ));
    }

    let pattern = parts[0].clone();
    let replacement = parts[1].clone();
    let flags = if parts.len() > 2 && !parts[2].is_empty() {
        Some(parts[2].clone())
    } else {
        None
    };

    Ok(MangleExpr {
        kind: MangleExprKind::Subst,
        pattern,
        replacement,
        flags,
    })
}

/// Parse a translation expression (tr/pattern/replacement/flags or y/pattern/replacement/flags)
///
/// # Examples
///
/// ```
/// use debian_watch::mangle::parse_transl_expr;
///
/// let expr = parse_transl_expr("tr/a-z/A-Z/").unwrap();
/// assert_eq!(expr.pattern, "a-z");
/// assert_eq!(expr.replacement, "A-Z");
/// ```
pub fn parse_transl_expr(vm: &str) -> Result<MangleExpr, MangleError> {
    let rest = if vm.starts_with("tr") {
        &vm[2..]
    } else if vm.starts_with('y') {
        &vm[1..]
    } else {
        return Err(MangleError::InvalidTranslExpr(
            "not a translation expression".to_string(),
        ));
    };

    if rest.is_empty() {
        return Err(MangleError::InvalidTranslExpr(
            "expression too short".to_string(),
        ));
    }

    let delimiter = rest.chars().next().unwrap();
    let rest = &rest[1..];

    // Split by unescaped delimiter
    let parts = split_by_unescaped_delimiter(rest, delimiter);

    if parts.len() < 2 {
        return Err(MangleError::InvalidTranslExpr(
            "not enough parts".to_string(),
        ));
    }

    let pattern = parts[0].clone();
    let replacement = parts[1].clone();
    let flags = if parts.len() > 2 && !parts[2].is_empty() {
        Some(parts[2].clone())
    } else {
        None
    };

    Ok(MangleExpr {
        kind: MangleExprKind::Transl,
        pattern,
        replacement,
        flags,
    })
}

/// Split a string by an unescaped delimiter
fn split_by_unescaped_delimiter(s: &str, delimiter: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for c in s.chars() {
        if escaped {
            current.push(c);
            escaped = false;
        } else if c == '\\' {
            current.push(c);
            escaped = true;
        } else if c == delimiter {
            parts.push(current.clone());
            current.clear();
        } else {
            current.push(c);
        }
    }

    // Don't forget the last part
    parts.push(current);

    parts
}

/// Apply a mangling expression to a string
///
/// # Examples
///
/// ```
/// use debian_watch::mangle::apply_mangle;
///
/// let result = apply_mangle("s/foo/bar/", "foo baz foo").unwrap();
/// assert_eq!(result, "bar baz foo");
///
/// let result = apply_mangle("s/foo/bar/g", "foo baz foo").unwrap();
/// assert_eq!(result, "bar baz bar");
/// ```
pub fn apply_mangle(vm: &str, orig: &str) -> Result<String, MangleError> {
    let expr = parse_mangle_expr(vm)?;

    match expr.kind {
        MangleExprKind::Subst => {
            let re =
                Regex::new(&expr.pattern).map_err(|e| MangleError::RegexError(e.to_string()))?;

            // Check if 'g' flag is present for global replacement
            let global = expr.flags.as_ref().is_some_and(|f| f.contains('g'));

            if global {
                Ok(re.replace_all(orig, expr.replacement.as_str()).to_string())
            } else {
                Ok(re.replace(orig, expr.replacement.as_str()).to_string())
            }
        }
        MangleExprKind::Transl => {
            // Translation: character-by-character replacement
            apply_translation(&expr.pattern, &expr.replacement, orig)
        }
    }
}

/// Apply a mangling expression with template variable substitution
///
/// This first substitutes template variables like @PACKAGE@ and @COMPONENT@ in the
/// mangle expression itself, then applies the mangle to the input string.
///
/// # Examples
///
/// ```
/// use debian_watch::mangle::apply_mangle_with_subst;
///
/// let result = apply_mangle_with_subst(
///     "s/@PACKAGE@/bar/",
///     "foo baz foo",
///     || "foo".to_string(),
///     || String::new()
/// ).unwrap();
/// assert_eq!(result, "bar baz foo");
/// ```
pub fn apply_mangle_with_subst(
    vm: &str,
    orig: &str,
    package: impl FnOnce() -> String,
    component: impl FnOnce() -> String,
) -> Result<String, MangleError> {
    // Apply template substitution to the mangle expression
    let substituted_vm = crate::subst::subst(vm, package, component);

    // Apply the mangle expression
    apply_mangle(&substituted_vm, orig)
}

/// Apply character-by-character translation
fn apply_translation(pattern: &str, replacement: &str, orig: &str) -> Result<String, MangleError> {
    // Expand ranges like a-z
    let from_chars = expand_char_range(pattern);
    let to_chars = expand_char_range(replacement);

    if from_chars.len() != to_chars.len() {
        return Err(MangleError::InvalidTranslExpr(
            "pattern and replacement must have same length".to_string(),
        ));
    }

    let mut result = String::new();
    for c in orig.chars() {
        if let Some(pos) = from_chars.iter().position(|&fc| fc == c) {
            result.push(to_chars[pos]);
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

/// Expand character ranges like a-z to actual characters
fn expand_char_range(s: &str) -> Vec<char> {
    let mut result = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if i + 2 < chars.len() && chars[i + 1] == '-' {
            // Range found
            let start = chars[i];
            let end = chars[i + 2];
            for c in (start as u32)..=(end as u32) {
                if let Some(ch) = char::from_u32(c) {
                    result.push(ch);
                }
            }
            i += 3;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_subst_expr() {
        let expr = parse_subst_expr("s/foo/bar/g").unwrap();
        assert_eq!(expr.pattern, "foo");
        assert_eq!(expr.replacement, "bar");
        assert_eq!(expr.flags.as_deref(), Some("g"));

        let expr = parse_subst_expr("s|foo|bar|").unwrap();
        assert_eq!(expr.pattern, "foo");
        assert_eq!(expr.replacement, "bar");
        assert_eq!(expr.flags, None);

        let expr = parse_subst_expr("s#a/b#c/d#").unwrap();
        assert_eq!(expr.pattern, "a/b");
        assert_eq!(expr.replacement, "c/d");
    }

    #[test]
    fn test_parse_transl_expr() {
        let expr = parse_transl_expr("tr/a-z/A-Z/").unwrap();
        assert_eq!(expr.pattern, "a-z");
        assert_eq!(expr.replacement, "A-Z");

        let expr = parse_transl_expr("y/abc/xyz/").unwrap();
        assert_eq!(expr.pattern, "abc");
        assert_eq!(expr.replacement, "xyz");
    }

    #[test]
    fn test_apply_mangle_subst() {
        let result = apply_mangle("s/foo/bar/", "foo baz foo").unwrap();
        assert_eq!(result, "bar baz foo");

        let result = apply_mangle("s/foo/bar/g", "foo baz foo").unwrap();
        assert_eq!(result, "bar baz bar");

        // Test with regex
        let result = apply_mangle("s/[0-9]+/X/g", "a1b2c3").unwrap();
        assert_eq!(result, "aXbXcX");
    }

    #[test]
    fn test_apply_mangle_transl() {
        let result = apply_mangle("tr/a-z/A-Z/", "hello").unwrap();
        assert_eq!(result, "HELLO");

        let result = apply_mangle("y/abc/xyz/", "aabbcc").unwrap();
        assert_eq!(result, "xxyyzz");
    }

    #[test]
    fn test_expand_char_range() {
        let result = expand_char_range("a-z");
        assert_eq!(result.len(), 26);
        assert_eq!(result[0], 'a');
        assert_eq!(result[25], 'z');

        let result = expand_char_range("a-c");
        assert_eq!(result, vec!['a', 'b', 'c']);

        let result = expand_char_range("abc");
        assert_eq!(result, vec!['a', 'b', 'c']);
    }

    #[test]
    fn test_split_by_unescaped_delimiter() {
        let result = split_by_unescaped_delimiter("foo/bar/baz", '/');
        assert_eq!(result, vec!["foo", "bar", "baz"]);

        let result = split_by_unescaped_delimiter("foo\\/bar/baz", '/');
        assert_eq!(result, vec!["foo\\/bar", "baz"]);
    }

    #[test]
    fn test_real_world_examples() {
        // Example from Python code: dversionmangle=s/\+ds//
        let result = apply_mangle(r"s/\+ds//", "1.0+ds").unwrap();
        assert_eq!(result, "1.0");

        // Example: filenamemangle
        let result = apply_mangle(
            r"s/.+\/v?(\d\S+)\.tar\.gz/syncthing-gtk-$1.tar.gz/",
            "https://github.com/syncthing/syncthing-gtk/archive/v0.9.4.tar.gz",
        )
        .unwrap();
        assert_eq!(result, "syncthing-gtk-0.9.4.tar.gz");
    }

    #[test]
    fn test_apply_mangle_with_subst_package() {
        // Template substitution happens in the mangle expression, so @PACKAGE@
        // becomes "mypackage" in the pattern, then it matches against the input
        let result = apply_mangle_with_subst(
            "s/@PACKAGE@/replaced/",
            "foo mypackage bar",
            || "mypackage".to_string(),
            || String::new(),
        )
        .unwrap();
        assert_eq!(result, "foo replaced bar");
    }

    #[test]
    fn test_apply_mangle_with_subst_component() {
        // Template substitution happens in the mangle expression, so @COMPONENT@
        // becomes "upstream" in the pattern, then it matches against the input
        let result = apply_mangle_with_subst(
            "s/@COMPONENT@/replaced/g",
            "upstream foo upstream",
            || unreachable!(),
            || "upstream".to_string(),
        )
        .unwrap();
        assert_eq!(result, "replaced foo replaced");
    }

    #[test]
    fn test_apply_mangle_with_subst_filenamemangle() {
        // Example: filenamemangle with @PACKAGE@ template
        let result = apply_mangle_with_subst(
            r"s/.+\/v?(\d\S+)\.tar\.gz/@PACKAGE@-$1.tar.gz/",
            "https://github.com/example/repo/archive/v0.9.4.tar.gz",
            || "myapp".to_string(),
            || String::new(),
        )
        .unwrap();
        assert_eq!(result, "myapp-0.9.4.tar.gz");
    }

    #[test]
    fn test_apply_mangle_with_subst_no_templates() {
        // Ensure it still works when no templates are present
        let result = apply_mangle_with_subst(
            "s/foo/bar/g",
            "foo baz foo",
            || unreachable!(),
            || unreachable!(),
        )
        .unwrap();
        assert_eq!(result, "bar baz bar");
    }
}
