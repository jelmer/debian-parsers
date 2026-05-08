#![cfg(any(feature = "linebased", feature = "deb822"))]
//! Format detection and parsing for watch files
//!
//! This module is only available when at least one of the `linebased` or `deb822` features is enabled.

/// Error type for parsing watch files
#[derive(Debug)]
pub enum ParseError {
    /// Error parsing line-based format (v1-4)
    #[cfg(feature = "linebased")]
    LineBased(crate::linebased::ParseError),
    /// Error parsing deb822 format (v5)
    #[cfg(feature = "deb822")]
    Deb822(crate::deb822::ParseError),
    /// Could not detect version
    UnknownVersion,
    /// Feature not enabled
    FeatureNotEnabled(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            #[cfg(feature = "linebased")]
            ParseError::LineBased(e) => write!(f, "{}", e),
            #[cfg(feature = "deb822")]
            ParseError::Deb822(e) => write!(f, "{}", e),
            ParseError::UnknownVersion => write!(f, "Could not detect watch file version"),
            ParseError::FeatureNotEnabled(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

/// Detected watch file format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchFileVersion {
    /// Line-based format (versions 1-4)
    LineBased(u32),
    /// Deb822 format (version 5)
    Deb822,
}

/// Detect the version/format of a watch file from its content
///
/// This function examines the content to determine if it's a line-based
/// format (v1-4) or deb822 format (v5).
///
/// After detecting the version, you can either:
/// - Use the `parse()` function to automatically parse and return a `ParsedWatchFile`
/// - Parse directly: `content.parse::<debian_watch::linebased::WatchFile>()`
///
/// # Examples
///
/// ```
/// use debian_watch::parse::{detect_version, WatchFileVersion};
///
/// let v4_content = "version=4\nhttps://example.com/ .*.tar.gz";
/// assert_eq!(detect_version(v4_content), Some(WatchFileVersion::LineBased(4)));
///
/// let v5_content = "Version: 5\n\nSource: https://example.com/";
/// assert_eq!(detect_version(v5_content), Some(WatchFileVersion::Deb822));
/// ```
pub fn detect_version(content: &str) -> Option<WatchFileVersion> {
    let trimmed = content.trim_start();

    // Check if it starts with RFC822-style "Version: 5"
    if trimmed.starts_with("Version:") || trimmed.starts_with("version:") {
        // Try to extract the version number
        if let Some(first_line) = trimmed.lines().next() {
            if let Some(colon_pos) = first_line.find(':') {
                let version_str = first_line[colon_pos + 1..].trim();
                if version_str == "5" {
                    return Some(WatchFileVersion::Deb822);
                }
            }
        }
    }

    // Otherwise, it's line-based format
    // Try to detect the version from "version=N" line
    for line in trimmed.lines() {
        let line = line.trim();

        // Skip comments and blank lines
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        // Check for version=N
        if line.starts_with("version=") || line.starts_with("version =") {
            let version_part = if line.starts_with("version=") {
                &line[8..]
            } else {
                &line[9..]
            };

            if let Ok(version) = version_part.trim().parse::<u32>() {
                return Some(WatchFileVersion::LineBased(version));
            }
        }

        // If we hit a non-comment, non-version line, assume default version
        break;
    }

    // Default to version 1 for line-based format
    Some(WatchFileVersion::LineBased(crate::DEFAULT_VERSION))
}

/// Parsed watch file that can be either line-based or deb822 format
#[derive(Debug)]
pub enum ParsedWatchFile {
    /// Line-based watch file (v1-4)
    #[cfg(feature = "linebased")]
    LineBased(crate::linebased::WatchFile),
    /// Deb822 watch file (v5)
    #[cfg(feature = "deb822")]
    Deb822(crate::deb822::WatchFile),
}

/// Parsed watch entry that can be either line-based or deb822 format
#[derive(Debug)]
pub enum ParsedEntry {
    /// Line-based entry (v1-4)
    #[cfg(feature = "linebased")]
    LineBased(crate::linebased::Entry),
    /// Deb822 entry (v5)
    #[cfg(feature = "deb822")]
    Deb822(crate::deb822::Entry),
}

impl ParsedWatchFile {
    /// Create a new empty watch file with the specified version.
    ///
    /// - For version 5, creates a deb822-format watch file (requires `deb822` feature)
    /// - For versions 1-4, creates a line-based watch file (requires `linebased` feature)
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "deb822")]
    /// # {
    /// use debian_watch::parse::ParsedWatchFile;
    ///
    /// let wf = ParsedWatchFile::new(5).unwrap();
    /// assert_eq!(wf.version(), 5);
    /// # }
    /// ```
    pub fn new(version: u32) -> Result<Self, ParseError> {
        match version {
            #[cfg(feature = "deb822")]
            5 => Ok(ParsedWatchFile::Deb822(crate::deb822::WatchFile::new())),
            #[cfg(not(feature = "deb822"))]
            5 => Err(ParseError::FeatureNotEnabled(
                "deb822 feature required for v5 format".to_string(),
            )),
            #[cfg(feature = "linebased")]
            v @ 1..=4 => Ok(ParsedWatchFile::LineBased(
                crate::linebased::WatchFile::new(Some(v)),
            )),
            #[cfg(not(feature = "linebased"))]
            v @ 1..=4 => Err(ParseError::FeatureNotEnabled(format!(
                "linebased feature required for v{} format",
                v
            ))),
            v => Err(ParseError::FeatureNotEnabled(format!(
                "unsupported watch file version: {}",
                v
            ))),
        }
    }

    /// Get the version of the watch file
    pub fn version(&self) -> u32 {
        match self {
            #[cfg(feature = "linebased")]
            ParsedWatchFile::LineBased(wf) => wf.version(),
            #[cfg(feature = "deb822")]
            ParsedWatchFile::Deb822(wf) => wf.version(),
        }
    }

    /// Get an iterator over entries as ParsedEntry enum
    pub fn entries(&self) -> impl Iterator<Item = ParsedEntry> + '_ {
        // We need to collect because we can't return different iterator types from match arms
        let entries: Vec<_> = match self {
            #[cfg(feature = "linebased")]
            ParsedWatchFile::LineBased(wf) => wf.entries().map(ParsedEntry::LineBased).collect(),
            #[cfg(feature = "deb822")]
            ParsedWatchFile::Deb822(wf) => wf.entries().map(ParsedEntry::Deb822).collect(),
        };
        entries.into_iter()
    }

    /// Add a new entry to the watch file and return it.
    ///
    /// For v5 (deb822) watch files, this adds a new paragraph with Source and Matching-Pattern fields.
    /// For v1-4 (line-based) watch files, this adds a new entry line.
    ///
    /// Returns a `ParsedEntry` that can be used to query or modify the entry.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "deb822")]
    /// # {
    /// use debian_watch::parse::ParsedWatchFile;
    /// use debian_watch::WatchOption;
    ///
    /// let mut wf = ParsedWatchFile::new(5).unwrap();
    /// let mut entry = wf.add_entry("https://github.com/foo/bar/tags", ".*/v?([\\d.]+)\\.tar\\.gz");
    /// entry.set_option(WatchOption::Component("upstream".to_string()));
    /// # }
    /// ```
    pub fn add_entry(&mut self, source: &str, matching_pattern: &str) -> ParsedEntry {
        match self {
            #[cfg(feature = "linebased")]
            ParsedWatchFile::LineBased(wf) => {
                let entry = crate::linebased::EntryBuilder::new(source)
                    .matching_pattern(matching_pattern)
                    .build();
                let added_entry = wf.add_entry(entry);
                ParsedEntry::LineBased(added_entry)
            }
            #[cfg(feature = "deb822")]
            ParsedWatchFile::Deb822(wf) => {
                let added_entry = wf.add_entry(source, matching_pattern);
                ParsedEntry::Deb822(added_entry)
            }
        }
    }

    /// Byte range of the version declaration.
    ///
    /// In line-based files this is the `version=N` directive on the
    /// first line; in deb822 files it's the `Version:` entry on the
    /// header paragraph. Returns `None` when the file has no version
    /// declaration (legal for v1 line-based files; unusual for v5).
    pub fn version_range(&self) -> Option<rowan::TextRange> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedWatchFile::LineBased(wf) => wf.version_node().map(|v| v.text_range()),
            #[cfg(feature = "deb822")]
            ParsedWatchFile::Deb822(wf) => {
                // The header paragraph in v5 carries `Version:`; it's
                // the first paragraph in the deb822 document.
                let first = wf.as_deb822().paragraphs().next()?;
                first.get_entry("Version").map(|e| e.text_range())
            }
        }
    }
}

impl ParsedEntry {
    /// Get the URL/Source of the entry
    pub fn url(&self) -> String {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.url(),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => e.source().unwrap_or(None).unwrap_or_default(),
        }
    }

    /// Get the matching pattern
    pub fn matching_pattern(&self) -> Option<String> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.matching_pattern(),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => e.matching_pattern().unwrap_or(None),
        }
    }

    /// Get a generic option/field value by key (case-insensitive)
    ///
    /// This handles the difference between line-based format (lowercase keys)
    /// and deb822 format (capitalized keys). It tries the key as-is first,
    /// then tries with the first letter capitalized.
    pub fn get_option(&self, key: &str) -> Option<String> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.get_option(key),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => {
                // Try exact match first, then try capitalized
                e.get_field(key).or_else(|| {
                    let mut chars = key.chars();
                    if let Some(first) = chars.next() {
                        let capitalized = first.to_uppercase().chain(chars).collect::<String>();
                        e.get_field(&capitalized)
                    } else {
                        None
                    }
                })
            }
        }
    }

    /// Check if an option/field is set (case-insensitive)
    pub fn has_option(&self, key: &str) -> bool {
        self.get_option(key).is_some()
    }

    /// Byte range of the source URL within the buffer.
    ///
    /// In line-based format this covers the URL token; in deb822 format
    /// it covers the `Source:` (or `URL:`) entry as a whole — key,
    /// separator, and value. Returns `None` when the entry has no
    /// recognisable source.
    pub fn url_range(&self) -> Option<rowan::TextRange> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.url_node().map(|n| n.text_range()),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => deb822_field_range(e.as_deb822(), &["Source", "URL"]),
        }
    }

    /// Byte range of the matching-pattern within the buffer.
    ///
    /// Returns `None` when the entry has no matching pattern (either
    /// not yet set, or the entry is a template).
    pub fn matching_pattern_range(&self) -> Option<rowan::TextRange> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.matching_pattern_node().map(|n| n.text_range()),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => deb822_field_range(e.as_deb822(), &["Matching-Pattern"]),
        }
    }

    /// Byte range of the named option's `key=value` pair (line-based)
    /// or `Key: value` entry (deb822).
    ///
    /// `key` is matched case-insensitively, mirroring `get_option`.
    /// Returns `None` if the option is unset.
    pub fn option_range(&self, key: &str) -> Option<rowan::TextRange> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => {
                let list = e.option_list()?;
                let opt = list.find_option(key)?;
                Some(opt.text_range())
            }
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => {
                // Try the key as-is, then capitalised — same shape as
                // `get_option`, since deb822 uses `Component` /
                // `Mode` / `Pgpsigurlmangle` while line-based uses
                // lowercase.
                if let Some(r) = deb822_field_range(e.as_deb822(), &[key]) {
                    return Some(r);
                }
                let mut chars = key.chars();
                if let Some(first) = chars.next() {
                    let capitalized = first.to_uppercase().chain(chars).collect::<String>();
                    deb822_field_range(e.as_deb822(), &[capitalized.as_str()])
                } else {
                    None
                }
            }
        }
    }

    /// Byte range of the version-policy / `version=...` part of the
    /// entry, in line-based files. Returns `None` when not set, or when
    /// this is a deb822 entry (per-file `Version:` lives on the header
    /// paragraph, not on individual entries — use
    /// [`ParsedWatchFile::version_range`] for that).
    pub fn version_policy_range(&self) -> Option<rowan::TextRange> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.version_node().map(|n| n.text_range()),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(_) => None,
        }
    }

    /// Byte range of the `Template:` field in this entry, when the
    /// entry uses one. Templates are a v5 (deb822) feature only;
    /// line-based entries always return `None`.
    pub fn template_range(&self) -> Option<rowan::TextRange> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(_) => None,
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => deb822_field_range(e.as_deb822(), &["Template"]),
        }
    }

    /// Template kind for this entry (e.g. `"GitHub"`, `"PyPI"`,
    /// `"CRAN"`), if the entry uses one. Line-based entries always
    /// return `None`.
    pub fn template_kind(&self) -> Option<String> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(_) => None,
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => e.as_deb822().get("Template"),
        }
    }

    /// Get the script
    pub fn script(&self) -> Option<String> {
        self.get_option("script")
    }

    /// Get the component name (empty for main paragraph)
    pub fn component(&self) -> Option<String> {
        self.get_option("component")
    }

    /// Format the URL with package and component substitution
    pub fn format_url(
        &self,
        package: impl FnOnce() -> String,
        component: impl FnOnce() -> String,
    ) -> Result<url::Url, url::ParseError> {
        crate::subst::subst(&self.url(), package, component).parse()
    }

    /// Get the user agent
    pub fn user_agent(&self) -> Option<String> {
        self.get_option("user-agent")
    }

    /// Get the pagemangle option
    pub fn pagemangle(&self) -> Option<String> {
        self.get_option("pagemangle")
    }

    /// Get the uversionmangle option
    pub fn uversionmangle(&self) -> Option<String> {
        self.get_option("uversionmangle")
    }

    /// Get the downloadurlmangle option
    pub fn downloadurlmangle(&self) -> Option<String> {
        self.get_option("downloadurlmangle")
    }

    /// Get the pgpsigurlmangle option
    pub fn pgpsigurlmangle(&self) -> Option<String> {
        self.get_option("pgpsigurlmangle")
    }

    /// Get the filenamemangle option
    pub fn filenamemangle(&self) -> Option<String> {
        self.get_option("filenamemangle")
    }

    /// Get the oversionmangle option
    pub fn oversionmangle(&self) -> Option<String> {
        self.get_option("oversionmangle")
    }

    /// Get the searchmode, with default fallback
    pub fn searchmode(&self) -> crate::types::SearchMode {
        self.get_option("searchmode")
            .and_then(|s| s.parse().ok())
            .unwrap_or_default()
    }

    /// Set an option/field value using a WatchOption enum.
    ///
    /// For v5 (deb822) entries, this sets a field in the paragraph.
    /// For v1-4 (line-based) entries, this sets an option in the opts= list.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "linebased")]
    /// # {
    /// use debian_watch::parse::ParsedWatchFile;
    /// use debian_watch::{WatchOption, Compression};
    ///
    /// let mut wf = ParsedWatchFile::new(4).unwrap();
    /// let mut entry = wf.add_entry("https://github.com/foo/bar/tags", ".*/v?([\\d.]+)\\.tar\\.gz");
    /// entry.set_option(WatchOption::Component("upstream".to_string()));
    /// entry.set_option(WatchOption::Compression(Compression::Xz));
    /// assert_eq!(entry.get_option("component"), Some("upstream".to_string()));
    /// assert_eq!(entry.get_option("compression"), Some("xz".to_string()));
    /// # }
    /// ```
    pub fn set_option(&mut self, option: crate::types::WatchOption) {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => {
                e.set_option(option);
            }
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => {
                e.set_option(option);
            }
        }
    }

    /// Set the URL/Source of the entry
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "linebased")]
    /// # {
    /// use debian_watch::parse::ParsedWatchFile;
    ///
    /// let mut wf = ParsedWatchFile::new(4).unwrap();
    /// let mut entry = wf.add_entry("https://github.com/foo/bar/tags", ".*/v?([\\d.]+)\\.tar\\.gz");
    /// entry.set_url("https://github.com/foo/bar/releases");
    /// assert_eq!(entry.url(), "https://github.com/foo/bar/releases");
    /// # }
    /// ```
    pub fn set_url(&mut self, url: &str) {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.set_url(url),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => e.set_source(url),
        }
    }

    /// Set the matching pattern of the entry
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "linebased")]
    /// # {
    /// use debian_watch::parse::ParsedWatchFile;
    ///
    /// let mut wf = ParsedWatchFile::new(4).unwrap();
    /// let mut entry = wf.add_entry("https://github.com/foo/bar/tags", ".*/v?([\\d.]+)\\.tar\\.gz");
    /// entry.set_matching_pattern(".*/release-([\\d.]+)\\.tar\\.gz");
    /// assert_eq!(entry.matching_pattern(), Some(".*/release-([\\d.]+)\\.tar\\.gz".to_string()));
    /// # }
    /// ```
    pub fn set_matching_pattern(&mut self, pattern: &str) {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.set_matching_pattern(pattern),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => e.set_matching_pattern(pattern),
        }
    }

    /// Get the line number (0-indexed) where this entry starts
    ///
    /// For line-based formats (v1-4), this returns the actual line number in the file.
    /// For deb822 format (v5), this returns the line where the paragraph starts.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "linebased")]
    /// # {
    /// use debian_watch::parse::parse;
    ///
    /// let content = "version=4\nhttps://example.com/ .*.tar.gz\nhttps://example2.com/ .*.tar.gz";
    /// let wf = parse(content).unwrap();
    /// let entries: Vec<_> = wf.entries().collect();
    /// assert_eq!(entries[0].line(), 1); // Second line (0-indexed)
    /// assert_eq!(entries[1].line(), 2); // Third line (0-indexed)
    /// # }
    /// ```
    pub fn line(&self) -> usize {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.line(),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => e.line(),
        }
    }

    /// Remove/delete an option from the entry
    ///
    /// For v5 (deb822) entries, this removes a field from the paragraph.
    /// For v1-4 (line-based) entries, this removes an option from the opts= list.
    /// If this is the last option in a line-based entry, the entire opts= declaration is removed.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "linebased")]
    /// # {
    /// use debian_watch::parse::ParsedWatchFile;
    /// use debian_watch::WatchOption;
    ///
    /// let mut wf = ParsedWatchFile::new(4).unwrap();
    /// let mut entry = wf.add_entry("https://github.com/foo/bar/tags", ".*/v?([\\d.]+)\\.tar\\.gz");
    /// entry.set_option(WatchOption::Compression(debian_watch::Compression::Xz));
    /// assert!(entry.has_option("compression"));
    /// entry.remove_option(WatchOption::Compression(debian_watch::Compression::Xz));
    /// assert!(!entry.has_option("compression"));
    /// # }
    /// ```
    pub fn remove_option(&mut self, option: crate::types::WatchOption) {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.del_opt(option),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => e.delete_option(option),
        }
    }

    /// Retrieve the mode of the watch file entry.
    ///
    /// Returns the mode with default fallback to `Mode::LWP` if not specified.
    /// Returns an error if the mode value is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "linebased")]
    /// # {
    /// use debian_watch::parse::ParsedWatchFile;
    /// use debian_watch::{WatchOption, Mode};
    ///
    /// let mut wf = ParsedWatchFile::new(4).unwrap();
    /// let mut entry = wf.add_entry("https://github.com/foo/bar/tags", ".*/v?([\\d.]+)\\.tar\\.gz");
    ///
    /// // Default mode is LWP
    /// assert_eq!(entry.mode().unwrap(), Mode::LWP);
    ///
    /// // Set git mode
    /// entry.set_option(WatchOption::Mode(Mode::Git));
    /// assert_eq!(entry.mode().unwrap(), Mode::Git);
    /// # }
    /// ```
    pub fn mode(&self) -> Result<crate::types::Mode, crate::types::ParseError> {
        match self {
            #[cfg(feature = "linebased")]
            ParsedEntry::LineBased(e) => e.try_mode(),
            #[cfg(feature = "deb822")]
            ParsedEntry::Deb822(e) => e.mode(),
        }
    }
}

/// Look up the byte range of a deb822 entry by trying each name in
/// `names` in order. Returns the first match's range. Used by the
/// watch-file range helpers to handle aliased fields (`Source` vs
/// `URL`) without spelling out two lookups at every call site.
#[cfg(feature = "deb822")]
fn deb822_field_range(
    paragraph: &deb822_lossless::Paragraph,
    names: &[&str],
) -> Option<rowan::TextRange> {
    for name in names {
        if let Some(entry) = paragraph.get_entry(name) {
            return Some(entry.text_range());
        }
    }
    None
}

impl std::fmt::Display for ParsedWatchFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "linebased")]
            ParsedWatchFile::LineBased(wf) => write!(f, "{}", wf),
            #[cfg(feature = "deb822")]
            ParsedWatchFile::Deb822(wf) => write!(f, "{}", wf),
        }
    }
}

/// Parse a watch file with automatic format detection
///
/// This function detects whether the input is line-based (v1-4) or
/// deb822 format (v5) and parses it accordingly, returning a unified
/// ParsedWatchFile enum.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "linebased")]
/// # {
/// use debian_watch::parse::parse;
///
/// let content = "version=4\nhttps://example.com/ .*.tar.gz";
/// let parsed = parse(content).unwrap();
/// assert_eq!(parsed.version(), 4);
/// # }
/// ```
pub fn parse(content: &str) -> Result<ParsedWatchFile, ParseError> {
    let version = detect_version(content).ok_or(ParseError::UnknownVersion)?;

    match version {
        #[cfg(feature = "linebased")]
        WatchFileVersion::LineBased(_v) => {
            let wf: crate::linebased::WatchFile = content.parse().map_err(ParseError::LineBased)?;
            Ok(ParsedWatchFile::LineBased(wf))
        }
        #[cfg(not(feature = "linebased"))]
        WatchFileVersion::LineBased(_v) => Err(ParseError::FeatureNotEnabled(
            "linebased feature required for v1-4 formats".to_string(),
        )),
        #[cfg(feature = "deb822")]
        WatchFileVersion::Deb822 => {
            let wf: crate::deb822::WatchFile = content.parse().map_err(ParseError::Deb822)?;
            Ok(ParsedWatchFile::Deb822(wf))
        }
        #[cfg(not(feature = "deb822"))]
        WatchFileVersion::Deb822 => Err(ParseError::FeatureNotEnabled(
            "deb822 feature required for v5 format".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_version_v1_default() {
        let content = "https://example.com/ .*.tar.gz";
        assert_eq!(
            detect_version(content),
            Some(WatchFileVersion::LineBased(1))
        );
    }

    #[test]
    fn test_detect_version_v4() {
        let content = "version=4\nhttps://example.com/ .*.tar.gz";
        assert_eq!(
            detect_version(content),
            Some(WatchFileVersion::LineBased(4))
        );
    }

    #[test]
    fn test_detect_version_v4_with_spaces() {
        let content = "version = 4\nhttps://example.com/ .*.tar.gz";
        assert_eq!(
            detect_version(content),
            Some(WatchFileVersion::LineBased(4))
        );
    }

    #[test]
    fn test_detect_version_v5() {
        let content = "Version: 5\n\nSource: https://example.com/";
        assert_eq!(detect_version(content), Some(WatchFileVersion::Deb822));
    }

    #[test]
    fn test_detect_version_v5_lowercase() {
        let content = "version: 5\n\nSource: https://example.com/";
        assert_eq!(detect_version(content), Some(WatchFileVersion::Deb822));
    }

    #[test]
    fn test_detect_version_with_leading_comments() {
        let content = "# This is a comment\nversion=4\nhttps://example.com/ .*.tar.gz";
        assert_eq!(
            detect_version(content),
            Some(WatchFileVersion::LineBased(4))
        );
    }

    #[test]
    fn test_detect_version_with_leading_whitespace() {
        let content = "  \n  version=3\nhttps://example.com/ .*.tar.gz";
        assert_eq!(
            detect_version(content),
            Some(WatchFileVersion::LineBased(3))
        );
    }

    #[test]
    fn test_detect_version_v2() {
        let content = "version=2\nhttps://example.com/ .*.tar.gz";
        assert_eq!(
            detect_version(content),
            Some(WatchFileVersion::LineBased(2))
        );
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_parse_linebased() {
        let content = "version=4\nhttps://example.com/ .*.tar.gz";
        let parsed = parse(content).unwrap();
        assert_eq!(parsed.version(), 4);
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_parse_deb822() {
        let content = "Version: 5\n\nSource: https://example.com/\nMatching-Pattern: .*.tar.gz";
        let parsed = parse(content).unwrap();
        assert_eq!(parsed.version(), 5);
    }

    #[cfg(all(feature = "linebased", feature = "deb822"))]
    #[test]
    fn test_parse_both_formats() {
        // Test v4
        let v4_content = "version=4\nhttps://example.com/ .*.tar.gz";
        let v4_parsed = parse(v4_content).unwrap();
        assert_eq!(v4_parsed.version(), 4);

        // Test v5
        let v5_content = "Version: 5\n\nSource: https://example.com/\nMatching-Pattern: .*.tar.gz";
        let v5_parsed = parse(v5_content).unwrap();
        assert_eq!(v5_parsed.version(), 5);
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_parse_roundtrip() {
        let content = "version=4\n# Comment\nhttps://example.com/ .*.tar.gz";
        let parsed = parse(content).unwrap();
        let output = parsed.to_string();

        // Parse again
        let reparsed = parse(&output).unwrap();
        assert_eq!(reparsed.version(), 4);
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_parsed_watch_file_new_v5() {
        let wf = ParsedWatchFile::new(5).unwrap();
        assert_eq!(wf.version(), 5);
        assert_eq!(wf.entries().count(), 0);
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_parsed_watch_file_new_v4() {
        let wf = ParsedWatchFile::new(4).unwrap();
        assert_eq!(wf.version(), 4);
        assert_eq!(wf.entries().count(), 0);
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_parsed_watch_file_add_entry_v5() {
        let mut wf = ParsedWatchFile::new(5).unwrap();
        let mut entry = wf.add_entry("https://github.com/foo/bar/tags", r".*/v?([\d.]+)\.tar\.gz");

        assert_eq!(wf.entries().count(), 1);
        assert_eq!(entry.url(), "https://github.com/foo/bar/tags");
        assert_eq!(
            entry.matching_pattern(),
            Some(r".*/v?([\d.]+)\.tar\.gz".to_string())
        );

        // Test setting options with enum
        entry.set_option(crate::types::WatchOption::Component("upstream".to_string()));
        entry.set_option(crate::types::WatchOption::Compression(
            crate::types::Compression::Xz,
        ));

        assert_eq!(entry.get_option("Component"), Some("upstream".to_string()));
        assert_eq!(entry.get_option("Compression"), Some("xz".to_string()));
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_parsed_watch_file_add_entry_v4() {
        let mut wf = ParsedWatchFile::new(4).unwrap();
        let entry = wf.add_entry("https://github.com/foo/bar/tags", r".*/v?([\d.]+)\.tar\.gz");

        assert_eq!(wf.entries().count(), 1);
        assert_eq!(entry.url(), "https://github.com/foo/bar/tags");
        assert_eq!(
            entry.matching_pattern(),
            Some(r".*/v?([\d.]+)\.tar\.gz".to_string())
        );
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_parsed_watch_file_roundtrip_with_add_entry() {
        let mut wf = ParsedWatchFile::new(5).unwrap();
        let mut entry = wf.add_entry(
            "https://github.com/owner/repo/tags",
            r".*/v?([\d.]+)\.tar\.gz",
        );
        entry.set_option(crate::types::WatchOption::Compression(
            crate::types::Compression::Xz,
        ));

        let output = wf.to_string();

        // Parse again
        let reparsed = parse(&output).unwrap();
        assert_eq!(reparsed.version(), 5);

        let entries: Vec<_> = reparsed.entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url(), "https://github.com/owner/repo/tags");
        assert_eq!(entries[0].get_option("Compression"), Some("xz".to_string()));
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_parsed_entry_set_url_v4() {
        let mut wf = ParsedWatchFile::new(4).unwrap();
        let mut entry = wf.add_entry("https://github.com/foo/bar/tags", r".*/v?([\d.]+)\.tar\.gz");

        assert_eq!(entry.url(), "https://github.com/foo/bar/tags");

        entry.set_url("https://github.com/foo/bar/releases");
        assert_eq!(entry.url(), "https://github.com/foo/bar/releases");
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_parsed_entry_set_url_v5() {
        let mut wf = ParsedWatchFile::new(5).unwrap();
        let mut entry = wf.add_entry("https://github.com/foo/bar/tags", r".*/v?([\d.]+)\.tar\.gz");

        assert_eq!(entry.url(), "https://github.com/foo/bar/tags");

        entry.set_url("https://github.com/foo/bar/releases");
        assert_eq!(entry.url(), "https://github.com/foo/bar/releases");
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_parsed_entry_set_matching_pattern_v4() {
        let mut wf = ParsedWatchFile::new(4).unwrap();
        let mut entry = wf.add_entry("https://github.com/foo/bar/tags", r".*/v?([\d.]+)\.tar\.gz");

        assert_eq!(
            entry.matching_pattern(),
            Some(r".*/v?([\d.]+)\.tar\.gz".to_string())
        );

        entry.set_matching_pattern(r".*/release-([\d.]+)\.tar\.gz");
        assert_eq!(
            entry.matching_pattern(),
            Some(r".*/release-([\d.]+)\.tar\.gz".to_string())
        );
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_parsed_entry_set_matching_pattern_v5() {
        let mut wf = ParsedWatchFile::new(5).unwrap();
        let mut entry = wf.add_entry("https://github.com/foo/bar/tags", r".*/v?([\d.]+)\.tar\.gz");

        assert_eq!(
            entry.matching_pattern(),
            Some(r".*/v?([\d.]+)\.tar\.gz".to_string())
        );

        entry.set_matching_pattern(r".*/release-([\d.]+)\.tar\.gz");
        assert_eq!(
            entry.matching_pattern(),
            Some(r".*/release-([\d.]+)\.tar\.gz".to_string())
        );
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_parsed_entry_line_v4() {
        let content = "version=4\nhttps://example.com/ .*.tar.gz\nhttps://example2.com/ .*.tar.gz";
        let wf = parse(content).unwrap();
        let entries: Vec<_> = wf.entries().collect();

        assert_eq!(entries[0].line(), 1); // Second line (0-indexed)
        assert_eq!(entries[1].line(), 2); // Third line (0-indexed)
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_parsed_entry_line_v5() {
        let content = r#"Version: 5

Source: https://example.com/repo1
Matching-Pattern: .*\.tar\.gz

Source: https://example.com/repo2
Matching-Pattern: .*\.tar\.xz
"#;
        let wf = parse(content).unwrap();
        let entries: Vec<_> = wf.entries().collect();

        assert_eq!(entries[0].line(), 2); // Third line (0-indexed)
        assert_eq!(entries[1].line(), 5); // Sixth line (0-indexed)
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_url_range_linebased() {
        let content = "version=4\nhttps://example.com/ .*-([\\d.]+)\\.tar\\.gz\n";
        let wf = parse(content).unwrap();
        let entry = wf.entries().next().unwrap();
        let range = entry.url_range().expect("entry has url");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&content[start..end], "https://example.com/");
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_matching_pattern_range_linebased() {
        let content = "version=4\nhttps://example.com/ .*-([\\d.]+)\\.tar\\.gz\n";
        let wf = parse(content).unwrap();
        let entry = wf.entries().next().unwrap();
        let range = entry.matching_pattern_range().expect("has pattern");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&content[start..end], ".*-([\\d.]+)\\.tar\\.gz");
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_option_range_linebased() {
        let content = "version=4\nopts=mode=git,pretty=raw https://example.com/ .*\n";
        let wf = parse(content).unwrap();
        let entry = wf.entries().next().unwrap();
        let mode = entry.option_range("mode").expect("mode option");
        let start: usize = mode.start().into();
        let end: usize = mode.end().into();
        assert_eq!(&content[start..end], "mode=git");

        let pretty = entry.option_range("pretty").expect("pretty option");
        let start: usize = pretty.start().into();
        let end: usize = pretty.end().into();
        assert_eq!(&content[start..end], "pretty=raw");

        assert!(entry.option_range("not-a-real-option").is_none());
    }

    #[cfg(feature = "linebased")]
    #[test]
    fn test_version_range_linebased() {
        let content = "version=4\nhttps://example.com/ .*\n";
        let wf = parse(content).unwrap();
        let range = wf.version_range().expect("has version");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&content[start..end], "version=4\n");
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_url_range_deb822() {
        let content =
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: .*\\.tar\\.gz\n";
        let wf = parse(content).unwrap();
        let entry = wf.entries().next().unwrap();
        let range = entry.url_range().expect("has source");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        // The range covers the whole `Source: ...` entry, ending after
        // the trailing newline.
        assert_eq!(&content[start..end], "Source: https://example.com/foo\n");
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_matching_pattern_range_deb822() {
        let content =
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: v(.+)\\.tar\\.gz\n";
        let wf = parse(content).unwrap();
        let entry = wf.entries().next().unwrap();
        let range = entry.matching_pattern_range().expect("has pattern");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(
            &content[start..end],
            "Matching-Pattern: v(.+)\\.tar\\.gz\n"
        );
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_option_range_deb822_lookup_capitalises_key() {
        // The line-based format uses `mode=git`; deb822 v5 spells the
        // same option as `Mode: git`. option_range looks up either
        // case, so callers using the line-based naming convention
        // still work against v5 files.
        let content =
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: x\nMode: git\n";
        let wf = parse(content).unwrap();
        let entry = wf.entries().next().unwrap();
        let range = entry.option_range("mode").expect("mode field");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&content[start..end], "Mode: git\n");
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_version_range_deb822() {
        let content =
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: x\n";
        let wf = parse(content).unwrap();
        let range = wf.version_range().expect("has version");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&content[start..end], "Version: 5\n");
    }

    #[cfg(feature = "deb822")]
    #[test]
    fn test_template_range_deb822() {
        let content = "Version: 5\n\nSource: https://github.com/foo/bar\nTemplate: GitHub\n";
        let wf = parse(content).unwrap();
        let entry = wf.entries().next().unwrap();
        let range = entry.template_range().expect("has template");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&content[start..end], "Template: GitHub\n");
        assert_eq!(entry.template_kind(), Some("GitHub".to_string()));
    }
}

/// Thread-safe parse result for watch files, suitable for use in Salsa databases.
///
/// This wrapper provides a thread-safe interface around the parsed watch file,
/// storing either a line-based parse tree or the raw text for deb822 format.
/// The underlying lossless parse trees (based on rowan's GreenNode) are thread-safe.
#[derive(Clone, PartialEq, Eq)]
pub struct Parse {
    inner: ParseInner,
}

#[derive(Clone, PartialEq, Eq)]
enum ParseInner {
    #[cfg(feature = "linebased")]
    LineBased(crate::linebased::Parse<crate::linebased::WatchFile>),
    #[cfg(feature = "deb822")]
    Deb822(deb822_lossless::Parse<deb822_lossless::Deb822>),
}

impl Parse {
    /// Parse a watch file with automatic format detection
    pub fn parse(text: &str) -> Self {
        let version = detect_version(text);

        let inner = match version {
            #[cfg(feature = "linebased")]
            Some(WatchFileVersion::LineBased(_)) => {
                ParseInner::LineBased(crate::linebased::parse_watch_file(text))
            }
            #[cfg(feature = "deb822")]
            Some(WatchFileVersion::Deb822) => {
                ParseInner::Deb822(deb822_lossless::Deb822::parse(text))
            }
            #[cfg(not(feature = "linebased"))]
            Some(WatchFileVersion::LineBased(_)) => {
                // Fallback to storing text if linebased feature is not enabled
                #[cfg(feature = "deb822")]
                {
                    ParseInner::Deb822(deb822_lossless::Deb822::parse(text))
                }
                #[cfg(not(feature = "deb822"))]
                {
                    panic!("No watch file parsing features enabled")
                }
            }
            #[cfg(not(feature = "deb822"))]
            Some(WatchFileVersion::Deb822) => {
                // Fallback to linebased if deb822 feature is not enabled
                #[cfg(feature = "linebased")]
                {
                    ParseInner::LineBased(crate::linebased::parse_watch_file(text))
                }
                #[cfg(not(feature = "linebased"))]
                {
                    panic!("No watch file parsing features enabled")
                }
            }
            None => {
                // Default to linebased v1 if we can't detect
                #[cfg(feature = "linebased")]
                {
                    ParseInner::LineBased(crate::linebased::parse_watch_file(text))
                }
                #[cfg(not(feature = "linebased"))]
                #[cfg(feature = "deb822")]
                {
                    ParseInner::Deb822(deb822_lossless::Deb822::parse(text))
                }
                #[cfg(not(any(feature = "linebased", feature = "deb822")))]
                {
                    panic!("No watch file parsing features enabled")
                }
            }
        };

        Parse { inner }
    }

    /// Get the parsed watch file
    pub fn to_watch_file(&self) -> ParsedWatchFile {
        match &self.inner {
            #[cfg(feature = "linebased")]
            ParseInner::LineBased(parse) => ParsedWatchFile::LineBased(parse.tree()),
            #[cfg(feature = "deb822")]
            ParseInner::Deb822(parse) => {
                let deb822 = parse.tree();
                ParsedWatchFile::Deb822(crate::deb822::WatchFile::from_deb822(deb822))
            }
        }
    }

    /// Get the version of the watch file
    pub fn version(&self) -> u32 {
        match &self.inner {
            #[cfg(feature = "linebased")]
            ParseInner::LineBased(parse) => parse.tree().version(),
            #[cfg(feature = "deb822")]
            ParseInner::Deb822(_) => 5,
        }
    }
}

// Implement Send + Sync since the underlying types are thread-safe
// Both variants store GreenNode (thread-safe) via their Parse types
unsafe impl Send for Parse {}
unsafe impl Sync for Parse {}
