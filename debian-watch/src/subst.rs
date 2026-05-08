//! Variable substitution for watch file patterns
//!
//! This module provides template variable substitution for Debian watch files,
//! allowing common patterns to be expressed concisely using `@VARIABLE@` syntax.
//!
//! # Supported Template Variables
//!
//! - `@PACKAGE@` - Source package name (dynamically provided)
//! - `@COMPONENT@` - Component name for multi-component packages (empty for main)
//! - `@ANY_VERSION@` - Generic upstream version regex
//! - `@SEMANTIC_VERSION@` - Semantic versioning pattern (MAJOR.MINOR.PATCH)
//! - `@STABLE_VERSION@` - Stable version pattern (1.2.3 format, no 0.x.x)
//! - `@ARCHIVE_EXT@` - Common archive file extensions
//! - `@SIGNATURE_EXT@` - Signature file extensions
//! - `@DEB_EXT@` - Debian-specific version extensions
//!
//! # Example
//!
//! ```
//! use debian_watch::subst::subst;
//!
//! let url = "https://github.com/@PACKAGE@/releases";
//! let result = subst(url, || "mypackage".to_string(), || String::new());
//! assert_eq!(result, "https://github.com/mypackage/releases");
//! ```

const SUBSTITUTIONS: &[(&str, &str)] = &[
    // @PACKAGE@: Substituted with the source package name found in the first line
    // of the debian/changelog file. This is handled dynamically in the subst() function.

    // @ANY_VERSION@: Legal upstream version regex (capturing).
    // Matches versions like: 1.2.3, 1.0-beta, 2.0+git20210101
    ("@ANY_VERSION@", r"[-_]?(\d[\-+\.:\~\da-zA-Z]*)"),
    // @SEMANTIC_VERSION@: Semantic versioning pattern (capturing).
    // Matches MAJOR.MINOR.PATCH with optional prerelease and build metadata.
    // Examples: 1.2.3, 0.1.0, 1.0.0-alpha, 2.1.0-beta.1
    // See https://semver.org/ for full specification.
    (
        "@SEMANTIC_VERSION@",
        r"[-_]?[Vv]?((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-(?:(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?)",
    ),
    // @STABLE_VERSION@: Stable version pattern (capturing).
    // Matches pure digit versions with exactly three numbers (MAJOR.MINOR.PATCH).
    // Examples: 1.2.3, 10.20.30
    // Note: Does NOT match 0.x.x versions (requires MAJOR >= 1)
    ("@STABLE_VERSION@", r"[-_]?[Vv]?((?:[1-9]\d*)(?:\.\d+){2})"),
    // @ARCHIVE_EXT@: Typical archive file extension regex (non-capturing).
    // Matches: .tar.xz, .tar.bz2, .tar.gz, .zip, .tgz, .tbz, .txz
    (
        "@ARCHIVE_EXT@",
        r"(?i)\.(?:tar\.xz|tar\.bz2|tar\.gz|zip|tgz|tbz|txz)",
    ),
    // @SIGNATURE_EXT@: Signature file extension regex (non-capturing).
    // Matches archive extensions followed by signature extensions
    // Examples: .tar.gz.asc, .tar.xz.sig, .zip.gpg
    (
        "@SIGNATURE_EXT@",
        r"(?i)\.(?:tar\.xz|tar\.bz2|tar\.gz|zip|tgz|tbz|txz)\.(?:asc|pgp|gpg|sig|sign)",
    ),
    // @DEB_EXT@: Debian extension pattern (capturing).
    // Matches Debian-specific version suffixes like +debian, ~dfsg, +ds1
    ("@DEB_EXT@", r"[\+~](debian|dfsg|ds|deb)(\.)?(\d+)?$"),
];

/// Substitute watch file variables like @PACKAGE@, @COMPONENT@, and @ANY_VERSION@
///
/// # Arguments
/// * `text` - The text containing template variables to substitute
/// * `package` - Closure that returns the package name for @PACKAGE@ substitution
/// * `component` - Closure that returns the component name for @COMPONENT@ substitution
///               (returns empty string for main paragraph)
pub fn subst(
    text: &str,
    package: impl FnOnce() -> String,
    component: impl FnOnce() -> String,
) -> String {
    // Early return if no substitutions are needed
    if !text.contains('@') {
        return text.to_string();
    }

    // Apply all substitutions in a single pass using fold
    let result = SUBSTITUTIONS
        .iter()
        .fold(text.to_string(), |acc, (pattern, replacement)| {
            acc.replace(pattern, replacement)
        });

    // Handle @PACKAGE@ substitution if needed
    let result = if result.contains("@PACKAGE@") {
        let package_name = package();
        result.replace("@PACKAGE@", &package_name)
    } else {
        result
    };

    // Handle @COMPONENT@ substitution if needed
    if result.contains("@COMPONENT@") {
        let component_name = component();
        result.replace("@COMPONENT@", &component_name)
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subst_any_version() {
        assert_eq!(
            subst("@ANY_VERSION@", || unreachable!(), || unreachable!()),
            r"[-_]?(\d[\-+\.:\~\da-zA-Z]*)"
        );
    }

    #[test]
    fn test_subst_package() {
        assert_eq!(
            subst("@PACKAGE@", || "foo".to_string(), || unreachable!()),
            "foo"
        );

        // Test in a URL pattern
        assert_eq!(
            subst(
                "https://github.com/@PACKAGE@/releases",
                || "mypackage".to_string(),
                || unreachable!()
            ),
            "https://github.com/mypackage/releases"
        );
    }

    #[test]
    fn test_subst_component() {
        assert_eq!(
            subst("@COMPONENT@", || unreachable!(), || "bar".to_string()),
            "bar"
        );

        // Test with empty component (main paragraph)
        assert_eq!(
            subst("@COMPONENT@", || unreachable!(), || String::new()),
            ""
        );

        // Test in a pattern
        assert_eq!(
            subst(
                "https://example.com/@COMPONENT@/files",
                || unreachable!(),
                || "upstream".to_string()
            ),
            "https://example.com/upstream/files"
        );
    }

    #[test]
    fn test_subst_semantic_version() {
        let pattern = subst("@SEMANTIC_VERSION@", || unreachable!(), || unreachable!());
        assert_eq!(
            pattern,
            r"[-_]?[Vv]?((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-(?:(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?)"
        );

        // Verify the pattern works with regex
        let re = regex::Regex::new(&pattern).unwrap();
        assert!(re.is_match("1.2.3"));
        assert!(re.is_match("v1.2.3"));
        assert!(re.is_match("0.0.0"));
        assert!(re.is_match("1.2.3-alpha"));
        assert!(re.is_match("1.2.3-alpha.1"));
    }

    #[test]
    fn test_subst_stable_version() {
        let pattern = subst("@STABLE_VERSION@", || unreachable!(), || unreachable!());
        assert_eq!(pattern, r"[-_]?[Vv]?((?:[1-9]\d*)(?:\.\d+){2})");

        // Verify the pattern works with regex
        let re = regex::Regex::new(&pattern).unwrap();
        assert!(re.is_match("1.2.3"));
        assert!(re.is_match("v1.2.3"));
        assert!(re.is_match("10.20.30"));
        // Stable version shouldn't match 0.x.x
        assert!(!re.is_match("0.2.3"));
    }

    #[test]
    fn test_subst_archive_ext() {
        let pattern = subst("@ARCHIVE_EXT@", || unreachable!(), || unreachable!());
        assert_eq!(
            pattern,
            r"(?i)\.(?:tar\.xz|tar\.bz2|tar\.gz|zip|tgz|tbz|txz)"
        );
    }

    #[test]
    fn test_subst_signature_ext() {
        let pattern = subst("@SIGNATURE_EXT@", || unreachable!(), || unreachable!());
        assert_eq!(
            pattern,
            r"(?i)\.(?:tar\.xz|tar\.bz2|tar\.gz|zip|tgz|tbz|txz)\.(?:asc|pgp|gpg|sig|sign)"
        );
    }

    #[test]
    fn test_subst_deb_ext() {
        let pattern = subst("@DEB_EXT@", || unreachable!(), || unreachable!());
        assert_eq!(pattern, r"[\+~](debian|dfsg|ds|deb)(\.)?(\d+)?$");
    }

    #[test]
    fn test_subst_multiple_templates() {
        assert_eq!(
            subst(
                "https://github.com/@PACKAGE@/releases/@COMPONENT@/file@ARCHIVE_EXT@",
                || "myapp".to_string(),
                || "core".to_string(),
            ),
            "https://github.com/myapp/releases/core/file(?i)\\.(?:tar\\.xz|tar\\.bz2|tar\\.gz|zip|tgz|tbz|txz)"
        );
    }

    #[test]
    fn test_subst_no_templates() {
        // Test early return optimization when no @ present
        assert_eq!(
            subst(
                "https://example.com/releases",
                || unreachable!(),
                || unreachable!(),
            ),
            "https://example.com/releases"
        );
    }
}
