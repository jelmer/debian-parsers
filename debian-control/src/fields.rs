//! Fields for the control file
use std::str::FromStr;

/// Priority of a package
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub enum Priority {
    /// Required
    Required,

    /// Important
    Important,

    /// Standard
    Standard,

    /// Optional
    Optional,

    /// Extra
    Extra,

    /// Source
    ///
    /// Note: This priority is not officially documented in Debian policy,
    /// but is commonly used to indicate source packages.
    ///
    /// While packages generally follow the priority values defined in policy, for source packages
    /// the archive-management software (such as dak, the
    /// Debian Archive Kit) may set "Priority: source".
    Source,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Priority::Required => "required",
            Priority::Important => "important",
            Priority::Standard => "standard",
            Priority::Optional => "optional",
            Priority::Extra => "extra",
            Priority::Source => "source",
        })
    }
}

impl std::str::FromStr for Priority {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "required" => Ok(Priority::Required),
            "important" => Ok(Priority::Important),
            "standard" => Ok(Priority::Standard),
            "optional" => Ok(Priority::Optional),
            "extra" => Ok(Priority::Extra),
            "source" => Ok(Priority::Source),
            _ => Err(format!("Invalid priority: {}", s)),
        }
    }
}

/// A checksum of a file
pub trait Checksum {
    /// Filename
    fn filename(&self) -> &str;

    /// Size of the file, in bytes
    fn size(&self) -> usize;
}

/// SHA1 checksum
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Sha1Checksum {
    /// SHA1 checksum
    pub sha1: String,

    /// Size of the file, in bytes
    pub size: usize,

    /// Filename
    pub filename: String,
}

impl Checksum for Sha1Checksum {
    fn filename(&self) -> &str {
        &self.filename
    }

    fn size(&self) -> usize {
        self.size
    }
}

impl std::fmt::Display for Sha1Checksum {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} {} {}", self.sha1, self.size, self.filename)
    }
}

impl std::str::FromStr for Sha1Checksum {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();
        let sha1 = parts.next().ok_or_else(|| "Missing sha1".to_string())?;
        let size = parts
            .next()
            .ok_or_else(|| "Missing size".to_string())?
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let filename = parts
            .next()
            .ok_or_else(|| "Missing filename".to_string())?
            .to_string();
        Ok(Self {
            sha1: sha1.to_string(),
            size,
            filename,
        })
    }
}

/// SHA-256 checksum
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Sha256Checksum {
    /// SHA-256 checksum
    pub sha256: String,

    /// Size of the file, in bytes
    pub size: usize,

    /// Filename
    pub filename: String,
}

impl Checksum for Sha256Checksum {
    fn filename(&self) -> &str {
        &self.filename
    }

    fn size(&self) -> usize {
        self.size
    }
}

impl std::fmt::Display for Sha256Checksum {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} {} {}", self.sha256, self.size, self.filename)
    }
}

impl std::str::FromStr for Sha256Checksum {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();
        let sha256 = parts.next().ok_or_else(|| "Missing sha256".to_string())?;
        let size = parts
            .next()
            .ok_or_else(|| "Missing size".to_string())?
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let filename = parts
            .next()
            .ok_or_else(|| "Missing filename".to_string())?
            .to_string();
        Ok(Self {
            sha256: sha256.to_string(),
            size,
            filename,
        })
    }
}

/// SHA-512 checksum
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Sha512Checksum {
    /// SHA-512 checksum
    pub sha512: String,

    /// Size of the file, in bytes
    pub size: usize,

    /// Filename
    pub filename: String,
}

impl Checksum for Sha512Checksum {
    fn filename(&self) -> &str {
        &self.filename
    }

    fn size(&self) -> usize {
        self.size
    }
}

impl std::fmt::Display for Sha512Checksum {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} {} {}", self.sha512, self.size, self.filename)
    }
}

impl std::str::FromStr for Sha512Checksum {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();
        let sha512 = parts.next().ok_or_else(|| "Missing sha512".to_string())?;
        let size = parts
            .next()
            .ok_or_else(|| "Missing size".to_string())?
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let filename = parts
            .next()
            .ok_or_else(|| "Missing filename".to_string())?
            .to_string();
        Ok(Self {
            sha512: sha512.to_string(),
            size,
            filename,
        })
    }
}

/// An MD5 checksum of a file
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Md5Checksum {
    /// The MD5 checksum
    pub md5sum: String,
    /// The size of the file
    pub size: usize,
    /// The filename
    pub filename: String,
}

impl std::fmt::Display for Md5Checksum {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} {} {}", self.md5sum, self.size, self.filename)
    }
}

impl std::str::FromStr for Md5Checksum {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();
        let md5sum = parts.next().ok_or_else(|| "Missing md5sum".to_string())?;
        let size = parts
            .next()
            .ok_or_else(|| "Missing size".to_string())?
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let filename = parts
            .next()
            .ok_or_else(|| "Missing filename".to_string())?
            .to_string();
        Ok(Self {
            md5sum: md5sum.to_string(),
            size,
            filename,
        })
    }
}

impl Checksum for Md5Checksum {
    fn filename(&self) -> &str {
        &self.filename
    }

    fn size(&self) -> usize {
        self.size
    }
}

/// A package list entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageListEntry {
    /// Package name
    pub package: String,

    /// Package type
    pub package_type: String,

    /// Section
    pub section: String,

    /// Priority
    pub priority: Priority,

    /// Extra fields
    pub extra: std::collections::HashMap<String, String>,
}

impl PackageListEntry {
    /// Create a new package list entry
    pub fn new(package: &str, package_type: &str, section: &str, priority: Priority) -> Self {
        Self {
            package: package.to_string(),
            package_type: package_type.to_string(),
            section: section.to_string(),
            priority,
            extra: std::collections::HashMap::new(),
        }
    }
}

impl std::fmt::Display for PackageListEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.package, self.package_type, self.section, self.priority
        )?;
        for (k, v) in &self.extra {
            write!(f, " {}={}", k, v)?;
        }
        Ok(())
    }
}

impl std::str::FromStr for PackageListEntry {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();
        let package = parts
            .next()
            .ok_or_else(|| "Missing package".to_string())?
            .to_string();
        let package_type = parts
            .next()
            .ok_or_else(|| "Missing package type".to_string())?
            .to_string();
        let section = parts
            .next()
            .ok_or_else(|| "Missing section".to_string())?
            .to_string();
        let priority = parts
            .next()
            .ok_or_else(|| "Missing priority".to_string())?
            .parse()?;
        let mut extra = std::collections::HashMap::new();
        for part in parts {
            let mut kv = part.split('=');
            let k = kv
                .next()
                .ok_or_else(|| "Missing key".to_string())?
                .to_string();
            let v = kv
                .next()
                .ok_or_else(|| "Missing value".to_string())?
                .to_string();
            extra.insert(k, v);
        }
        Ok(Self {
            package,
            package_type,
            section,
            priority,
            extra,
        })
    }
}

/// Urgency of a particular package version
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum Urgency {
    /// Low
    #[default]
    Low,
    /// Medium
    Medium,
    /// High
    High,
    /// Emergency
    Emergency,
    /// Critical
    Critical,
}

impl std::fmt::Display for Urgency {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Urgency::Low => f.write_str("low"),
            Urgency::Medium => f.write_str("medium"),
            Urgency::High => f.write_str("high"),
            Urgency::Emergency => f.write_str("emergency"),
            Urgency::Critical => f.write_str("critical"),
        }
    }
}

impl FromStr for Urgency {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(Urgency::Low),
            "medium" => Ok(Urgency::Medium),
            "high" => Ok(Urgency::High),
            "emergency" => Ok(Urgency::Emergency),
            "critical" => Ok(Urgency::Critical),
            _ => Err(format!("invalid urgency: {}", s)),
        }
    }
}

/// Multi-arch policy
#[derive(PartialEq, Eq, Debug, Default, Clone)]
pub enum MultiArch {
    /// Indicates that the package is identical across all architectures. The package can satisfy dependencies for other architectures.
    Same,
    /// The package can be installed alongside the same package of other architectures. It doesn't provide files that conflict with other architectures.
    Foreign,
    /// The package is only for its native architecture and cannot satisfy dependencies for other architectures.
    #[default]
    No,
    /// Similar to "foreign", but the package manager may choose not to install it for foreign architectures if a native package is available.
    Allowed,
}

impl std::str::FromStr for MultiArch {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "same" => Ok(MultiArch::Same),
            "foreign" => Ok(MultiArch::Foreign),
            "no" => Ok(MultiArch::No),
            "allowed" => Ok(MultiArch::Allowed),
            _ => Err(format!("Invalid multiarch: {}", s)),
        }
    }
}

impl std::fmt::Display for MultiArch {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            MultiArch::Same => "same",
            MultiArch::Foreign => "foreign",
            MultiArch::No => "no",
            MultiArch::Allowed => "allowed",
        })
    }
}

/// Format a Debian package description according to Debian policy.
///
/// Package descriptions consist of a short description (synopsis) and a long description.
/// The long description lines are indented with a single space, and empty lines are
/// represented as " ." (space followed by a period).
///
/// # Arguments
///
/// * `short` - The short description (synopsis), typically one line
/// * `long` - The long description, can be multiple lines
///
/// # Returns
///
/// A formatted description string suitable for use in a Debian control file.
///
/// # Examples
///
/// ```
/// use debian_control::fields::format_description;
///
/// let formatted = format_description("A great package", "This package does amazing things.\nIt is very useful.");
/// assert_eq!(formatted, "A great package\n This package does amazing things.\n It is very useful.");
///
/// // Empty lines become " ."
/// let with_empty = format_description("Summary", "First paragraph.\n\nSecond paragraph.");
/// assert_eq!(with_empty, "Summary\n First paragraph.\n .\n Second paragraph.");
/// ```
pub fn format_description(short: &str, long: &str) -> String {
    let mut result = short.to_string();

    for line in long.lines() {
        result.push('\n');
        if line.trim().is_empty() {
            result.push_str(" .");
        } else {
            result.push(' ');
            result.push_str(line);
        }
    }

    result
}

/// Standards-Version field value
///
/// Represents a Debian standards version as a tuple of up to 4 components.
/// Commonly used versions include "3.9.8", "4.6.2", etc.
///
/// # Examples
///
/// ```
/// use debian_control::fields::StandardsVersion;
/// use std::str::FromStr;
///
/// let version = StandardsVersion::from_str("4.6.2").unwrap();
/// assert_eq!(version.major(), 4);
/// assert_eq!(version.minor(), 6);
/// assert_eq!(version.patch(), 2);
/// assert_eq!(version.to_string(), "4.6.2");
///
/// // Versions can be compared
/// let v1 = StandardsVersion::from_str("4.6.2").unwrap();
/// let v2 = StandardsVersion::from_str("4.5.1").unwrap();
/// assert!(v1 > v2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StandardsVersion {
    major: u8,
    minor: u8,
    patch: u8,
    micro: u8,
}

impl StandardsVersion {
    /// Create a new standards version
    pub fn new(major: u8, minor: u8, patch: u8, micro: u8) -> Self {
        Self {
            major,
            minor,
            patch,
            micro,
        }
    }

    /// Get the major version component
    pub fn major(&self) -> u8 {
        self.major
    }

    /// Get the minor version component
    pub fn minor(&self) -> u8 {
        self.minor
    }

    /// Get the patch version component
    pub fn patch(&self) -> u8 {
        self.patch
    }

    /// Get the micro version component
    pub fn micro(&self) -> u8 {
        self.micro
    }

    /// Convert to a tuple (major, minor, patch, micro)
    pub fn as_tuple(&self) -> (u8, u8, u8, u8) {
        (self.major, self.minor, self.patch, self.micro)
    }
}

impl std::fmt::Display for StandardsVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if self.micro != 0 {
            write!(
                f,
                "{}.{}.{}.{}",
                self.major, self.minor, self.patch, self.micro
            )
        } else if self.patch != 0 {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        } else if self.minor != 0 {
            write!(f, "{}.{}", self.major, self.minor)
        } else {
            write!(f, "{}", self.major)
        }
    }
}

impl std::str::FromStr for StandardsVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.is_empty() || parts.len() > 4 {
            return Err(format!(
                "Invalid standards version format: {} (expected 1-4 dot-separated components)",
                s
            ));
        }

        let major = parts[0]
            .parse()
            .map_err(|_| format!("Invalid major version: {}", parts[0]))?;
        let minor = if parts.len() > 1 {
            parts[1]
                .parse()
                .map_err(|_| format!("Invalid minor version: {}", parts[1]))?
        } else {
            0
        };
        let patch = if parts.len() > 2 {
            parts[2]
                .parse()
                .map_err(|_| format!("Invalid patch version: {}", parts[2]))?
        } else {
            0
        };
        let micro = if parts.len() > 3 {
            parts[3]
                .parse()
                .map_err(|_| format!("Invalid micro version: {}", parts[3]))?
        } else {
            0
        };

        Ok(Self {
            major,
            minor,
            patch,
            micro,
        })
    }
}

impl PartialOrd for StandardsVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StandardsVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_tuple().cmp(&other.as_tuple())
    }
}

/// Dgit information
///
/// The Dgit field format is: `<commit-hash> <suite> <ref> <url>`
/// For example: `c1370424e2404d3c22bd09c828d4b28d81d897ad debian archive/debian/1.1.0 https://git.dgit.debian.org/cltl`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DgitInfo {
    /// Git commit hash
    pub commit: String,
    /// Suite (e.g., "debian")
    pub suite: String,
    /// Git reference (e.g., "archive/debian/1.1.0")
    pub git_ref: String,
    /// Git repository URL
    pub url: String,
}

impl FromStr for DgitInfo {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() != 4 {
            return Err(format!(
                "Invalid Dgit field format: expected 4 parts (commit suite ref url), got {}",
                parts.len()
            ));
        }
        Ok(Self {
            commit: parts[0].to_string(),
            suite: parts[1].to_string(),
            git_ref: parts[2].to_string(),
            url: parts[3].to_string(),
        })
    }
}

impl std::fmt::Display for DgitInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.commit, self.suite, self.git_ref, self.url
        )
    }
}

/// Source-stanza fields whose value is a list of package relations.
///
/// In declaration order as defined by Debian Policy 7. Listed here so that
/// tools (linters, indexers, syntax highlighters) can enumerate them without
/// duplicating the list.
pub const SOURCE_RELATION_FIELDS: &[&str] = &[
    "Build-Depends",
    "Build-Depends-Indep",
    "Build-Depends-Arch",
    "Build-Conflicts",
    "Build-Conflicts-Indep",
    "Build-Conflicts-Arch",
];

/// Binary-stanza fields whose value is a list of package relations.
///
/// In declaration order as defined by Debian Policy 7.
pub const BINARY_RELATION_FIELDS: &[&str] = &[
    "Depends",
    "Pre-Depends",
    "Recommends",
    "Suggests",
    "Enhances",
    "Breaks",
    "Conflicts",
    "Replaces",
    "Provides",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha1_checksum_filename() {
        let checksum = Sha1Checksum {
            sha1: "abc123".to_string(),
            size: 1234,
            filename: "test.deb".to_string(),
        };
        assert_eq!(checksum.filename(), "test.deb".to_string());
    }

    #[test]
    fn test_md5_checksum_filename() {
        let checksum = Md5Checksum {
            md5sum: "abc123".to_string(),
            size: 1234,
            filename: "test.deb".to_string(),
        };
        assert_eq!(checksum.filename(), "test.deb".to_string());
    }

    #[test]
    fn test_sha256_checksum_filename() {
        let checksum = Sha256Checksum {
            sha256: "abc123".to_string(),
            size: 1234,
            filename: "test.deb".to_string(),
        };
        assert_eq!(checksum.filename(), "test.deb".to_string());
    }

    #[test]
    fn test_sha512_checksum_filename() {
        let checksum = Sha512Checksum {
            sha512: "abc123".to_string(),
            size: 1234,
            filename: "test.deb".to_string(),
        };
        assert_eq!(checksum.filename(), "test.deb".to_string());
    }

    #[test]
    fn test_format_description_basic() {
        let formatted = format_description(
            "A great package",
            "This package does amazing things.\nIt is very useful.",
        );
        assert_eq!(
            formatted,
            "A great package\n This package does amazing things.\n It is very useful."
        );
    }

    #[test]
    fn test_format_description_empty_lines() {
        let formatted = format_description("Summary", "First paragraph.\n\nSecond paragraph.");
        assert_eq!(
            formatted,
            "Summary\n First paragraph.\n .\n Second paragraph."
        );
    }

    #[test]
    fn test_format_description_short_only() {
        let formatted = format_description("Short description", "");
        assert_eq!(formatted, "Short description");
    }

    #[test]
    fn test_format_description_multiple_empty_lines() {
        let formatted = format_description("Test", "Line 1\n\n\nLine 2");
        assert_eq!(formatted, "Test\n Line 1\n .\n .\n Line 2");
    }

    #[test]
    fn test_format_description_whitespace_only_line() {
        let formatted = format_description("Test", "Line 1\n   \nLine 2");
        assert_eq!(formatted, "Test\n Line 1\n .\n Line 2");
    }

    #[test]
    fn test_format_description_complex() {
        let long_desc = "This is a test package.\n\nIt has multiple paragraphs.\n\nAnd even lists:\n - Item 1\n - Item 2";
        let formatted = format_description("Test package", long_desc);
        assert_eq!(
            formatted,
            "Test package\n This is a test package.\n .\n It has multiple paragraphs.\n .\n And even lists:\n  - Item 1\n  - Item 2"
        );
    }

    #[test]
    fn test_standards_version_parse() {
        let v = "4.6.2".parse::<StandardsVersion>().unwrap();
        assert_eq!(v.major(), 4);
        assert_eq!(v.minor(), 6);
        assert_eq!(v.patch(), 2);
        assert_eq!(v.micro(), 0);
        assert_eq!(v.as_tuple(), (4, 6, 2, 0));
    }

    #[test]
    fn test_standards_version_parse_two_components() {
        let v = "3.9".parse::<StandardsVersion>().unwrap();
        assert_eq!(v.major(), 3);
        assert_eq!(v.minor(), 9);
        assert_eq!(v.patch(), 0);
        assert_eq!(v.micro(), 0);
    }

    #[test]
    fn test_standards_version_parse_four_components() {
        let v = "4.6.2.1".parse::<StandardsVersion>().unwrap();
        assert_eq!(v.major(), 4);
        assert_eq!(v.minor(), 6);
        assert_eq!(v.patch(), 2);
        assert_eq!(v.micro(), 1);
    }

    #[test]
    fn test_standards_version_parse_single_component() {
        let v = "4".parse::<StandardsVersion>().unwrap();
        assert_eq!(v.major(), 4);
        assert_eq!(v.minor(), 0);
        assert_eq!(v.patch(), 0);
        assert_eq!(v.micro(), 0);
    }

    #[test]
    fn test_standards_version_display() {
        let v = StandardsVersion::new(4, 6, 2, 0);
        assert_eq!(v.to_string(), "4.6.2");

        let v = StandardsVersion::new(3, 9, 8, 0);
        assert_eq!(v.to_string(), "3.9.8");

        let v = StandardsVersion::new(4, 6, 2, 1);
        assert_eq!(v.to_string(), "4.6.2.1");

        let v = StandardsVersion::new(3, 9, 0, 0);
        assert_eq!(v.to_string(), "3.9");

        let v = StandardsVersion::new(4, 0, 0, 0);
        assert_eq!(v.to_string(), "4");
    }

    #[test]
    fn test_standards_version_comparison() {
        let v1 = "4.6.2".parse::<StandardsVersion>().unwrap();
        let v2 = "4.5.1".parse::<StandardsVersion>().unwrap();
        assert!(v1 > v2);

        let v3 = "4.6.2".parse::<StandardsVersion>().unwrap();
        assert_eq!(v1, v3);

        let v4 = "3.9.8".parse::<StandardsVersion>().unwrap();
        assert!(v1 > v4);

        let v5 = "4.6.2.1".parse::<StandardsVersion>().unwrap();
        assert!(v5 > v1);
    }

    #[test]
    fn test_standards_version_roundtrip() {
        let versions = vec!["4.6.2", "3.9.8", "4.6.2.1", "3.9", "4"];
        for version_str in versions {
            let v = version_str.parse::<StandardsVersion>().unwrap();
            assert_eq!(v.to_string(), version_str);
        }
    }

    #[test]
    fn test_standards_version_invalid() {
        assert!("".parse::<StandardsVersion>().is_err());
        assert!("a.b.c".parse::<StandardsVersion>().is_err());
        assert!("1.2.3.4.5".parse::<StandardsVersion>().is_err());
        assert!("1.2.3.-1".parse::<StandardsVersion>().is_err());
    }

    #[test]
    fn test_dgit_info_parse() {
        let input = "c1370424e2404d3c22bd09c828d4b28d81d897ad debian archive/debian/1.1.0 https://git.dgit.debian.org/cltl";
        let dgit: DgitInfo = input.parse().unwrap();
        assert_eq!(dgit.commit, "c1370424e2404d3c22bd09c828d4b28d81d897ad");
        assert_eq!(dgit.suite, "debian");
        assert_eq!(dgit.git_ref, "archive/debian/1.1.0");
        assert_eq!(dgit.url, "https://git.dgit.debian.org/cltl");
    }

    #[test]
    fn test_dgit_info_display() {
        let dgit = DgitInfo {
            commit: "c1370424e2404d3c22bd09c828d4b28d81d897ad".to_string(),
            suite: "debian".to_string(),
            git_ref: "archive/debian/1.1.0".to_string(),
            url: "https://git.dgit.debian.org/cltl".to_string(),
        };
        let output = dgit.to_string();
        assert_eq!(
            output,
            "c1370424e2404d3c22bd09c828d4b28d81d897ad debian archive/debian/1.1.0 https://git.dgit.debian.org/cltl"
        );
    }

    #[test]
    fn test_dgit_info_roundtrip() {
        let original = "90f40df9c40b0ceb59c207bcbec0a729e90d7ea9 debian archive/debian/1.0.debian1-5 https://git.dgit.debian.org/crafty-books-medium";
        let dgit: DgitInfo = original.parse().unwrap();
        assert_eq!(dgit.to_string(), original);
    }

    #[test]
    fn test_dgit_info_invalid() {
        // Too few parts
        assert!("abc123 debian".parse::<DgitInfo>().is_err());
        // Too many parts
        assert!("abc123 debian ref url extra".parse::<DgitInfo>().is_err());
        // Empty string
        assert!("".parse::<DgitInfo>().is_err());
    }
}
