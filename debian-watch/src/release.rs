//! Types for representing discovered releases.

use debversion::Version;
use std::cmp::Ordering;

/// A discovered release from an upstream source
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The version string of the release (after uversionmangle)
    pub version: String,
    /// The URL to download the release tarball (after downloadurlmangle)
    pub url: String,
    /// Optional URL to the PGP signature file
    pub pgpsigurl: Option<String>,
    /// Optional target filename for the downloaded tarball (from filenamemangle)
    pub target_filename: Option<String>,
    /// Optional Debian package version (from oversionmangle, e.g., "1.0+dfsg")
    pub package_version: Option<String>,
}

impl Release {
    /// Create a new Release
    ///
    /// # Examples
    ///
    /// ```
    /// use debian_watch::Release;
    ///
    /// let release = Release::new("1.0.0", "https://example.com/project-1.0.0.tar.gz", None);
    /// assert_eq!(release.version, "1.0.0");
    /// assert_eq!(release.url, "https://example.com/project-1.0.0.tar.gz");
    /// ```
    pub fn new(
        version: impl Into<String>,
        url: impl Into<String>,
        pgpsigurl: Option<String>,
    ) -> Self {
        Self {
            version: version.into(),
            url: url.into(),
            pgpsigurl,
            target_filename: None,
            package_version: None,
        }
    }

    /// Create a new Release with all fields
    ///
    /// # Examples
    ///
    /// ```
    /// use debian_watch::Release;
    ///
    /// let release = Release::new_full(
    ///     "1.0.0",
    ///     "https://example.com/project-1.0.0.tar.gz",
    ///     Some("https://example.com/project-1.0.0.tar.gz.asc".to_string()),
    ///     Some("myproject_1.0.0.orig.tar.gz".to_string()),
    ///     Some("1.0.0+dfsg".to_string()),
    /// );
    /// assert_eq!(release.version, "1.0.0");
    /// assert_eq!(release.target_filename, Some("myproject_1.0.0.orig.tar.gz".to_string()));
    /// ```
    pub fn new_full(
        version: impl Into<String>,
        url: impl Into<String>,
        pgpsigurl: Option<String>,
        target_filename: Option<String>,
        package_version: Option<String>,
    ) -> Self {
        Self {
            version: version.into(),
            url: url.into(),
            pgpsigurl,
            target_filename,
            package_version,
        }
    }

    /// Download the release tarball (async version)
    ///
    /// Downloads the tarball from the release URL.
    /// Requires the 'discover' feature.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use debian_watch::Release;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let release = Release::new("1.0.0", "https://example.com/project-1.0.tar.gz", None);
    /// let data = release.download().await?;
    /// println!("Downloaded {} bytes", data.len());
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "discover")]
    pub async fn download(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        let response = client.get(&self.url).send().await?;
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Download the release tarball (blocking version)
    ///
    /// Downloads the tarball from the release URL.
    /// Requires both 'discover' and 'blocking' features.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use debian_watch::Release;
    ///
    /// let release = Release::new("1.0.0", "https://example.com/project-1.0.tar.gz", None);
    /// let data = release.download_blocking()?;
    /// println!("Downloaded {} bytes", data.len());
    /// ```
    #[cfg(all(feature = "discover", feature = "blocking"))]
    pub fn download_blocking(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let client = reqwest::blocking::Client::new();
        let response = client.get(&self.url).send()?;
        let bytes = response.bytes()?;
        Ok(bytes.to_vec())
    }
}

impl PartialOrd for Release {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Release {
    fn cmp(&self, other: &Self) -> Ordering {
        // Parse versions and compare them
        match (
            self.version.parse::<Version>(),
            other.version.parse::<Version>(),
        ) {
            (Ok(v1), Ok(v2)) => v1.cmp(&v2),
            // If parsing fails, fall back to string comparison
            _ => self.version.cmp(&other.version),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_release_new() {
        let release = Release::new("1.0.0", "https://example.com/foo.tar.gz", None);
        assert_eq!(release.version, "1.0.0");
        assert_eq!(release.url, "https://example.com/foo.tar.gz");
        assert_eq!(release.pgpsigurl, None);

        let release = Release::new(
            "2.0.0",
            "https://example.com/foo-2.0.0.tar.gz",
            Some("https://example.com/foo-2.0.0.tar.gz.asc".to_string()),
        );
        assert_eq!(release.version, "2.0.0");
        assert_eq!(
            release.pgpsigurl,
            Some("https://example.com/foo-2.0.0.tar.gz.asc".to_string())
        );
    }

    #[test]
    fn test_release_ordering() {
        let r1 = Release::new("1.0.0", "https://example.com/foo-1.0.0.tar.gz", None);
        let r2 = Release::new("2.0.0", "https://example.com/foo-2.0.0.tar.gz", None);
        let r3 = Release::new("1.5.0", "https://example.com/foo-1.5.0.tar.gz", None);

        assert!(r1 < r2);
        assert!(r2 > r1);
        assert!(r1 < r3);
        assert!(r3 < r2);
    }

    #[test]
    fn test_release_ordering_debian_versions() {
        // Test with Debian version strings
        let r1 = Release::new("1.0", "https://example.com/foo-1.0.tar.gz", None);
        let r2 = Release::new("1.0+dfsg", "https://example.com/foo-1.0+dfsg.tar.gz", None);
        let r3 = Release::new("1.0~rc1", "https://example.com/foo-1.0~rc1.tar.gz", None);

        // 1.0~rc1 < 1.0 < 1.0+dfsg in Debian version ordering
        assert!(r3 < r1);
        assert!(r1 < r2);
    }

    #[test]
    fn test_release_max() {
        let releases = vec![
            Release::new("1.0.0", "https://example.com/foo-1.0.0.tar.gz", None),
            Release::new("2.0.0", "https://example.com/foo-2.0.0.tar.gz", None),
            Release::new("1.5.0", "https://example.com/foo-1.5.0.tar.gz", None),
        ];

        let max = releases.iter().max().unwrap();
        assert_eq!(max.version, "2.0.0");
    }
}
