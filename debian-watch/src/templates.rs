//! Template expansion for v5 watch files
//!
//! This module provides template expansion for common project hosting platforms,
//! simplifying watch file creation by auto-generating Source URLs, matching patterns,
//! and other configuration based on template type.
//!
//! # Supported Templates
//!
//! - `GitHub` - For GitHub-hosted projects
//! - `GitLab` - For GitLab instances
//! - `PyPI` - For Python packages on PyPI
//! - `Npmregistry` - For npm packages
//! - `Metacpan` - For Perl modules on MetaCPAN

/// Error type for template expansion
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// Unknown template type
    UnknownTemplate(String),
    /// Missing required field
    MissingField {
        /// Template type
        template: String,
        /// Field name
        field: String,
    },
    /// Invalid field value
    InvalidValue {
        /// Field name
        field: String,
        /// Reason for invalidity
        reason: String,
    },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TemplateError::UnknownTemplate(t) => write!(f, "Unknown template type: {}", t),
            TemplateError::MissingField { template, field } => {
                write!(f, "{} template requires '{}' field", template, field)
            }
            TemplateError::InvalidValue { field, reason } => {
                write!(f, "Invalid value for '{}': {}", field, reason)
            }
        }
    }
}

impl std::error::Error for TemplateError {}

/// Template with variant-specific parameters
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Template {
    /// GitHub template
    GitHub {
        /// Project owner
        owner: String,
        /// Project repository name
        repository: String,
        /// Search only releases (not all tags)
        release_only: bool,
        /// Version type pattern to use
        version_type: Option<String>,
    },
    /// GitLab template
    GitLab {
        /// Project URL
        dist: String,
        /// Search only releases (not all tags)
        release_only: bool,
        /// Version type pattern to use
        version_type: Option<String>,
    },
    /// PyPI template
    PyPI {
        /// Package name
        package: String,
        /// Version type pattern to use
        version_type: Option<String>,
    },
    /// npm registry template
    Npmregistry {
        /// Package name (may include @scope/)
        package: String,
        /// Version type pattern to use
        version_type: Option<String>,
    },
    /// MetaCPAN template
    Metacpan {
        /// Distribution name (using :: or -)
        dist: String,
        /// Version type pattern to use
        version_type: Option<String>,
    },
    /// CRAN template
    Cran {
        /// Package name
        package: String,
        /// Version type pattern to use
        version_type: Option<String>,
    },
    /// Bioconductor template
    Bioconductor {
        /// Package name
        package: String,
        /// Version type pattern to use
        version_type: Option<String>,
    },
}

/// Expanded template fields
#[derive(Debug, Clone, Default)]
pub struct ExpandedTemplate {
    /// Source URL
    pub source: Option<String>,
    /// Matching pattern
    pub matching_pattern: Option<String>,
    /// Search mode
    pub searchmode: Option<String>,
    /// Mode
    pub mode: Option<String>,
    /// PGP mode
    pub pgpmode: Option<String>,
    /// Download URL mangle
    pub downloadurlmangle: Option<String>,
}

/// Expand a template into field values
pub fn expand_template(template: Template) -> ExpandedTemplate {
    match template {
        Template::GitHub {
            owner,
            repository,
            release_only,
            version_type,
        } => expand_github_template(owner, repository, release_only, version_type),
        Template::GitLab {
            dist,
            release_only,
            version_type,
        } => expand_gitlab_template(dist, release_only, version_type),
        Template::PyPI {
            package,
            version_type,
        } => expand_pypi_template(package, version_type),
        Template::Npmregistry {
            package,
            version_type,
        } => expand_npmregistry_template(package, version_type),
        Template::Metacpan { dist, version_type } => expand_metacpan_template(dist, version_type),
        Template::Cran {
            package,
            version_type,
        } => expand_cran_template(package, version_type),
        Template::Bioconductor {
            package,
            version_type,
        } => expand_bioconductor_template(package, version_type),
    }
}

/// Expand GitHub template
fn expand_github_template(
    owner: String,
    repository: String,
    release_only: bool,
    version_type: Option<String>,
) -> ExpandedTemplate {
    let version_pattern = version_type
        .as_deref()
        .map(|v| format!("@{}_VERSION@", v.to_uppercase()))
        .unwrap_or_else(|| "@ANY_VERSION@".to_string());

    let source = if release_only {
        format!("https://github.com/{}/{}/releases", owner, repository)
    } else {
        format!("https://github.com/{}/{}/tags", owner, repository)
    };

    let matching_pattern = format!(
        r".*/(?:refs/tags/)?v?{}{}",
        version_pattern, "@ARCHIVE_EXT@"
    );

    ExpandedTemplate {
        source: Some(source),
        matching_pattern: Some(matching_pattern),
        searchmode: Some("html".to_string()),
        ..Default::default()
    }
}

/// Expand GitLab template
fn expand_gitlab_template(
    dist: String,
    _release_only: bool,
    version_type: Option<String>,
) -> ExpandedTemplate {
    let version_pattern = version_type
        .as_deref()
        .map(|v| format!("@{}_VERSION@", v.to_uppercase()))
        .unwrap_or_else(|| "@ANY_VERSION@".to_string());

    // GitLab uses mode=gitlab
    ExpandedTemplate {
        source: Some(dist),
        matching_pattern: Some(format!(r".*/v?{}{}", version_pattern, "@ARCHIVE_EXT@")),
        mode: Some("gitlab".to_string()),
        ..Default::default()
    }
}

/// Expand PyPI template
fn expand_pypi_template(package: String, version_type: Option<String>) -> ExpandedTemplate {
    let version_pattern = version_type
        .as_deref()
        .map(|v| format!("@{}_VERSION@", v.to_uppercase()))
        .unwrap_or_else(|| "@ANY_VERSION@".to_string());

    ExpandedTemplate {
        source: Some(format!("https://pypi.debian.net/{}/", package)),
        matching_pattern: Some(format!(
            r"https://pypi\.debian\.net/{}/[^/]+\.tar\.gz#/.*-{}\.tar\.gz",
            package, version_pattern
        )),
        searchmode: Some("plain".to_string()),
        ..Default::default()
    }
}

/// Expand Npmregistry template
fn expand_npmregistry_template(package: String, version_type: Option<String>) -> ExpandedTemplate {
    let version_pattern = version_type
        .as_deref()
        .map(|v| format!("@{}_VERSION@", v.to_uppercase()))
        .unwrap_or_else(|| "@ANY_VERSION@".to_string());

    // npm package names might have @ prefix for scoped packages
    let package_name = package.trim_start_matches('@');

    ExpandedTemplate {
        source: Some(format!("https://registry.npmjs.org/{}", package)),
        matching_pattern: Some(format!(
            r"https://registry\.npmjs\.org/{}/-/.*-{}@ARCHIVE_EXT@",
            package_name.replace('/', r"\/"),
            version_pattern
        )),
        searchmode: Some("plain".to_string()),
        ..Default::default()
    }
}

/// Expand Metacpan template
fn expand_metacpan_template(dist: String, version_type: Option<String>) -> ExpandedTemplate {
    let version_pattern = version_type
        .as_deref()
        .map(|v| format!("@{}_VERSION@", v.to_uppercase()))
        .unwrap_or_else(|| "@ANY_VERSION@".to_string());

    // MetaCPAN dist names can use :: or -
    let dist_name = dist.replace("::", "-");

    ExpandedTemplate {
        source: Some("https://cpan.metacpan.org/authors/id/".to_string()),
        matching_pattern: Some(format!(r".*/{}{}@ARCHIVE_EXT@", dist_name, version_pattern)),
        searchmode: Some("plain".to_string()),
        ..Default::default()
    }
}

/// Expand CRAN template
fn expand_cran_template(package: String, version_type: Option<String>) -> ExpandedTemplate {
    let version_pattern = version_type
        .as_deref()
        .map(|v| format!("@{}_VERSION@", v.to_uppercase()))
        .unwrap_or_else(|| "@ANY_VERSION@".to_string());

    ExpandedTemplate {
        source: Some(format!("https://cran.r-project.org/package={}", package)),
        matching_pattern: Some(format!(".*_{}.tar.gz", version_pattern)),
        downloadurlmangle: Some(
            "s%.*/src/contrib/%https://cran.r-project.org/src/contrib/%".to_string(),
        ),
        ..Default::default()
    }
}

/// Expand Bioconductor template
fn expand_bioconductor_template(package: String, version_type: Option<String>) -> ExpandedTemplate {
    let version_pattern = version_type
        .as_deref()
        .map(|v| format!("@{}_VERSION@", v.to_uppercase()))
        .unwrap_or_else(|| "@ANY_VERSION@".to_string());

    ExpandedTemplate {
        source: Some(format!("https://bioconductor.org/packages/{}", package)),
        matching_pattern: Some(format!(".*_{}.tar.gz", version_pattern)),
        downloadurlmangle: Some(
            "s%.*/src/contrib/%https://bioconductor.org/packages/release/bioc/src/contrib/%"
                .to_string(),
        ),
        ..Default::default()
    }
}

/// Try to detect if the given fields match a known template pattern
/// and return the corresponding Template if a match is found.
///
/// This is the reverse of `expand_template` - it analyzes expanded fields
/// and tries to identify which template would produce them.
///
/// # Arguments
///
/// * `source` - The Source URL
/// * `matching_pattern` - The Matching-Pattern
/// * `searchmode` - The Searchmode field (if any)
/// * `mode` - The Mode field (if any)
///
/// # Returns
///
/// Returns `Some(Template)` if the fields match a known template pattern,
/// `None` otherwise.
pub fn detect_template(
    source: Option<&str>,
    matching_pattern: Option<&str>,
    searchmode: Option<&str>,
    mode: Option<&str>,
) -> Option<Template> {
    let source = source?;

    // Try GitHub template detection
    if let Some(template) = detect_github_template(source, matching_pattern, searchmode) {
        return Some(template);
    }

    // Try GitLab template detection
    if let Some(template) = detect_gitlab_template(source, matching_pattern, mode) {
        return Some(template);
    }

    // Try PyPI template detection
    if let Some(template) = detect_pypi_template(source, matching_pattern, searchmode) {
        return Some(template);
    }

    // Try Npmregistry template detection
    if let Some(template) = detect_npmregistry_template(source, matching_pattern, searchmode) {
        return Some(template);
    }

    // Try Metacpan template detection
    if let Some(template) = detect_metacpan_template(source, matching_pattern, searchmode) {
        return Some(template);
    }

    // Try CRAN template detection
    if let Some(template) = detect_cran_template(source, matching_pattern) {
        return Some(template);
    }

    // Try CRAN detection from old-style source URL (e.g. converted from v4 where
    // URL and pattern were combined, or from explicit Source/Matching-Pattern)
    if let Some(template) = detect_cran_from_source_url(source, matching_pattern) {
        return Some(template);
    }

    // Try CRAN detection from source URL with embedded pattern (no separate
    // Matching-Pattern field, common after v4 → v5 conversion)
    if matching_pattern.is_none() {
        if let Some(template) = detect_cran_from_inline_url(source) {
            return Some(template);
        }
        if let Some(template) = detect_bioconductor_from_inline_url(source) {
            return Some(template);
        }
    }

    // Try Bioconductor template detection
    if let Some(template) = detect_bioconductor_template(source, matching_pattern) {
        return Some(template);
    }

    None
}

/// Detect GitHub template
fn detect_github_template(
    source: &str,
    matching_pattern: Option<&str>,
    searchmode: Option<&str>,
) -> Option<Template> {
    // Check searchmode is html
    if searchmode != Some("html") && searchmode.is_some() {
        return None;
    }

    // Parse source URL to extract owner and repository
    let release_only = if source.ends_with("/releases") {
        true
    } else if source.ends_with("/tags") {
        false
    } else {
        return None;
    };

    // Extract owner/repo from URL
    let url_without_suffix = if release_only {
        source.strip_suffix("/releases")?
    } else {
        source.strip_suffix("/tags")?
    };

    let (owner, repository) = if let Ok(parsed) = url::Url::parse(url_without_suffix) {
        if parsed.host_str() != Some("github.com") {
            return None;
        }
        let path = parsed.path().trim_start_matches('/').trim_end_matches('/');
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() != 2 {
            return None;
        }
        (parts[0].to_string(), parts[1].to_string())
    } else {
        return None;
    };

    // Try to detect version_type from matching pattern
    let version_type = if let Some(pattern) = matching_pattern {
        extract_version_type(pattern)
    } else {
        None
    };

    Some(Template::GitHub {
        owner,
        repository,
        release_only,
        version_type,
    })
}

/// Detect GitLab template
fn detect_gitlab_template(
    source: &str,
    matching_pattern: Option<&str>,
    mode: Option<&str>,
) -> Option<Template> {
    // Check mode is gitlab
    if mode != Some("gitlab") {
        return None;
    }

    // Try to detect version_type from matching pattern
    let version_type = if let Some(pattern) = matching_pattern {
        extract_version_type(pattern)
    } else {
        None
    };

    Some(Template::GitLab {
        dist: source.to_string(),
        release_only: false, // GitLab template doesn't use release_only
        version_type,
    })
}

/// Detect PyPI template
fn detect_pypi_template(
    source: &str,
    matching_pattern: Option<&str>,
    searchmode: Option<&str>,
) -> Option<Template> {
    // Check searchmode is plain
    if searchmode != Some("plain") && searchmode.is_some() {
        return None;
    }

    // Check if source matches PyPI pattern
    if !source.starts_with("https://pypi.debian.net/") {
        return None;
    }

    let remainder = source
        .strip_prefix("https://pypi.debian.net/")?
        .trim_end_matches('/');

    // Extract only the package name (first path component)
    let package = match remainder.split_once('/') {
        Some((pkg, _)) => pkg,
        None => remainder,
    };

    // Try to detect version_type from matching pattern
    let version_type = if let Some(pattern) = matching_pattern {
        extract_version_type(pattern)
    } else {
        None
    };

    Some(Template::PyPI {
        package: package.to_string(),
        version_type,
    })
}

/// Detect Npmregistry template
fn detect_npmregistry_template(
    source: &str,
    matching_pattern: Option<&str>,
    searchmode: Option<&str>,
) -> Option<Template> {
    // Check searchmode is plain
    if searchmode != Some("plain") && searchmode.is_some() {
        return None;
    }

    // Check if source matches npm registry pattern
    if !source.starts_with("https://registry.npmjs.org/") {
        return None;
    }

    let package = source.strip_prefix("https://registry.npmjs.org/")?;

    // Try to detect version_type from matching pattern
    let version_type = if let Some(pattern) = matching_pattern {
        extract_version_type(pattern)
    } else {
        None
    };

    Some(Template::Npmregistry {
        package: package.to_string(),
        version_type,
    })
}

/// Detect Metacpan template
fn detect_metacpan_template(
    source: &str,
    matching_pattern: Option<&str>,
    searchmode: Option<&str>,
) -> Option<Template> {
    // Check searchmode is plain
    if searchmode != Some("plain") && searchmode.is_some() {
        return None;
    }

    if source == "https://cpan.metacpan.org/authors/id/" {
        // Extract dist from matching pattern
        let pattern = matching_pattern?;

        // Pattern should be like: .*/DIST-NAME@VERSION@@ARCHIVE_EXT@
        // We need to extract DIST-NAME
        if !pattern.starts_with(".*/") {
            return None;
        }

        let after_prefix = pattern.strip_prefix(".*/").unwrap();

        // Find where the version pattern starts
        let version_type = extract_version_type(pattern);

        // Extract dist name - everything before the version pattern
        // Strip optional trailing `-v?` or `-` before the version placeholder
        let dist = if let Some(idx) = after_prefix.find('@') {
            after_prefix[..idx]
                .trim_end_matches("-v?")
                .trim_end_matches('-')
        } else {
            return None;
        };

        Some(Template::Metacpan {
            dist: dist.to_string(),
            version_type,
        })
    } else if let Some(dist) = source
        .strip_prefix("https://metacpan.org/release/")
        .or_else(|| source.strip_prefix("https://metacpan.org/dist/"))
    {
        let dist = dist.trim_end_matches('/');
        if dist.is_empty() {
            return None;
        }

        let version_type = matching_pattern.and_then(extract_version_type);

        Some(Template::Metacpan {
            dist: dist.to_string(),
            version_type,
        })
    } else {
        None
    }
}

/// Detect CRAN template
fn detect_cran_template(source: &str, matching_pattern: Option<&str>) -> Option<Template> {
    // Check if source matches CRAN package URL pattern
    let package = source.strip_prefix("https://cran.r-project.org/package=")?;

    if package.is_empty() {
        return None;
    }

    let version_type = matching_pattern.and_then(extract_version_type);

    Some(Template::Cran {
        package: package.to_string(),
        version_type,
    })
}

/// Detect CRAN template from old-style source URL (pre-template format)
///
/// Matches URLs like `https://cran.r-project.org/src/contrib/` or
/// `https://cloud.r-project.org/src/contrib/` with a matching pattern
/// that contains the package name.
pub fn detect_cran_from_source_url(
    source: &str,
    matching_pattern: Option<&str>,
) -> Option<Template> {
    // Check for CRAN contrib URLs (both cran.r-project.org and cloud.r-project.org)
    if source != "https://cran.r-project.org/src/contrib/"
        && source != "https://cloud.r-project.org/src/contrib/"
    {
        return None;
    }

    // Extract package name from matching pattern like "forecast_([-.\d]*)\.tar\.gz"
    let pattern = matching_pattern?;
    let package = pattern.split('_').next()?;
    if package.is_empty() {
        return None;
    }

    let version_type = extract_version_type(pattern);

    Some(Template::Cran {
        package: package.to_string(),
        version_type,
    })
}

/// Detect CRAN template from an inline URL where the source URL contains both
/// the base URL and the matching pattern (common after v4 → v5 conversion).
///
/// Matches URLs like `https://cran.r-project.org/src/contrib/forecast_([-\d.]*)\\.tar\\.gz`
/// or `https://cloud.r-project.org/src/contrib/forecast_([-\d.]*)\\.tar\\.gz`
fn detect_cran_from_inline_url(source: &str) -> Option<Template> {
    let remainder = source
        .strip_prefix("https://cran.r-project.org/src/contrib/")
        .or_else(|| source.strip_prefix("https://cloud.r-project.org/src/contrib/"))?;

    // Extract package name: everything before the first underscore
    let package = remainder.split('_').next()?;
    if package.is_empty() {
        return None;
    }

    Some(Template::Cran {
        package: package.to_string(),
        version_type: None,
    })
}

/// Detect Bioconductor template from an inline URL where the source URL contains both
/// the base URL and the matching pattern.
fn detect_bioconductor_from_inline_url(source: &str) -> Option<Template> {
    let remainder =
        source.strip_prefix("https://bioconductor.org/packages/release/bioc/src/contrib/")?;

    let package = remainder.split('_').next()?;
    if package.is_empty() {
        return None;
    }

    Some(Template::Bioconductor {
        package: package.to_string(),
        version_type: None,
    })
}

/// Detect Bioconductor template
fn detect_bioconductor_template(source: &str, matching_pattern: Option<&str>) -> Option<Template> {
    // Check if source matches Bioconductor package URL pattern
    let package = source.strip_prefix("https://bioconductor.org/packages/")?;

    if package.is_empty() {
        return None;
    }

    let version_type = matching_pattern.and_then(extract_version_type);

    Some(Template::Bioconductor {
        package: package.to_string(),
        version_type,
    })
}

/// Extract version type from a matching pattern
/// Returns None for @ANY_VERSION@, Some(type) for specific types
fn extract_version_type(pattern: &str) -> Option<String> {
    // Look for @XXXXX_VERSION@ or @ANY_VERSION@ patterns
    if pattern.contains("@ANY_VERSION@") {
        None
    } else if let Some(start) = pattern.find('@') {
        if let Some(end) = pattern[start + 1..].find('@') {
            let version_str = &pattern[start + 1..start + 1 + end];
            if version_str.ends_with("_VERSION") {
                let type_str = version_str.strip_suffix("_VERSION")?;
                Some(type_str.to_lowercase())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    }
}

/// Parse GitHub URL or owner/repository to extract owner and repository
pub fn parse_github_url(url: &str) -> Result<(String, String), TemplateError> {
    let url = url.trim_end_matches('/');

    // Try to parse as URL
    if let Ok(parsed) = url::Url::parse(url) {
        if parsed.host_str() == Some("github.com") {
            let path = parsed.path().trim_start_matches('/').trim_end_matches('/');
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 2 {
                return Ok((parts[0].to_string(), parts[1].to_string()));
            }
        }
    }

    // Try to parse as owner/repository
    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() == 2 {
        return Ok((parts[0].to_string(), parts[1].to_string()));
    }

    Err(TemplateError::InvalidValue {
        field: "Dist".to_string(),
        reason: format!("Could not parse GitHub URL: {}", url),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_template_with_owner_repository() {
        let template = Template::GitHub {
            owner: "torvalds".to_string(),
            repository: "linux".to_string(),
            release_only: false,
            version_type: None,
        };

        let result = expand_template(template);
        assert_eq!(
            result.source,
            Some("https://github.com/torvalds/linux/tags".to_string())
        );
        assert_eq!(
            result.matching_pattern,
            Some(r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@".to_string())
        );
    }

    #[test]
    fn test_github_template_release_only() {
        let template = Template::GitHub {
            owner: "test".to_string(),
            repository: "project".to_string(),
            release_only: true,
            version_type: None,
        };

        let result = expand_template(template);
        assert_eq!(
            result.source,
            Some("https://github.com/test/project/releases".to_string())
        );
    }

    #[test]
    fn test_parse_github_url() {
        let (owner, repo) = parse_github_url("https://github.com/guimard/llng-docker").unwrap();
        assert_eq!(owner, "guimard");
        assert_eq!(repo, "llng-docker");

        let (owner, repo) = parse_github_url("torvalds/linux").unwrap();
        assert_eq!(owner, "torvalds");
        assert_eq!(repo, "linux");
    }

    #[test]
    fn test_pypi_template() {
        let template = Template::PyPI {
            package: "bitbox02".to_string(),
            version_type: None,
        };

        let result = expand_template(template);
        assert_eq!(
            result.source,
            Some("https://pypi.debian.net/bitbox02/".to_string())
        );
        assert_eq!(result.searchmode, Some("plain".to_string()));
    }

    #[test]
    fn test_npmregistry_template() {
        let template = Template::Npmregistry {
            package: "@lemonldapng/handler".to_string(),
            version_type: None,
        };

        let result = expand_template(template);
        assert_eq!(
            result.source,
            Some("https://registry.npmjs.org/@lemonldapng/handler".to_string())
        );
        assert_eq!(result.searchmode, Some("plain".to_string()));
    }

    #[test]
    fn test_gitlab_template() {
        let template = Template::GitLab {
            dist: "https://salsa.debian.org/debian/devscripts".to_string(),
            release_only: false,
            version_type: None,
        };

        let result = expand_template(template);
        assert_eq!(
            result.source,
            Some("https://salsa.debian.org/debian/devscripts".to_string())
        );
        assert_eq!(result.mode, Some("gitlab".to_string()));
    }

    #[test]
    fn test_metacpan_template() {
        let template = Template::Metacpan {
            dist: "MetaCPAN-Client".to_string(),
            version_type: None,
        };

        let result = expand_template(template);
        assert_eq!(
            result.source,
            Some("https://cpan.metacpan.org/authors/id/".to_string())
        );
    }

    #[test]
    fn test_detect_github_template() {
        let template = detect_template(
            Some("https://github.com/torvalds/linux/tags"),
            Some(r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@"),
            Some("html"),
            None,
        );

        assert_eq!(
            template,
            Some(Template::GitHub {
                owner: "torvalds".to_string(),
                repository: "linux".to_string(),
                release_only: false,
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_github_template_releases() {
        let template = detect_template(
            Some("https://github.com/test/project/releases"),
            Some(r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@"),
            Some("html"),
            None,
        );

        assert_eq!(
            template,
            Some(Template::GitHub {
                owner: "test".to_string(),
                repository: "project".to_string(),
                release_only: true,
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_github_template_with_version_type() {
        let template = detect_template(
            Some("https://github.com/foo/bar/tags"),
            Some(r".*/(?:refs/tags/)?v?@SEMANTIC_VERSION@@ARCHIVE_EXT@"),
            Some("html"),
            None,
        );

        assert_eq!(
            template,
            Some(Template::GitHub {
                owner: "foo".to_string(),
                repository: "bar".to_string(),
                release_only: false,
                version_type: Some("semantic".to_string()),
            })
        );
    }

    #[test]
    fn test_detect_pypi_template() {
        let template = detect_template(
            Some("https://pypi.debian.net/bitbox02/"),
            Some(r"https://pypi\.debian\.net/bitbox02/[^/]+\.tar\.gz#/.*-@ANY_VERSION@\.tar\.gz"),
            Some("plain"),
            None,
        );

        assert_eq!(
            template,
            Some(Template::PyPI {
                package: "bitbox02".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_gitlab_template() {
        let template = detect_template(
            Some("https://salsa.debian.org/debian/devscripts"),
            Some(r".*/v?@ANY_VERSION@@ARCHIVE_EXT@"),
            None,
            Some("gitlab"),
        );

        assert_eq!(
            template,
            Some(Template::GitLab {
                dist: "https://salsa.debian.org/debian/devscripts".to_string(),
                release_only: false,
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_npmregistry_template() {
        let template = detect_template(
            Some("https://registry.npmjs.org/@lemonldapng/handler"),
            Some(
                r"https://registry\.npmjs\.org/lemonldapng/handler/-/.*-@ANY_VERSION@@ARCHIVE_EXT@",
            ),
            Some("plain"),
            None,
        );

        assert_eq!(
            template,
            Some(Template::Npmregistry {
                package: "@lemonldapng/handler".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_metacpan_template() {
        let template = detect_template(
            Some("https://cpan.metacpan.org/authors/id/"),
            Some(r".*/MetaCPAN-Client@ANY_VERSION@@ARCHIVE_EXT@"),
            Some("plain"),
            None,
        );

        assert_eq!(
            template,
            Some(Template::Metacpan {
                dist: "MetaCPAN-Client".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_no_template() {
        // Non-template source
        let template = detect_template(
            Some("https://example.com/downloads/"),
            Some(r".*/v?(\d+\.\d+)\.tar\.gz"),
            Some("html"),
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_roundtrip_github_template() {
        // Expand template
        let original = Template::GitHub {
            owner: "torvalds".to_string(),
            repository: "linux".to_string(),
            release_only: false,
            version_type: None,
        };
        let expanded = expand_template(original.clone());

        // Detect template from expanded fields
        let detected = detect_template(
            expanded.source.as_deref(),
            expanded.matching_pattern.as_deref(),
            expanded.searchmode.as_deref(),
            expanded.mode.as_deref(),
        );

        assert_eq!(detected, Some(original));
    }

    #[test]
    fn test_extract_version_type() {
        assert_eq!(extract_version_type("@ANY_VERSION@"), None);
        assert_eq!(
            extract_version_type("@SEMANTIC_VERSION@"),
            Some("semantic".to_string())
        );
        assert_eq!(
            extract_version_type("@STABLE_VERSION@"),
            Some("stable".to_string())
        );
        assert_eq!(extract_version_type("no-template-here"), None);
    }

    #[test]
    fn test_detect_github_wrong_searchmode() {
        // GitHub template requires searchmode=html or None
        let template = detect_template(
            Some("https://github.com/torvalds/linux/tags"),
            Some(r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@"),
            Some("plain"), // Wrong searchmode
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_github_invalid_url() {
        // URL doesn't end with /tags or /releases
        let template = detect_template(
            Some("https://github.com/torvalds/linux"),
            Some(r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@"),
            Some("html"),
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_github_wrong_host() {
        // Not github.com
        let template = detect_template(
            Some("https://gitlab.com/foo/bar/tags"),
            Some(r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@"),
            Some("html"),
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_gitlab_without_mode() {
        // GitLab template requires mode=gitlab
        let template = detect_template(
            Some("https://salsa.debian.org/debian/devscripts"),
            Some(r".*/v?@ANY_VERSION@@ARCHIVE_EXT@"),
            None,
            None, // Missing mode=gitlab
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_pypi_source_with_inline_pattern() {
        // When the Source URL contains the matching pattern inline
        // (e.g., from old-style watch files), only the package name should be extracted
        let template = detect_template(
            Some("https://pypi.debian.net/dulwich/dulwich-(.*).tar.gz"),
            None,
            None,
            None,
        );

        assert_eq!(
            template,
            Some(Template::PyPI {
                package: "dulwich".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_pypi_wrong_searchmode() {
        let template = detect_template(
            Some("https://pypi.debian.net/bitbox02/"),
            Some(r"https://pypi\.debian\.net/bitbox02/[^/]+\.tar\.gz#/.*-@ANY_VERSION@\.tar\.gz"),
            Some("html"), // Wrong searchmode
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_pypi_wrong_url() {
        let template = detect_template(
            Some("https://pypi.org/bitbox02/"), // Wrong domain
            Some(r"https://pypi\.debian\.net/bitbox02/[^/]+\.tar\.gz#/.*-@ANY_VERSION@\.tar\.gz"),
            Some("plain"),
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_npmregistry_wrong_url() {
        let template = detect_template(
            Some("https://npm.example.com/@lemonldapng/handler"), // Wrong domain
            Some(
                r"https://registry\.npmjs\.org/lemonldapng/handler/-/.*-@ANY_VERSION@@ARCHIVE_EXT@",
            ),
            Some("plain"),
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_metacpan_wrong_source() {
        let template = detect_template(
            Some("https://cpan.example.com/authors/id/"), // Wrong URL
            Some(r".*/MetaCPAN-Client@ANY_VERSION@@ARCHIVE_EXT@"),
            Some("plain"),
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_metacpan_missing_pattern() {
        let template = detect_template(
            Some("https://cpan.metacpan.org/authors/id/"),
            None, // Missing pattern needed to extract dist
            Some("plain"),
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_metacpan_release_url() {
        let template = detect_template(
            Some("https://metacpan.org/release/Time-ParseDate"),
            Some(r".*/Time-ParseDate-v?@ANY_VERSION@@ARCHIVE_EXT@$"),
            None,
            None,
        );

        assert_eq!(
            template,
            Some(Template::Metacpan {
                dist: "Time-ParseDate".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_metacpan_dist_url() {
        let template = detect_template(
            Some("https://metacpan.org/dist/Mail-AuthenticationResults"),
            Some(r".*/Mail-AuthenticationResults-v?@ANY_VERSION@@ARCHIVE_EXT@$"),
            None,
            None,
        );

        assert_eq!(
            template,
            Some(Template::Metacpan {
                dist: "Mail-AuthenticationResults".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_metacpan_cpan_url_with_v_prefix() {
        // Pattern with `-v?` before version placeholder (common in the wild)
        let template = detect_template(
            Some("https://cpan.metacpan.org/authors/id/"),
            Some(r".*/Time-ParseDate-v?@ANY_VERSION@@ARCHIVE_EXT@"),
            Some("plain"),
            None,
        );

        assert_eq!(
            template,
            Some(Template::Metacpan {
                dist: "Time-ParseDate".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_metacpan_release_url_wrong_domain() {
        let template = detect_template(
            Some("https://example.org/release/Time-ParseDate"),
            Some(r".*/Time-ParseDate-v?@ANY_VERSION@@ARCHIVE_EXT@$"),
            None,
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_roundtrip_gitlab_template() {
        let original = Template::GitLab {
            dist: "https://salsa.debian.org/debian/devscripts".to_string(),
            release_only: false,
            version_type: None,
        };
        let expanded = expand_template(original.clone());

        let detected = detect_template(
            expanded.source.as_deref(),
            expanded.matching_pattern.as_deref(),
            expanded.searchmode.as_deref(),
            expanded.mode.as_deref(),
        );

        assert_eq!(detected, Some(original));
    }

    #[test]
    fn test_roundtrip_pypi_template() {
        let original = Template::PyPI {
            package: "bitbox02".to_string(),
            version_type: None,
        };
        let expanded = expand_template(original.clone());

        let detected = detect_template(
            expanded.source.as_deref(),
            expanded.matching_pattern.as_deref(),
            expanded.searchmode.as_deref(),
            expanded.mode.as_deref(),
        );

        assert_eq!(detected, Some(original));
    }

    #[test]
    fn test_roundtrip_npmregistry_template() {
        let original = Template::Npmregistry {
            package: "@scope/package".to_string(),
            version_type: None,
        };
        let expanded = expand_template(original.clone());

        let detected = detect_template(
            expanded.source.as_deref(),
            expanded.matching_pattern.as_deref(),
            expanded.searchmode.as_deref(),
            expanded.mode.as_deref(),
        );

        assert_eq!(detected, Some(original));
    }

    #[test]
    fn test_roundtrip_metacpan_template() {
        let original = Template::Metacpan {
            dist: "MetaCPAN-Client".to_string(),
            version_type: None,
        };
        let expanded = expand_template(original.clone());

        let detected = detect_template(
            expanded.source.as_deref(),
            expanded.matching_pattern.as_deref(),
            expanded.searchmode.as_deref(),
            expanded.mode.as_deref(),
        );

        assert_eq!(detected, Some(original));
    }

    #[test]
    fn test_roundtrip_github_with_version_type() {
        let original = Template::GitHub {
            owner: "foo".to_string(),
            repository: "bar".to_string(),
            release_only: true,
            version_type: Some("stable".to_string()),
        };
        let expanded = expand_template(original.clone());

        let detected = detect_template(
            expanded.source.as_deref(),
            expanded.matching_pattern.as_deref(),
            expanded.searchmode.as_deref(),
            expanded.mode.as_deref(),
        );

        assert_eq!(detected, Some(original));
    }

    #[test]
    fn test_detect_with_none_source() {
        // Should return None if source is None
        let template = detect_template(
            None,
            Some(r".*/v?@ANY_VERSION@@ARCHIVE_EXT@"),
            Some("html"),
            None,
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_detect_github_partial_match() {
        // Right URL but wrong pattern
        let template = detect_template(
            Some("https://github.com/torvalds/linux/tags"),
            Some(r".*/v?(\d+\.\d+)\.tar\.gz"), // Not a template pattern
            Some("html"),
            None,
        );

        // Should still detect GitHub but won't have version_type
        assert_eq!(
            template,
            Some(Template::GitHub {
                owner: "torvalds".to_string(),
                repository: "linux".to_string(),
                release_only: false,
                version_type: None,
            })
        );
    }

    #[test]
    fn test_extract_version_type_edge_cases() {
        // Multiple @ symbols
        assert_eq!(extract_version_type("@FOO@@BAR@"), None);

        // Only one @ symbol
        assert_eq!(extract_version_type("@INCOMPLETE"), None);

        // Not ending with _VERSION
        assert_eq!(extract_version_type("@SOMETHING@"), None);

        // Empty between @
        assert_eq!(extract_version_type("@@"), None);
    }

    #[test]
    fn test_cran_template() {
        let template = Template::Cran {
            package: "forecast".to_string(),
            version_type: None,
        };

        let result = expand_template(template);
        assert_eq!(
            result.source,
            Some("https://cran.r-project.org/package=forecast".to_string())
        );
        assert_eq!(
            result.matching_pattern,
            Some(".*_@ANY_VERSION@.tar.gz".to_string())
        );
        assert_eq!(
            result.downloadurlmangle,
            Some("s%.*/src/contrib/%https://cran.r-project.org/src/contrib/%".to_string())
        );
    }

    #[test]
    fn test_bioconductor_template() {
        let template = Template::Bioconductor {
            package: "GenomicRanges".to_string(),
            version_type: None,
        };

        let result = expand_template(template);
        assert_eq!(
            result.source,
            Some("https://bioconductor.org/packages/GenomicRanges".to_string())
        );
        assert_eq!(
            result.matching_pattern,
            Some(".*_@ANY_VERSION@.tar.gz".to_string())
        );
        assert_eq!(
            result.downloadurlmangle,
            Some(
                "s%.*/src/contrib/%https://bioconductor.org/packages/release/bioc/src/contrib/%"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_detect_cran_template() {
        let template = detect_template(
            Some("https://cran.r-project.org/package=forecast"),
            Some(".*_@ANY_VERSION@.tar.gz"),
            None,
            None,
        );

        assert_eq!(
            template,
            Some(Template::Cran {
                package: "forecast".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_bioconductor_template() {
        let template = detect_template(
            Some("https://bioconductor.org/packages/GenomicRanges"),
            Some(".*_@ANY_VERSION@.tar.gz"),
            None,
            None,
        );

        assert_eq!(
            template,
            Some(Template::Bioconductor {
                package: "GenomicRanges".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_cran_from_source_url() {
        let template = detect_cran_from_source_url(
            "https://cran.r-project.org/src/contrib/",
            Some(r"forecast_([-.\d]*)\.tar\.gz"),
        );

        assert_eq!(
            template,
            Some(Template::Cran {
                package: "forecast".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_cran_from_cloud_url() {
        let template = detect_cran_from_source_url(
            "https://cloud.r-project.org/src/contrib/",
            Some(r"forecast_([-.\d]*)\.tar\.gz"),
        );

        assert_eq!(
            template,
            Some(Template::Cran {
                package: "forecast".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_cran_from_source_url_no_match() {
        let template = detect_cran_from_source_url(
            "https://example.com/src/contrib/",
            Some(r"forecast_([-.\d]*)\.tar\.gz"),
        );

        assert_eq!(template, None);
    }

    #[test]
    fn test_roundtrip_cran_template() {
        let original = Template::Cran {
            package: "forecast".to_string(),
            version_type: None,
        };
        let expanded = expand_template(original.clone());

        let detected = detect_template(
            expanded.source.as_deref(),
            expanded.matching_pattern.as_deref(),
            expanded.searchmode.as_deref(),
            expanded.mode.as_deref(),
        );

        assert_eq!(detected, Some(original));
    }

    #[test]
    fn test_roundtrip_bioconductor_template() {
        let original = Template::Bioconductor {
            package: "GenomicRanges".to_string(),
            version_type: None,
        };
        let expanded = expand_template(original.clone());

        let detected = detect_template(
            expanded.source.as_deref(),
            expanded.matching_pattern.as_deref(),
            expanded.searchmode.as_deref(),
            expanded.mode.as_deref(),
        );

        assert_eq!(detected, Some(original));
    }

    #[test]
    fn test_detect_cran_from_inline_url() {
        // This is the exact case from Debian bug #1133114: a v4 watch file that gets
        // mechanically converted to v5 puts the entire URL+pattern into Source
        let template = detect_template(
            Some("https://cloud.r-project.org/src/contrib/forecast_([-\\d.]*)\\.tar\\.gz"),
            None,
            None,
            None,
        );

        assert_eq!(
            template,
            Some(Template::Cran {
                package: "forecast".to_string(),
                version_type: None,
            })
        );
    }

    #[test]
    fn test_detect_cran_from_inline_url_cran_domain() {
        let template = detect_template(
            Some("https://cran.r-project.org/src/contrib/gower_([-\\d.]*)\\.tar\\.gz"),
            None,
            None,
            None,
        );

        assert_eq!(
            template,
            Some(Template::Cran {
                package: "gower".to_string(),
                version_type: None,
            })
        );
    }
}
