//! Functions for searching web pages for upstream releases.

use regex::Regex;
use std::io::Read;

/// Search for version matches in HTML content
///
/// Parses the HTML and searches for links matching the given pattern.
/// Returns an iterator of (version, url) tuples.
///
/// # Arguments
///
/// * `body` - The HTML content to search
/// * `matching_pattern` - Regex pattern to match against URLs
/// * `base_url` - Base URL for resolving relative links
///
/// # Examples
///
/// ```ignore
/// use debian_watch::search::html_search;
///
/// let html = b"<a href='project-1.0.tar.gz'>Download</a>";
/// let results: Vec<_> = html_search(html, r"project-(\d+\.\d+)\.tar\.gz", "https://example.com/")
///     .collect();
/// assert_eq!(results.len(), 1);
/// assert_eq!(results[0].0, "1.0");
/// ```
#[cfg(feature = "discover")]
pub fn html_search(
    body: &[u8],
    matching_pattern: &str,
    base_url: &str,
) -> Box<dyn Iterator<Item = (String, String)>> {
    let html = String::from_utf8_lossy(body);
    let doc = scraper::Html::parse_document(&html);

    // Check for <base href="..."> tag to use as base URL for resolving relative hrefs
    let base_selector = scraper::Selector::parse("base").unwrap();
    let effective_base_url = doc
        .select(&base_selector)
        .filter_map(|element| element.value().attr("href"))
        .next()
        .unwrap_or(base_url);

    let base_url_parsed = match url::Url::parse(effective_base_url) {
        Ok(u) => u,
        Err(_) => return Box::new(std::iter::empty()),
    };
    let selector = scraper::Selector::parse("a").unwrap();

    let re = match Regex::new(matching_pattern) {
        Ok(r) => r,
        Err(_) => return Box::new(std::iter::empty()),
    };

    let results: Vec<(String, String)> = doc
        .select(&selector)
        .filter_map(move |element| {
            let href = element.value().attr("href")?;

            // Match the pattern against the raw href value (as per uscan behavior)
            if let Some(captures) = re.captures(href) {
                // Extract the first capture group as the version
                if let Some(version_match) = captures.get(1) {
                    let version = version_match.as_str().to_string();

                    // Convert href to absolute URL using proper URL joining
                    // Use base tag href if present, otherwise use page URL
                    let full_url = match base_url_parsed.join(href) {
                        Ok(url) => url.to_string(),
                        Err(_) => return None,
                    };

                    Some((version, full_url))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    Box::new(results.into_iter())
}

/// Search for version matches in plain text content
///
/// Searches the plain text for matches of the given pattern.
/// Returns an iterator of (version, url) tuples.
///
/// # Arguments
///
/// * `body` - The plain text content to search
/// * `matching_pattern` - Regex pattern to match
/// * `base_url` - Base URL for resolving relative links
///
/// # Examples
///
/// ```
/// use debian_watch::search::plain_search;
///
/// let text = b"project-1.0.tar.gz\nproject-2.0.tar.gz";
/// let results: Vec<_> = plain_search(text, r"project-(\d+\.\d+)\.tar\.gz", "https://example.com/")
///     .collect();
/// assert!(results.len() >= 1);
/// ```
pub fn plain_search(
    body: &[u8],
    matching_pattern: &str,
    base_url: &str,
) -> Box<dyn Iterator<Item = (String, String)>> {
    let re = match Regex::new(matching_pattern) {
        Ok(r) => r,
        Err(_) => return Box::new(std::iter::empty()),
    };

    let text = String::from_utf8_lossy(body);
    let base_url_parsed = match url::Url::parse(base_url) {
        Ok(u) => u,
        Err(_) => return Box::new(std::iter::empty()),
    };

    let results: Vec<(String, String)> = re
        .captures_iter(&text)
        .filter_map(|captures| {
            // Extract the first capture group as the version
            if let Some(version_match) = captures.get(1) {
                let version = version_match.as_str().to_string();
                // Use capture group 0 (full match) for constructing the URL
                let matched = captures.get(0).unwrap().as_str();

                // Convert matched text to absolute URL using proper URL joining
                let full_url = if matched.starts_with("http://") || matched.starts_with("https://")
                {
                    // Already absolute
                    matched.to_string()
                } else {
                    // Relative - use proper URL joining
                    match base_url_parsed.join(matched) {
                        Ok(url) => url.to_string(),
                        Err(_) => return None,
                    }
                };

                Some((version, full_url))
            } else {
                None
            }
        })
        .collect();

    Box::new(results.into_iter())
}

/// Search for version matches in content
///
/// Dispatches to either html_search or plain_search based on search mode.
pub fn search<R: Read>(
    searchmode: &str,
    mut resp: R,
    matching_pattern: &str,
    _package: &str,
    url: &str,
) -> Result<Box<dyn Iterator<Item = (String, String)>>, std::io::Error> {
    let mut body = Vec::new();
    resp.read_to_end(&mut body)?;

    let iter: Box<dyn Iterator<Item = (String, String)>> = match searchmode {
        #[cfg(feature = "discover")]
        "html" => html_search(&body, matching_pattern, url),
        "plain" => plain_search(&body, matching_pattern, url),
        _ => Box::new(std::iter::empty()),
    };

    Ok(iter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "discover")]
    fn test_html_search() {
        let html = b"<html><body><a href='project-1.0.tar.gz'>v1.0</a><a href='project-2.0.tar.gz'>v2.0</a></body></html>";
        let results: Vec<_> =
            html_search(html, r"project-(\d+\.\d+)\.tar\.gz", "https://example.com/").collect();

        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|(v, _)| v == "1.0"));
        assert!(results.iter().any(|(v, _)| v == "2.0"));
    }

    #[test]
    #[cfg(feature = "discover")]
    fn test_html_search_absolute_urls() {
        // Test curl case with <base> tag
        let html = b"<html><head><base href='https://curl.se'></head><body><a href='download/curl-8.14.0.tar.gz'>curl</a></body></html>";
        let results: Vec<_> = html_search(
            html,
            r"download/curl-([\d.]+)\.tar\.gz",
            "https://curl.se/download/",
        )
        .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "8.14.0");
        // With <base href="https://curl.se">, the href resolves to https://curl.se/download/curl-8.14.0.tar.gz
        assert_eq!(results[0].1, "https://curl.se/download/curl-8.14.0.tar.gz");
    }

    #[test]
    #[cfg(feature = "discover")]
    fn test_html_search_absolute_urls_with_slash_prefix() {
        // Test that returned URLs are absolute when href starts with '/'
        let html = b"<html><body><a href='/download/curl-8.14.0.tar.gz'>curl</a></body></html>";
        let results: Vec<_> = html_search(
            html,
            r"download/curl-([\d.]+)\.tar\.gz",
            "https://curl.se/download/",
        )
        .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "8.14.0");
        assert_eq!(results[0].1, "https://curl.se/download/curl-8.14.0.tar.gz");
    }

    #[test]
    #[cfg(feature = "discover")]
    fn test_html_search_with_absolute_href() {
        // Test that absolute URLs in href are preserved correctly
        let html = b"<html><body><a href='https://example.org/files/project-3.5.0.tar.gz'>v3.5.0</a></body></html>";
        let results: Vec<_> = html_search(
            html,
            r"https://example\.org/files/project-([\d.]+)\.tar\.gz",
            "https://example.com/",
        )
        .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "3.5.0");
        assert_eq!(
            results[0].1,
            "https://example.org/files/project-3.5.0.tar.gz"
        );
    }

    #[test]
    fn test_plain_search() {
        let text = b"Available: project-1.0.tar.gz project-2.0.tar.gz";
        let results: Vec<_> =
            plain_search(text, r"project-(\d+\.\d+)\.tar\.gz", "https://example.com/").collect();

        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|(v, _)| v == "1.0"));
        assert!(results.iter().any(|(v, _)| v == "2.0"));
    }

    #[test]
    fn test_plain_search_absolute_urls() {
        // Test that returned URLs are absolute, not relative
        let text = b"Available: curl-8.14.0.tar.gz";
        let results: Vec<_> =
            plain_search(text, r"curl-([\d.]+)\.tar\.gz", "https://curl.se/download/").collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "8.14.0");
        assert_eq!(results[0].1, "https://curl.se/download/curl-8.14.0.tar.gz");
    }

    #[test]
    fn test_plain_search_with_absolute_urls() {
        // Test that absolute URLs in text are preserved correctly
        let text = b"Available: https://example.org/files/project-3.5.0.tar.gz";
        let results: Vec<_> = plain_search(
            text,
            r"https://example\.org/files/project-([\d.]+)\.tar\.gz",
            "https://example.com/",
        )
        .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "3.5.0");
        assert_eq!(
            results[0].1,
            "https://example.org/files/project-3.5.0.tar.gz"
        );
    }
}
