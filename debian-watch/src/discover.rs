//! Discover upstream releases from watch file entries
//!
//! This module provides methods for discovering upstream releases by fetching URLs
//! and searching for version patterns.

use crate::parse::{ParsedEntry, ParsedWatchFile};
use crate::release::Release;
use crate::DEFAULT_USER_AGENT;
use std::error::Error;

/// Default matching pattern used when none is specified
/// Expands to: (?:package-name)?[-_]?(\d[\-+\.:\~\da-zA-Z]*)(?i)\.(?:tar\.xz|tar\.bz2|tar\.gz|zip|tgz|tbz|txz)
const DEFAULT_MATCHING_PATTERN: &str = "(?:@PACKAGE@)?@ANY_VERSION@@ARCHIVE_EXT@";

/// Error type for discovery operations
#[derive(Debug)]
pub enum DiscoveryError {
    /// HTTP request failed
    HttpError(reqwest::Error),
    /// Pattern matching failed
    PatternError(MangleError),
    /// Missing required field
    MissingField(String),
    /// URL parsing error
    UrlError(url::ParseError),
    /// IO error
    IoError(std::io::Error),
}

use crate::mangle::MangleError;

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DiscoveryError::HttpError(e) => write!(f, "HTTP error: {}", e),
            DiscoveryError::PatternError(e) => write!(f, "Pattern error: {}", e),
            DiscoveryError::MissingField(msg) => write!(f, "Missing field: {}", msg),
            DiscoveryError::UrlError(e) => write!(f, "URL error: {}", e),
            DiscoveryError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            DiscoveryError::HttpError(e) => Some(e),
            DiscoveryError::PatternError(e) => Some(e),
            DiscoveryError::MissingField(_) => None,
            DiscoveryError::UrlError(e) => Some(e),
            DiscoveryError::IoError(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for DiscoveryError {
    fn from(err: std::io::Error) -> Self {
        DiscoveryError::IoError(err)
    }
}

impl From<reqwest::Error> for DiscoveryError {
    fn from(err: reqwest::Error) -> Self {
        DiscoveryError::HttpError(err)
    }
}

impl From<url::ParseError> for DiscoveryError {
    fn from(err: url::ParseError) -> Self {
        DiscoveryError::UrlError(err)
    }
}

impl From<MangleError> for DiscoveryError {
    fn from(err: MangleError) -> Self {
        DiscoveryError::PatternError(err)
    }
}

impl ParsedEntry {
    /// Discover all available releases for this watch entry (async version)
    ///
    /// Fetches the URL specified in the watch entry and searches for releases
    /// matching the configured pattern.
    ///
    /// # Arguments
    ///
    /// * `package` - Closure that returns the package name to use for substitution in URLs
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use debian_watch::parse::ParsedWatchFile;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let wf: ParsedWatchFile = debian_watch::parse::parse(r#"version=4
    /// https://example.com/files .*/v?(\\d\\S+)\\.tar\\.gz
    /// "#)?;
    ///
    /// let entry = wf.entries().next().unwrap();
    /// let releases = entry.discover(|| "mypackage".to_string()).await?;
    /// for release in releases {
    ///     println!("Found version: {}", release.version);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn discover(
        &self,
        package: impl FnOnce() -> String + Send,
    ) -> Result<Vec<Release>, DiscoveryError> {
        self.discover_impl(package, None).await
    }

    /// Discover all available releases with a custom HTTP client (async version)
    ///
    /// This is the same as `discover()` but allows providing a custom reqwest client
    /// for more control over HTTP requests.
    pub async fn discover_with_client(
        &self,
        package: impl FnOnce() -> String + Send,
        client: &reqwest::Client,
    ) -> Result<Vec<Release>, DiscoveryError> {
        self.discover_impl(package, Some(client)).await
    }

    /// Internal implementation for discovering releases
    async fn discover_impl(
        &self,
        package: impl FnOnce() -> String + Send,
        client: Option<&reqwest::Client>,
    ) -> Result<Vec<Release>, DiscoveryError> {
        let component = self.component().unwrap_or_default();
        let url = self.format_url(package, || component.clone())?;
        let user_agent = self
            .user_agent()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

        // Build HTTP client if not provided
        let default_client;
        let http_client = if let Some(c) = client {
            c
        } else {
            default_client = reqwest::Client::builder().user_agent(user_agent).build()?;
            &default_client
        };

        // Fetch the URL
        let response = http_client.get(url.as_str()).send().await?;

        let body = response.bytes().await?;

        // Apply pagemangle if present
        let mangled_body = if let Some(mangle) = self.pagemangle() {
            let page_str = String::from_utf8_lossy(&body);
            let result = crate::mangle::apply_mangle(&mangle, &page_str)?;
            result.into_bytes()
        } else {
            body.to_vec()
        };

        // Get the matching pattern, using default if not specified
        let pattern_str = self
            .matching_pattern()
            .unwrap_or_else(|| DEFAULT_MATCHING_PATTERN.to_string());

        // Apply substitution to the matching pattern
        let package_name = String::new();
        let component_name = String::new();
        let pattern = crate::subst::subst(
            &pattern_str,
            || package_name.clone(),
            || component_name.clone(),
        );

        // Determine search mode
        let searchmode = self.searchmode();
        let searchmode_str = searchmode.to_string();

        // Search for versions
        let results = crate::search::search(
            &searchmode_str,
            std::io::Cursor::new(mangled_body.as_ref() as &[u8]),
            &pattern,
            &package_name,
            url.as_str(),
        )?;

        // Apply mangles to each result and convert to Release objects
        let mut releases = Vec::new();
        for (version, full_url) in results {
            // Apply uversionmangle
            let mangled_version = if let Some(mangle) = self.uversionmangle() {
                crate::mangle::apply_mangle(&mangle, &version)?
            } else {
                version
            };

            // Apply downloadurlmangle
            let mangled_url = if let Some(mangle) = self.downloadurlmangle() {
                crate::mangle::apply_mangle(&mangle, &full_url)?
            } else {
                full_url
            };

            // Apply pgpsigurlmangle if present
            let pgpsigurl = if let Some(mangle) = self.pgpsigurlmangle() {
                Some(crate::mangle::apply_mangle(&mangle, &mangled_url)?)
            } else {
                None
            };

            // Apply filenamemangle if present
            let target_filename = if let Some(mangle) = self.filenamemangle() {
                Some(crate::mangle::apply_mangle(&mangle, &mangled_url)?)
            } else {
                None
            };

            // Apply oversionmangle if present
            let package_version = if let Some(mangle) = self.oversionmangle() {
                Some(crate::mangle::apply_mangle(&mangle, &mangled_version)?)
            } else {
                None
            };

            releases.push(Release::new_full(
                mangled_version,
                mangled_url,
                pgpsigurl,
                target_filename,
                package_version,
            ));
        }

        Ok(releases)
    }

    /// Discover all available releases for this watch entry (blocking version)
    ///
    /// This is the blocking version of `discover()`. Requires the 'blocking' feature.
    #[cfg(feature = "blocking")]
    pub fn discover_blocking(
        &self,
        package: impl FnOnce() -> String,
    ) -> Result<Vec<Release>, DiscoveryError> {
        self.discover_blocking_impl(package, None)
    }

    /// Discover all available releases with a custom HTTP client (blocking version)
    #[cfg(feature = "blocking")]
    pub fn discover_blocking_with_client(
        &self,
        package: impl FnOnce() -> String,
        client: &reqwest::blocking::Client,
    ) -> Result<Vec<Release>, DiscoveryError> {
        self.discover_blocking_impl(package, Some(client))
    }

    /// Internal implementation for blocking discover
    #[cfg(feature = "blocking")]
    fn discover_blocking_impl(
        &self,
        package: impl FnOnce() -> String,
        client: Option<&reqwest::blocking::Client>,
    ) -> Result<Vec<Release>, DiscoveryError> {
        // Get the URL and apply package and component substitution
        let component = self.component().unwrap_or_default();
        let url = self.format_url(package, || component.clone())?;

        // Get user agent
        let user_agent = self
            .user_agent()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

        // Build HTTP client if not provided
        let default_client;
        let http_client = if let Some(c) = client {
            c
        } else {
            default_client = reqwest::blocking::Client::builder()
                .user_agent(user_agent)
                .build()?;
            &default_client
        };

        // Fetch the URL
        let response = http_client.get(url.as_str()).send()?;

        let body = response.bytes()?;

        // Apply pagemangle if present
        let mangled_body = if let Some(mangle) = self.pagemangle() {
            let page_str = String::from_utf8_lossy(&body);
            let result = crate::mangle::apply_mangle(&mangle, &page_str)?;
            result.into_bytes()
        } else {
            body.to_vec()
        };

        // Get the matching pattern, using default if not specified
        let matching_pattern = self
            .matching_pattern()
            .unwrap_or_else(|| DEFAULT_MATCHING_PATTERN.to_string());

        // Apply substitution to the matching pattern
        let package_name = String::new();
        let component_name = String::new();
        let pattern = crate::subst::subst(
            &matching_pattern,
            || package_name.clone(),
            || component_name.clone(),
        );

        // Determine search mode
        let searchmode = self.searchmode();
        let searchmode_str = searchmode.to_string();

        // Search for versions
        let results = crate::search::search(
            &searchmode_str,
            std::io::Cursor::new(mangled_body.as_ref() as &[u8]),
            &pattern,
            &package_name,
            url.as_str(),
        )?;

        // Apply mangles to each result and convert to Release objects
        let mut releases = Vec::new();
        for (version, full_url) in results {
            // Apply uversionmangle
            let mangled_version = if let Some(mangle) = self.uversionmangle() {
                crate::mangle::apply_mangle(&mangle, &version)?
            } else {
                version
            };

            // Apply downloadurlmangle
            let mangled_url = if let Some(mangle) = self.downloadurlmangle() {
                crate::mangle::apply_mangle(&mangle, &full_url)?
            } else {
                full_url
            };

            // Apply pgpsigurlmangle if present
            let pgpsigurl = if let Some(mangle) = self.pgpsigurlmangle() {
                Some(crate::mangle::apply_mangle(&mangle, &mangled_url)?)
            } else {
                None
            };

            // Apply filenamemangle if present
            let target_filename = if let Some(mangle) = self.filenamemangle() {
                Some(crate::mangle::apply_mangle(&mangle, &mangled_url)?)
            } else {
                None
            };

            // Apply oversionmangle if present
            let package_version = if let Some(mangle) = self.oversionmangle() {
                Some(crate::mangle::apply_mangle(&mangle, &mangled_version)?)
            } else {
                None
            };

            releases.push(Release::new_full(
                mangled_version,
                mangled_url,
                pgpsigurl,
                target_filename,
                package_version,
            ));
        }

        Ok(releases)
    }
}

impl ParsedWatchFile {
    /// Discover releases from all entries in the watch file (async version)
    ///
    /// # Arguments
    ///
    /// * `package` - Closure that returns the package name to use for substitution in URLs
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use debian_watch::parse::ParsedWatchFile;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let wf: ParsedWatchFile = debian_watch::parse::parse(r#"version=4
    /// https://example.com/files .*/v?(\\d\\S+)\\.tar\\.gz
    /// "#)?;
    ///
    /// let all_releases = wf.discover_all(|| "mypackage".to_string()).await?;
    /// for (entry_idx, releases) in all_releases.iter().enumerate() {
    ///     println!("Entry {}: {} releases found", entry_idx, releases.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn discover_all(
        &self,
        package: impl Fn() -> String + Send + Clone + 'static,
    ) -> Result<Vec<Vec<Release>>, DiscoveryError> {
        // Collect entries before async block to avoid holding self reference
        let entries: Vec<_> = self.entries().collect();

        let mut all_releases = Vec::new();
        for entry in entries {
            let pkg = package.clone();
            let releases = entry.discover(move || pkg()).await?;
            all_releases.push(releases);
        }
        Ok(all_releases)
    }

    /// Discover releases from all entries in the watch file (blocking version)
    #[cfg(feature = "blocking")]
    pub fn discover_all_blocking(
        &self,
        package: impl Fn() -> String,
    ) -> Result<Vec<Vec<Release>>, DiscoveryError> {
        let mut all_releases = Vec::new();
        for entry in self.entries() {
            let releases = entry.discover_blocking(&package)?;
            all_releases.push(releases);
        }
        Ok(all_releases)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_error_display() {
        let err = DiscoveryError::MissingField("url".to_string());
        assert_eq!(err.to_string(), "Missing field: url");

        let err =
            DiscoveryError::PatternError(MangleError::RegexError("invalid regex".to_string()));
        assert_eq!(err.to_string(), "Pattern error: regex error: invalid regex");
    }

    #[test]
    fn test_default_matching_pattern_value() {
        // Verify the default pattern constant matches uscan's default
        assert_eq!(
            DEFAULT_MATCHING_PATTERN,
            "(?:@PACKAGE@)?@ANY_VERSION@@ARCHIVE_EXT@"
        );
    }

    #[cfg(feature = "blocking")]
    #[test]
    fn test_discover_blocking_with_no_matching_pattern() {
        use crate::parse::parse;
        use std::thread;

        // Start a tiny_http server on a random port
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr();

        let server_thread = thread::spawn(move || {
            let request = server.recv().unwrap();
            let response = tiny_http::Response::from_string(
                "<html><body><a href=\"mypackage-1.2.3.tar.gz\">Download</a></body></html>",
            )
            .with_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap(),
            );
            request.respond(response).unwrap();
        });

        // Create a watch file with no matching pattern
        let watch_content = format!(
            r#"version=4
http://{}
"#,
            addr
        );

        let parsed = parse(&watch_content).unwrap();
        let entries: Vec<_> = parsed.entries().collect();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];

        // Verify matching_pattern is None
        assert_eq!(entry.matching_pattern(), None);

        // Call discover_blocking - should use default pattern and find the release
        let result = entry.discover_blocking(|| "mypackage".to_string());

        server_thread.join().unwrap();

        // Should successfully find the release using default pattern
        let releases = result.expect("discover should succeed with default pattern");
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, "1.2.3");
    }

    #[test]
    fn test_explicit_pattern_still_works() {
        // Ensure that when a pattern IS specified, it's still used (not overridden by default)
        use crate::parse::parse;

        let watch_content = r#"version=4
https://example.com/releases/ custom-pattern-(\d+)\.zip
"#;
        let parsed = parse(watch_content).unwrap();
        let entries: Vec<_> = parsed.entries().collect();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];

        // Verify the explicit pattern is present
        assert_eq!(
            entry.matching_pattern(),
            Some("custom-pattern-(\\d+)\\.zip".to_string())
        );

        // Verify the logic would use the explicit pattern, not the default
        let pattern_str = entry
            .matching_pattern()
            .unwrap_or_else(|| DEFAULT_MATCHING_PATTERN.to_string());

        assert_eq!(pattern_str, "custom-pattern-(\\d+)\\.zip");
    }
}
