//! Watch file implementation for format 5 (RFC822/deb822 style)
use crate::types::ParseError as TypesParseError;
use crate::VersionPolicy;
use deb822_lossless::{Deb822, Paragraph};
use std::str::FromStr;

/// Get the deb822 field name for a WatchOption variant
fn watch_option_to_key(option: &crate::types::WatchOption) -> &'static str {
    use crate::types::WatchOption;

    match option {
        WatchOption::Component(_) => "Component",
        WatchOption::Compression(_) => "Compression",
        WatchOption::UserAgent(_) => "User-Agent",
        WatchOption::Pagemangle(_) => "Pagemangle",
        WatchOption::Uversionmangle(_) => "Uversionmangle",
        WatchOption::Dversionmangle(_) => "Dversionmangle",
        WatchOption::Dirversionmangle(_) => "Dirversionmangle",
        WatchOption::Oversionmangle(_) => "Oversionmangle",
        WatchOption::Downloadurlmangle(_) => "Downloadurlmangle",
        WatchOption::Pgpsigurlmangle(_) => "Pgpsigurlmangle",
        WatchOption::Filenamemangle(_) => "Filenamemangle",
        WatchOption::VersionPolicy(_) => "Version-Policy",
        WatchOption::Searchmode(_) => "Searchmode",
        WatchOption::Mode(_) => "Mode",
        WatchOption::Pgpmode(_) => "Pgpmode",
        WatchOption::Gitexport(_) => "Gitexport",
        WatchOption::Gitmode(_) => "Gitmode",
        WatchOption::Pretty(_) => "Pretty",
        WatchOption::Ctype(_) => "Ctype",
        WatchOption::Repacksuffix(_) => "Repacksuffix",
        WatchOption::Unzipopt(_) => "Unzipopt",
        WatchOption::Script(_) => "Script",
        WatchOption::Decompress => "Decompress",
        WatchOption::Bare => "Bare",
        WatchOption::Repack => "Repack",
    }
}

#[derive(Debug)]
/// Parse error for watch file parsing
pub struct ParseError(String);

impl std::error::Error for ParseError {}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "ParseError: {}", self.0)
    }
}

/// A watch file in format 5 (RFC822/deb822 style)
#[derive(Debug)]
pub struct WatchFile(Deb822);

/// An entry in a format 5 watch file
#[derive(Debug)]
pub struct Entry {
    paragraph: Paragraph,
    defaults: Option<Paragraph>,
}

impl WatchFile {
    /// Returns a reference to the underlying deb822 document.
    pub fn as_deb822(&self) -> &Deb822 {
        &self.0
    }

    /// Construct a WatchFile from an already-parsed Deb822 document.
    ///
    /// This avoids re-parsing when the caller already has a parsed tree.
    /// No version validation is performed — the caller is responsible for
    /// ensuring the document is a valid version 5 watch file.
    pub(crate) fn from_deb822(deb822: Deb822) -> Self {
        WatchFile(deb822)
    }

    /// Create a new empty format 5 watch file
    pub fn new() -> Self {
        // Create a minimal format 5 watch file from a string
        let content = "Version: 5\n";
        WatchFile::from_str(content).expect("Failed to create empty watch file")
    }

    /// Returns the version of the watch file (always 5 for this type)
    pub fn version(&self) -> u32 {
        5
    }

    /// Returns the defaults paragraph if it exists.
    /// The defaults paragraph is the second paragraph (after Version) if it has no Source field.
    pub fn defaults(&self) -> Option<Paragraph> {
        let paragraphs: Vec<_> = self.0.paragraphs().collect();

        if paragraphs.len() > 1 {
            // Check if second paragraph looks like defaults (no Source field)
            if !paragraphs[1].contains_key("Source") && !paragraphs[1].contains_key("source") {
                return Some(paragraphs[1].clone());
            }
        }

        None
    }

    /// Returns an iterator over all entries in the watch file.
    /// The first paragraph contains defaults, subsequent paragraphs are entries.
    pub fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        let paragraphs: Vec<_> = self.0.paragraphs().collect();
        let defaults = self.defaults();

        // Skip the first paragraph (version)
        // The second paragraph (if it exists and has specific fields) contains defaults
        // Otherwise all paragraphs are entries
        let start_index = if paragraphs.len() > 1 {
            // Check if second paragraph looks like defaults (no Source or Template field)
            let has_source =
                paragraphs[1].contains_key("Source") || paragraphs[1].contains_key("source");
            let has_template =
                paragraphs[1].contains_key("Template") || paragraphs[1].contains_key("template");

            if !has_source && !has_template {
                2 // Skip version and defaults
            } else {
                1 // Skip only version
            }
        } else {
            1
        };

        paragraphs
            .into_iter()
            .skip(start_index)
            .map(move |p| Entry {
                paragraph: p,
                defaults: defaults.clone(),
            })
    }

    /// Get the underlying Deb822 object
    pub fn inner(&self) -> &Deb822 {
        &self.0
    }

    /// Get a mutable reference to the underlying Deb822 object
    pub fn inner_mut(&mut self) -> &mut Deb822 {
        &mut self.0
    }

    /// Add a new entry to the watch file with the given source and matching pattern.
    /// Returns the newly created Entry.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "deb822")]
    /// # {
    /// use debian_watch::deb822::WatchFile;
    /// use debian_watch::WatchOption;
    ///
    /// let mut wf = WatchFile::new();
    /// let mut entry = wf.add_entry("https://github.com/foo/bar/tags", ".*/v?([\\d.]+)\\.tar\\.gz");
    /// entry.set_option(WatchOption::Component("upstream".to_string()));
    /// # }
    /// ```
    pub fn add_entry(&mut self, source: &str, matching_pattern: &str) -> Entry {
        let mut para = self.0.add_paragraph();
        para.set("Source", source);
        para.set("Matching-Pattern", matching_pattern);

        // Create an Entry from the paragraph we just added
        // Get the defaults paragraph if it exists
        let defaults = self.defaults();

        Entry {
            paragraph: para.clone(),
            defaults,
        }
    }
}

impl Default for WatchFile {
    fn default() -> Self {
        Self::new()
    }
}

impl FromStr for WatchFile {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match Deb822::from_str(s) {
            Ok(deb822) => {
                // Verify it's version 5
                let version = deb822
                    .paragraphs()
                    .next()
                    .and_then(|p| p.get("Version"))
                    .unwrap_or_else(|| "1".to_string());

                if version != "5" {
                    return Err(ParseError(format!("Expected version 5, got {}", version)));
                }

                Ok(WatchFile(deb822))
            }
            Err(e) => Err(ParseError(e.to_string())),
        }
    }
}

impl std::fmt::Display for WatchFile {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Entry {
    /// Get a field value from the entry, with fallback to defaults paragraph.
    /// First checks the entry's own fields, then falls back to the defaults paragraph if present.
    pub(crate) fn get_field(&self, key: &str) -> Option<String> {
        // Try the key as-is first in the entry
        if let Some(value) = self.paragraph.get(key) {
            return Some(value);
        }

        // If not found, try with different case variations in the entry
        // deb822-lossless is case-preserving, so we need to check all field names
        let normalized_key = normalize_key(key);

        // Iterate through all keys in the paragraph and check for normalized match
        for (k, v) in self.paragraph.items() {
            if normalize_key(&k) == normalized_key {
                return Some(v);
            }
        }

        // If not found in entry, check the defaults paragraph
        if let Some(ref defaults) = self.defaults {
            // Try the key as-is first in defaults
            if let Some(value) = defaults.get(key) {
                return Some(value);
            }

            // Try with case variations in defaults
            for (k, v) in defaults.items() {
                if normalize_key(&k) == normalized_key {
                    return Some(v);
                }
            }
        }

        None
    }

    /// Returns the source URL, expanding templates if present
    ///
    /// Returns `Ok(None)` if no Source field is set and no template is present.
    /// Returns `Err` if template expansion fails.
    pub fn source(&self) -> Result<Option<String>, crate::templates::TemplateError> {
        // First check if explicitly set
        if let Some(source) = self.get_field("Source") {
            return Ok(Some(source));
        }

        // If not set, check if there's a template to expand
        if self.get_field("Template").is_none() {
            return Ok(None);
        }

        // Template exists, expand it (propagate any errors)
        self.expand_template().map(|t| t.source)
    }

    /// Returns the matching pattern, expanding templates if present
    ///
    /// Returns `Ok(None)` if no Matching-Pattern field is set and no template is present.
    /// Returns `Err` if template expansion fails.
    pub fn matching_pattern(&self) -> Result<Option<String>, crate::templates::TemplateError> {
        // First check if explicitly set
        if let Some(pattern) = self.get_field("Matching-Pattern") {
            return Ok(Some(pattern));
        }

        // If not set, check if there's a template to expand
        if self.get_field("Template").is_none() {
            return Ok(None);
        }

        // Template exists, expand it (propagate any errors)
        self.expand_template().map(|t| t.matching_pattern)
    }

    /// Get the underlying paragraph
    pub fn as_deb822(&self) -> &Paragraph {
        &self.paragraph
    }

    /// Name of the component, if specified
    pub fn component(&self) -> Option<String> {
        self.get_field("Component")
    }

    /// Get the an option value from the entry, with fallback to defaults paragraph.
    pub fn get_option(&self, key: &str) -> Option<String> {
        match key {
            "Source" => None,           // Source is not an option
            "Matching-Pattern" => None, // Matching-Pattern is not an option
            "Component" => None,        // Component is not an option
            "Version" => None,          // Version is not an option
            key => self.get_field(key),
        }
    }

    /// Set an option value in the entry using a WatchOption enum
    pub fn set_option(&mut self, option: crate::types::WatchOption) {
        use crate::types::WatchOption;

        let (key, value) = match option {
            WatchOption::Component(v) => ("Component", Some(v)),
            WatchOption::Compression(v) => ("Compression", Some(v.to_string())),
            WatchOption::UserAgent(v) => ("User-Agent", Some(v)),
            WatchOption::Pagemangle(v) => ("Pagemangle", Some(v)),
            WatchOption::Uversionmangle(v) => ("Uversionmangle", Some(v)),
            WatchOption::Dversionmangle(v) => ("Dversionmangle", Some(v)),
            WatchOption::Dirversionmangle(v) => ("Dirversionmangle", Some(v)),
            WatchOption::Oversionmangle(v) => ("Oversionmangle", Some(v)),
            WatchOption::Downloadurlmangle(v) => ("Downloadurlmangle", Some(v)),
            WatchOption::Pgpsigurlmangle(v) => ("Pgpsigurlmangle", Some(v)),
            WatchOption::Filenamemangle(v) => ("Filenamemangle", Some(v)),
            WatchOption::VersionPolicy(v) => ("Version-Policy", Some(v.to_string())),
            WatchOption::Searchmode(v) => ("Searchmode", Some(v.to_string())),
            WatchOption::Mode(v) => ("Mode", Some(v.to_string())),
            WatchOption::Pgpmode(v) => ("Pgpmode", Some(v.to_string())),
            WatchOption::Gitexport(v) => ("Gitexport", Some(v.to_string())),
            WatchOption::Gitmode(v) => ("Gitmode", Some(v.to_string())),
            WatchOption::Pretty(v) => ("Pretty", Some(v.to_string())),
            WatchOption::Ctype(v) => ("Ctype", Some(v.to_string())),
            WatchOption::Repacksuffix(v) => ("Repacksuffix", Some(v)),
            WatchOption::Unzipopt(v) => ("Unzipopt", Some(v)),
            WatchOption::Script(v) => ("Script", Some(v)),
            WatchOption::Decompress => ("Decompress", None),
            WatchOption::Bare => ("Bare", None),
            WatchOption::Repack => ("Repack", None),
        };

        if let Some(v) = value {
            self.paragraph.set(key, &v);
        } else {
            // For boolean flags, set the key with empty value
            self.paragraph.set(key, "");
        }
    }

    /// Set an option value in the entry using string key and value (for backward compatibility)
    pub fn set_option_str(&mut self, key: &str, value: &str) {
        self.paragraph.set(key, value);
    }

    /// Delete an option from the entry using a WatchOption enum
    pub fn delete_option(&mut self, option: crate::types::WatchOption) {
        let key = watch_option_to_key(&option);
        self.paragraph.remove(key);
    }

    /// Delete an option from the entry using a string key (for backward compatibility)
    pub fn delete_option_str(&mut self, key: &str) {
        self.paragraph.remove(key);
    }

    /// Get the URL (same as source() but named url() for consistency)
    pub fn url(&self) -> String {
        self.source().unwrap_or(None).unwrap_or_default()
    }

    /// Get the version policy
    pub fn version_policy(&self) -> Result<Option<VersionPolicy>, TypesParseError> {
        match self.get_field("Version-Policy") {
            Some(policy) => Ok(Some(policy.parse()?)),
            None => Ok(None),
        }
    }

    /// Get the script
    pub fn script(&self) -> Option<String> {
        self.get_field("Script")
    }

    /// Set the source URL
    pub fn set_source(&mut self, url: &str) {
        self.paragraph.set("Source", url);
    }

    /// Set the matching pattern
    pub fn set_matching_pattern(&mut self, pattern: &str) {
        self.paragraph.set("Matching-Pattern", pattern);
    }

    /// Get the line number (0-indexed) where this entry starts
    pub fn line(&self) -> usize {
        self.paragraph.line()
    }

    /// Retrieve the mode of the watch file entry with detailed error information.
    pub fn mode(&self) -> Result<crate::types::Mode, TypesParseError> {
        Ok(self
            .get_field("Mode")
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or_default())
    }

    /// Expand template if present
    fn expand_template(
        &self,
    ) -> Result<crate::templates::ExpandedTemplate, crate::templates::TemplateError> {
        use crate::templates::{expand_template, parse_github_url, Template, TemplateError};

        // Check if there's a Template field
        let template_str =
            self.get_field("Template")
                .ok_or_else(|| TemplateError::MissingField {
                    template: "any".to_string(),
                    field: "Template".to_string(),
                })?;

        let release_only = self
            .get_field("Release-Only")
            .map(|v| v.to_lowercase() == "yes")
            .unwrap_or(false);

        let version_type = self.get_field("Version-Type");

        // Build the appropriate Template enum variant
        let template = match template_str.to_lowercase().as_str() {
            "github" => {
                // GitHub requires either Dist or Owner+Project
                let (owner, repository) = if let (Some(o), Some(p)) =
                    (self.get_field("Owner"), self.get_field("Project"))
                {
                    (o, p)
                } else if let Some(dist) = self.get_field("Dist") {
                    parse_github_url(&dist)?
                } else {
                    return Err(TemplateError::MissingField {
                        template: "GitHub".to_string(),
                        field: "Dist or Owner+Project".to_string(),
                    });
                };

                Template::GitHub {
                    owner,
                    repository,
                    release_only,
                    version_type,
                }
            }
            "gitlab" => {
                let dist = self
                    .get_field("Dist")
                    .ok_or_else(|| TemplateError::MissingField {
                        template: "GitLab".to_string(),
                        field: "Dist".to_string(),
                    })?;

                Template::GitLab {
                    dist,
                    release_only,
                    version_type,
                }
            }
            "pypi" => {
                let package =
                    self.get_field("Dist")
                        .ok_or_else(|| TemplateError::MissingField {
                            template: "PyPI".to_string(),
                            field: "Dist".to_string(),
                        })?;

                Template::PyPI {
                    package,
                    version_type,
                }
            }
            "npmregistry" => {
                let package =
                    self.get_field("Dist")
                        .ok_or_else(|| TemplateError::MissingField {
                            template: "Npmregistry".to_string(),
                            field: "Dist".to_string(),
                        })?;

                Template::Npmregistry {
                    package,
                    version_type,
                }
            }
            "metacpan" => {
                let dist = self
                    .get_field("Dist")
                    .ok_or_else(|| TemplateError::MissingField {
                        template: "Metacpan".to_string(),
                        field: "Dist".to_string(),
                    })?;

                Template::Metacpan { dist, version_type }
            }
            "cran" => {
                let package =
                    self.get_field("Package")
                        .ok_or_else(|| TemplateError::MissingField {
                            template: "CRAN".to_string(),
                            field: "Package".to_string(),
                        })?;

                Template::Cran {
                    package,
                    version_type,
                }
            }
            "bioconductor" => {
                let package =
                    self.get_field("Package")
                        .ok_or_else(|| TemplateError::MissingField {
                            template: "Bioconductor".to_string(),
                            field: "Package".to_string(),
                        })?;

                Template::Bioconductor {
                    package,
                    version_type,
                }
            }
            _ => return Err(TemplateError::UnknownTemplate(template_str)),
        };

        Ok(expand_template(template))
    }

    /// Try to detect if this entry matches a template pattern and convert it to use that template.
    ///
    /// This analyzes the Source, Matching-Pattern, Searchmode, and Mode fields to determine
    /// if they match a known template pattern. If a match is found, the entry is converted
    /// to use the template syntax instead.
    ///
    /// # Returns
    ///
    /// Returns `Some(template)` if a template was detected and applied, `None` if no
    /// template matches the current entry configuration.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "deb822")]
    /// # {
    /// use debian_watch::deb822::WatchFile;
    ///
    /// let mut wf = WatchFile::new();
    /// let mut entry = wf.add_entry(
    ///     "https://github.com/torvalds/linux/tags",
    ///     r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@"
    /// );
    /// entry.set_option_str("Searchmode", "html");
    ///
    /// // Convert to template
    /// if let Some(template) = entry.try_convert_to_template() {
    ///     println!("Converted to {:?}", template);
    /// }
    /// # }
    /// ```
    pub fn try_convert_to_template(&mut self) -> Option<crate::templates::Template> {
        use crate::templates::detect_template;

        // Get current field values
        let source = self.source().ok().flatten();
        let matching_pattern = self.matching_pattern().ok().flatten();
        let searchmode = self.get_field("Searchmode");
        let mode = self.get_field("Mode");

        // Try to detect template
        let template = detect_template(
            source.as_deref(),
            matching_pattern.as_deref(),
            searchmode.as_deref(),
            mode.as_deref(),
        )?;

        // Apply the template - remove old fields and add template fields
        self.paragraph.remove("Source");
        self.paragraph.remove("Matching-Pattern");
        self.paragraph.remove("Searchmode");
        self.paragraph.remove("Mode");

        // Set template fields based on the detected template
        match &template {
            crate::templates::Template::GitHub {
                owner,
                repository,
                release_only,
                version_type,
            } => {
                self.paragraph.set("Template", "GitHub");
                self.paragraph.set("Owner", owner);
                self.paragraph.set("Project", repository);
                if *release_only {
                    self.paragraph.set("Release-Only", "yes");
                }
                if let Some(vt) = version_type {
                    self.paragraph.set("Version-Type", vt);
                }
            }
            crate::templates::Template::GitLab {
                dist,
                release_only: _,
                version_type,
            } => {
                self.paragraph.set("Template", "GitLab");
                self.paragraph.set("Dist", dist);
                if let Some(vt) = version_type {
                    self.paragraph.set("Version-Type", vt);
                }
            }
            crate::templates::Template::PyPI {
                package,
                version_type,
            } => {
                self.paragraph.set("Template", "PyPI");
                self.paragraph.set("Dist", package);
                if let Some(vt) = version_type {
                    self.paragraph.set("Version-Type", vt);
                }
            }
            crate::templates::Template::Npmregistry {
                package,
                version_type,
            } => {
                self.paragraph.set("Template", "Npmregistry");
                self.paragraph.set("Dist", package);
                if let Some(vt) = version_type {
                    self.paragraph.set("Version-Type", vt);
                }
            }
            crate::templates::Template::Metacpan { dist, version_type } => {
                self.paragraph.set("Template", "Metacpan");
                self.paragraph.set("Dist", dist);
                if let Some(vt) = version_type {
                    self.paragraph.set("Version-Type", vt);
                }
            }
            crate::templates::Template::Cran {
                package,
                version_type,
            } => {
                self.paragraph.set("Template", "CRAN");
                self.paragraph.set("Package", &package);
                if let Some(vt) = version_type {
                    self.paragraph.set("Version-Type", vt);
                }
            }
            crate::templates::Template::Bioconductor {
                package,
                version_type,
            } => {
                self.paragraph.set("Template", "Bioconductor");
                self.paragraph.set("Package", &package);
                if let Some(vt) = version_type {
                    self.paragraph.set("Version-Type", vt);
                }
            }
        }

        Some(template)
    }
}

/// Normalize a field key according to RFC822 rules:
/// - Convert to lowercase
/// - Hyphens and underscores are treated as equivalent
fn normalize_key(key: &str) -> String {
    key.to_lowercase().replace(['-', '_'], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_as_deb822() {
        let input = r#"Version: 5

Source: https://github.com/owner/repo/tags
Matching-Pattern: .*/v?(\d\S+)\.tar\.gz
"#;

        let wf: WatchFile = input.parse().unwrap();
        let deb822 = wf.as_deb822();

        // Should have 2 paragraphs: version header + entry
        assert_eq!(deb822.paragraphs().count(), 2);
    }

    #[test]
    fn test_create_v5_watchfile() {
        let wf = WatchFile::new();
        assert_eq!(wf.version(), 5);

        let output = wf.to_string();
        assert!(output.contains("Version"));
        assert!(output.contains("5"));
    }

    #[test]
    fn test_parse_v5_basic() {
        let input = r#"Version: 5

Source: https://github.com/owner/repo/tags
Matching-Pattern: .*/v?(\d\S+)\.tar\.gz
"#;

        let wf: WatchFile = input.parse().unwrap();
        assert_eq!(wf.version(), 5);

        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.source().unwrap().as_deref(),
            Some("https://github.com/owner/repo/tags")
        );
        assert_eq!(
            entry.matching_pattern().unwrap(),
            Some(".*/v?(\\d\\S+)\\.tar\\.gz".to_string())
        );
    }

    #[test]
    fn test_parse_v5_multiple_entries() {
        let input = r#"Version: 5

Source: https://github.com/owner/repo1/tags
Matching-Pattern: .*/v?(\d\S+)\.tar\.gz

Source: https://github.com/owner/repo2/tags
Matching-Pattern: .*/release-(\d\S+)\.tar\.gz
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 2);

        assert_eq!(
            entries[0].source().unwrap().as_deref(),
            Some("https://github.com/owner/repo1/tags")
        );
        assert_eq!(
            entries[1].source().unwrap().as_deref(),
            Some("https://github.com/owner/repo2/tags")
        );
    }

    #[test]
    fn test_v5_case_insensitive_fields() {
        let input = r#"Version: 5

source: https://example.com/files
matching-pattern: .*\.tar\.gz
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.source().unwrap().as_deref(),
            Some("https://example.com/files")
        );
        assert_eq!(
            entry.matching_pattern().unwrap().as_deref(),
            Some(".*\\.tar\\.gz")
        );
    }

    #[test]
    fn test_v5_with_compression_option() {
        let input = r#"Version: 5

Source: https://example.com/files
Matching-Pattern: .*\.tar\.gz
Compression: xz
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        let compression = entry.get_option("compression");
        assert!(compression.is_some());
    }

    #[test]
    fn test_v5_with_component() {
        let input = r#"Version: 5

Source: https://example.com/files
Matching-Pattern: .*\.tar\.gz
Component: foo
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.component(), Some("foo".to_string()));
    }

    #[test]
    fn test_v5_rejects_wrong_version() {
        let input = r#"Version: 4

Source: https://example.com/files
Matching-Pattern: .*\.tar\.gz
"#;

        let result: Result<WatchFile, _> = input.parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_v5_roundtrip() {
        let input = r#"Version: 5

Source: https://example.com/files
Matching-Pattern: .*\.tar\.gz
"#;

        let wf: WatchFile = input.parse().unwrap();
        let output = wf.to_string();

        // The output should be parseable again
        let wf2: WatchFile = output.parse().unwrap();
        assert_eq!(wf2.version(), 5);

        let entries: Vec<_> = wf2.entries().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_normalize_key() {
        assert_eq!(normalize_key("Matching-Pattern"), "matchingpattern");
        assert_eq!(normalize_key("matching_pattern"), "matchingpattern");
        assert_eq!(normalize_key("MatchingPattern"), "matchingpattern");
        assert_eq!(normalize_key("MATCHING-PATTERN"), "matchingpattern");
    }

    #[test]
    fn test_defaults_paragraph() {
        let input = r#"Version: 5

Compression: xz
User-Agent: Custom/1.0

Source: https://example.com/repo1
Matching-Pattern: .*\.tar\.gz

Source: https://example.com/repo2
Matching-Pattern: .*\.tar\.gz
Compression: gz
"#;

        let wf: WatchFile = input.parse().unwrap();

        // Check that defaults paragraph is detected
        let defaults = wf.defaults();
        assert!(defaults.is_some());
        let defaults = defaults.unwrap();
        assert_eq!(defaults.get("Compression"), Some("xz".to_string()));
        assert_eq!(defaults.get("User-Agent"), Some("Custom/1.0".to_string()));

        // Check that entries inherit from defaults
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 2);

        // First entry should inherit Compression and User-Agent from defaults
        assert_eq!(entries[0].get_option("Compression"), Some("xz".to_string()));
        assert_eq!(
            entries[0].get_option("User-Agent"),
            Some("Custom/1.0".to_string())
        );

        // Second entry overrides Compression but inherits User-Agent
        assert_eq!(entries[1].get_option("Compression"), Some("gz".to_string()));
        assert_eq!(
            entries[1].get_option("User-Agent"),
            Some("Custom/1.0".to_string())
        );
    }

    #[test]
    fn test_no_defaults_paragraph() {
        let input = r#"Version: 5

Source: https://example.com/repo1
Matching-Pattern: .*\.tar\.gz
"#;

        let wf: WatchFile = input.parse().unwrap();

        // Check that there's no defaults paragraph (first paragraph has Source)
        assert!(wf.defaults().is_none());

        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_set_source() {
        let mut wf = WatchFile::new();
        let mut entry = wf.add_entry("https://example.com/repo1", ".*\\.tar\\.gz");

        assert_eq!(
            entry.source().unwrap(),
            Some("https://example.com/repo1".to_string())
        );

        entry.set_source("https://example.com/repo2");
        assert_eq!(
            entry.source().unwrap(),
            Some("https://example.com/repo2".to_string())
        );
    }

    #[test]
    fn test_set_matching_pattern() {
        let mut wf = WatchFile::new();
        let mut entry = wf.add_entry("https://example.com/repo1", ".*\\.tar\\.gz");

        assert_eq!(
            entry.matching_pattern().unwrap(),
            Some(".*\\.tar\\.gz".to_string())
        );

        entry.set_matching_pattern(".*/v?([\\d.]+)\\.tar\\.gz");
        assert_eq!(
            entry.matching_pattern().unwrap(),
            Some(".*/v?([\\d.]+)\\.tar\\.gz".to_string())
        );
    }

    #[test]
    fn test_entry_line() {
        let input = r#"Version: 5

Source: https://example.com/repo1
Matching-Pattern: .*\.tar\.gz

Source: https://example.com/repo2
Matching-Pattern: .*\.tar\.xz
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();

        // First entry starts at line 2 (0-indexed)
        assert_eq!(entries[0].line(), 2);
        // Second entry starts at line 5 (0-indexed)
        assert_eq!(entries[1].line(), 5);
    }

    #[test]
    fn test_defaults_with_case_variations() {
        let input = r#"Version: 5

compression: xz
user-agent: Custom/1.0

Source: https://example.com/repo1
Matching-Pattern: .*\.tar\.gz
"#;

        let wf: WatchFile = input.parse().unwrap();

        // Check that defaults work with different case
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        // Should find defaults even with different case
        assert_eq!(entries[0].get_option("Compression"), Some("xz".to_string()));
        assert_eq!(
            entries[0].get_option("User-Agent"),
            Some("Custom/1.0".to_string())
        );
    }

    #[test]
    fn test_v5_with_uversionmangle() {
        let input = r#"Version: 5

Source: https://pypi.org/project/foo/
Matching-Pattern: foo-(\d+\.\d+)\.tar\.gz
Uversionmangle: s/\.0+$//
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.get_option("Uversionmangle"),
            Some("s/\\.0+$//".to_string())
        );
    }

    #[test]
    fn test_v5_with_filenamemangle() {
        let input = r#"Version: 5

Source: https://example.com/files
Matching-Pattern: .*\.tar\.gz
Filenamemangle: s/.*\///;s/@PACKAGE@-(.*)\.tar\.gz/foo_$1.orig.tar.gz/
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.get_option("Filenamemangle"),
            Some("s/.*\\///;s/@PACKAGE@-(.*)\\.tar\\.gz/foo_$1.orig.tar.gz/".to_string())
        );
    }

    #[test]
    fn test_v5_with_searchmode() {
        let input = r#"Version: 5

Source: https://example.com/files
Matching-Pattern: foo-(\d[\d.]*)\.tar\.gz
Searchmode: plain
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.get_field("Searchmode").as_deref(), Some("plain"));
    }

    #[test]
    fn test_v5_with_version_policy() {
        let input = r#"Version: 5

Source: https://example.com/files
Matching-Pattern: .*\.tar\.gz
Version-Policy: debian
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        let policy = entry.version_policy();
        assert!(policy.is_ok());
        assert_eq!(format!("{:?}", policy.unwrap().unwrap()), "Debian");
    }

    #[test]
    fn test_v5_multiple_mangles() {
        let input = r#"Version: 5

Source: https://example.com/files
Matching-Pattern: .*\.tar\.gz
Uversionmangle: s/^v//;s/\.0+$//
Dversionmangle: s/\+dfsg\d*$//
Filenamemangle: s/.*/foo-$1.tar.gz/
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.get_option("Uversionmangle"),
            Some("s/^v//;s/\\.0+$//".to_string())
        );
        assert_eq!(
            entry.get_option("Dversionmangle"),
            Some("s/\\+dfsg\\d*$//".to_string())
        );
        assert_eq!(
            entry.get_option("Filenamemangle"),
            Some("s/.*/foo-$1.tar.gz/".to_string())
        );
    }

    #[test]
    fn test_v5_with_pgpmode() {
        let input = r#"Version: 5

Source: https://example.com/files
Matching-Pattern: .*\.tar\.gz
Pgpmode: auto
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.get_option("Pgpmode"), Some("auto".to_string()));
    }

    #[test]
    fn test_v5_with_comments() {
        let input = r#"Version: 5

# This is a comment about the entry
Source: https://example.com/files
Matching-Pattern: .*\.tar\.gz
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        // Verify roundtrip preserves comments
        let output = wf.to_string();
        assert!(output.contains("# This is a comment about the entry"));
    }

    #[test]
    fn test_v5_empty_after_version() {
        let input = "Version: 5\n";

        let wf: WatchFile = input.parse().unwrap();
        assert_eq!(wf.version(), 5);

        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_v5_trait_url() {
        let input = r#"Version: 5

Source: https://example.com/files/@PACKAGE@
Matching-Pattern: .*\.tar\.gz
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        // Test url() method
        assert_eq!(
            entry.source().unwrap().as_deref(),
            Some("https://example.com/files/@PACKAGE@")
        );
    }

    #[test]
    fn test_github_template() {
        let input = r#"Version: 5

Template: GitHub
Owner: torvalds
Project: linux
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.source().unwrap(),
            Some("https://github.com/torvalds/linux/tags".to_string())
        );
        assert_eq!(
            entry.matching_pattern().unwrap(),
            Some(r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@".to_string())
        );
    }

    #[test]
    fn test_github_template_with_dist() {
        let input = r#"Version: 5

Template: GitHub
Dist: https://github.com/guimard/llng-docker
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.source().unwrap(),
            Some("https://github.com/guimard/llng-docker/tags".to_string())
        );
    }

    #[test]
    fn test_pypi_template() {
        let input = r#"Version: 5

Template: PyPI
Dist: bitbox02
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.source().unwrap(),
            Some("https://pypi.debian.net/bitbox02/".to_string())
        );
        assert_eq!(
            entry.matching_pattern().unwrap(),
            Some(
                r"https://pypi\.debian\.net/bitbox02/[^/]+\.tar\.gz#/.*-@ANY_VERSION@\.tar\.gz"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_gitlab_template() {
        let input = r#"Version: 5

Template: GitLab
Dist: https://salsa.debian.org/debian/devscripts
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.source().unwrap(),
            Some("https://salsa.debian.org/debian/devscripts".to_string())
        );
        assert_eq!(
            entry.matching_pattern().unwrap(),
            Some(r".*/v?@ANY_VERSION@@ARCHIVE_EXT@".to_string())
        );
    }

    #[test]
    fn test_template_with_explicit_source() {
        // Explicit Source should override template expansion
        let input = r#"Version: 5

Template: GitHub
Owner: test
Project: project
Source: https://custom.example.com/
"#;

        let wf: WatchFile = input.parse().unwrap();
        let entries: Vec<_> = wf.entries().collect();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(
            entry.source().unwrap(),
            Some("https://custom.example.com/".to_string())
        );
    }

    #[test]
    fn test_convert_to_template_github() {
        let mut wf = WatchFile::new();
        let mut entry = wf.add_entry(
            "https://github.com/torvalds/linux/tags",
            r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@",
        );
        entry.set_option_str("Searchmode", "html");

        // Convert to template
        let template = entry.try_convert_to_template();
        assert_eq!(
            template,
            Some(crate::templates::Template::GitHub {
                owner: "torvalds".to_string(),
                repository: "linux".to_string(),
                release_only: false,
                version_type: None,
            })
        );

        // Verify the entry now uses template syntax
        assert_eq!(entry.get_field("Template"), Some("GitHub".to_string()));
        assert_eq!(entry.get_field("Owner"), Some("torvalds".to_string()));
        assert_eq!(entry.get_field("Project"), Some("linux".to_string()));
        assert_eq!(entry.get_field("Source"), None);
        assert_eq!(entry.get_field("Matching-Pattern"), None);
    }

    #[test]
    fn test_convert_to_template_pypi() {
        let mut wf = WatchFile::new();
        let mut entry = wf.add_entry(
            "https://pypi.debian.net/bitbox02/",
            r"https://pypi\.debian\.net/bitbox02/[^/]+\.tar\.gz#/.*-@ANY_VERSION@\.tar\.gz",
        );
        entry.set_option_str("Searchmode", "plain");

        // Convert to template
        let template = entry.try_convert_to_template();
        assert_eq!(
            template,
            Some(crate::templates::Template::PyPI {
                package: "bitbox02".to_string(),
                version_type: None,
            })
        );

        // Verify the entry now uses template syntax
        assert_eq!(entry.get_field("Template"), Some("PyPI".to_string()));
        assert_eq!(entry.get_field("Dist"), Some("bitbox02".to_string()));
    }

    #[test]
    fn test_convert_to_template_no_match() {
        let mut wf = WatchFile::new();
        let mut entry = wf.add_entry(
            "https://example.com/downloads/",
            r".*/v?(\d+\.\d+)\.tar\.gz",
        );

        // Try to convert - should return None
        let template = entry.try_convert_to_template();
        assert_eq!(template, None);

        // Entry should remain unchanged
        assert_eq!(
            entry.source().unwrap(),
            Some("https://example.com/downloads/".to_string())
        );
    }

    #[test]
    fn test_convert_to_template_roundtrip() {
        let mut wf = WatchFile::new();
        let mut entry = wf.add_entry(
            "https://github.com/test/project/releases",
            r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@",
        );
        entry.set_option_str("Searchmode", "html");

        // Convert to template
        entry.try_convert_to_template().unwrap();

        // Now the entry should be able to expand back to the same values
        let source = entry.source().unwrap();
        let matching_pattern = entry.matching_pattern().unwrap();

        assert_eq!(
            source,
            Some("https://github.com/test/project/releases".to_string())
        );
        assert_eq!(
            matching_pattern,
            Some(r".*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@".to_string())
        );
    }
}
