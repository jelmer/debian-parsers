//! Launchpad PPA (Personal Package Archive) integration
//!
//! This module provides functionality for handling Personal Package Archives (PPAs) in a
//! Debian/Ubuntu context, including API integration using launchpadlib for authenticated access.

use launchpadlib::blocking::v1_0::{self, ArchiveFull, PersonOrTeam};
use launchpadlib::blocking::Client;
use launchpadlib::Error as WadlError;
use std::fmt;
use url::Url;

const CONSUMER_KEY: &str = "apt-add-repository";

/// Default URL for Launchpad PPAs
pub const LAUNCHPAD_PPA_URL: &str = "https://ppa.launchpadcontent.net";

/// Valid components for PPAs
pub const VALID_PPA_COMPONENTS: &[&str] = &["main", "main/debug"];

/// Information about a PPA (Personal Package Archive)
#[derive(Debug, Clone)]
pub struct PpaInfo {
    /// The PPA owner's username
    pub user: String,
    /// The PPA name
    pub name: String,
}

impl PpaInfo {
    /// Parse a PPA specification string (e.g., "ppa:user/ppa-name")
    pub fn parse(ppa_spec: &str) -> Result<PpaInfo, String> {
        if !ppa_spec.starts_with("ppa:") {
            return Err("Not a PPA format".to_string());
        }

        let ppa_part = &ppa_spec[4..];
        let parts: Vec<&str> = ppa_part.split('/').collect();

        if parts.len() != 2 {
            return Err("Invalid PPA format. Expected ppa:user/ppa-name".to_string());
        }

        Ok(PpaInfo {
            user: parts[0].to_string(),
            name: parts[1].to_string(),
        })
    }

    /// Try to extract PPA info from a Repository by parsing its URLs
    ///
    /// Returns `Some(PpaInfo)` if the repository URL matches the PPA URL pattern.
    pub fn from_repository(repo: &crate::Repository) -> Option<PpaInfo> {
        for uri in &repo.uris {
            let url_str = uri.as_str();
            // Check for PPA URLs like:
            // https://ppa.launchpadcontent.net/user/ppa-name/ubuntu
            // http://ppa.launchpad.net/user/ppa-name/ubuntu
            if url_str.contains("ppa.launchpadcontent.net") || url_str.contains("ppa.launchpad.net")
            {
                let path = uri.path();
                let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
                if parts.len() >= 2 {
                    return Some(PpaInfo {
                        user: parts[0].to_string(),
                        name: parts[1].to_string(),
                    });
                }
            }
        }
        None
    }

    /// Generate the repository URL for this PPA
    pub fn repository_url(&self, _codename: &str) -> Result<Url, String> {
        Url::parse(&format!(
            "{}/{}/{}/ubuntu",
            LAUNCHPAD_PPA_URL, self.user, self.name
        ))
        .map_err(|e| format!("Failed to construct PPA URL: {e}"))
    }

    /// Generate a filename for this PPA
    pub fn filename(&self, extension: &str) -> String {
        format!("{}-ubuntu-{}.{}", self.user, self.name, extension)
    }

    /// Generate a keyring filename for this PPA
    pub fn keyring_filename(&self) -> String {
        format!("{}-{}-keyring.asc", self.user, self.name)
    }
}

/// Result of validating a PPA
#[derive(Debug, Clone)]
pub struct PpaValidationResult {
    /// Whether the PPA exists
    pub exists: bool,
    /// Whether the PPA is private
    pub is_private: bool,
    /// Whether the PPA publishes debug symbols
    pub publishes_debug_symbols: bool,
    /// Display name of the PPA
    pub display_name: String,
}

/// Validate PPA components
pub fn validate_ppa_components(components: &[String]) -> Result<(), String> {
    for component in components {
        if !VALID_PPA_COMPONENTS.contains(&component.as_str()) {
            return Err(format!(
                "Invalid component '{}' for PPA.\n\
                 Valid components are: {}\n\
                 Suggestion: Use 'main' for regular packages or 'main/debug' for debug symbols.",
                component,
                VALID_PPA_COMPONENTS.join(", ")
            ));
        }
    }
    Ok(())
}

/// Errors that can occur when interacting with the Launchpad API
#[derive(Debug)]
pub enum Error {
    /// Failed to authenticate with Launchpad
    Authentication(String),
    /// Failed to connect to Launchpad API
    Api(WadlError),
    /// The requested user was not found
    UserNotFound(String),
    /// The requested PPA was not found
    PpaNotFound {
        /// The user or team name
        user: String,
        /// The PPA name
        name: String,
    },
    /// No signing key is configured for the PPA
    NoSigningKey,
    /// Failed to download the signing key
    KeyDownload(reqwest::Error),
    /// The keyserver returned an error status
    KeyserverError(reqwest::StatusCode),
    /// Failed to read key data
    KeyRead(reqwest::Error),
    /// Key fingerprint verification failed
    #[cfg(feature = "key-management")]
    KeyVerification(String),
    /// Not authenticated - no current user available
    NotAuthenticated,
    /// PPA has no self link
    MissingSelfLink,
    /// Failed to get subscription URL
    SubscriptionUrl(WadlError),
    /// Invalid subscription URL returned by API
    InvalidSubscriptionUrl {
        /// The invalid URL string
        url: String,
        /// The parse error
        error: url::ParseError,
    },
    /// Failed to create HTTP client
    HttpClient(reqwest::Error),
    /// Launchpad API returned unexpected response
    UnexpectedResponse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Authentication(msg) => {
                write!(f, "Failed to authenticate with Launchpad: {}", msg)
            }
            Error::Api(e) => write!(f, "Launchpad API error: {}", e),
            Error::UserNotFound(user) => write!(f, "User '{}' not found on Launchpad", user),
            Error::PpaNotFound { user, name } => {
                write!(f, "PPA '{}/{}' not found", user, name)
            }
            Error::NoSigningKey => write!(f, "PPA has no signing key configured"),
            Error::KeyDownload(e) => write!(f, "Failed to download signing key: {}", e),
            Error::KeyserverError(status) => {
                write!(f, "Keyserver returned error status: {}", status)
            }
            Error::KeyRead(e) => write!(f, "Failed to read key data: {}", e),
            #[cfg(feature = "key-management")]
            Error::KeyVerification(msg) => {
                write!(f, "Key fingerprint verification failed: {}", msg)
            }
            Error::NotAuthenticated => {
                write!(f, "Not authenticated - no current user available")
            }
            Error::MissingSelfLink => write!(f, "PPA has no self link"),
            Error::SubscriptionUrl(e) => write!(f, "Failed to get subscription URL: {}", e),
            Error::InvalidSubscriptionUrl { url, error } => {
                write!(f, "Invalid subscription URL '{}': {}", url, error)
            }
            Error::HttpClient(e) => write!(f, "Failed to create HTTP client: {}", e),
            Error::UnexpectedResponse(msg) => {
                write!(f, "Unexpected response from Launchpad: {}", msg)
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Api(e) => Some(e),
            Error::KeyDownload(e) => Some(e),
            Error::KeyRead(e) => Some(e),
            Error::SubscriptionUrl(e) => Some(e),
            Error::InvalidSubscriptionUrl { error, .. } => Some(error),
            Error::HttpClient(e) => Some(e),
            _ => None,
        }
    }
}

impl From<WadlError> for Error {
    fn from(e: WadlError) -> Self {
        Error::Api(e)
    }
}

/// Result of downloading a PPA signing key
#[derive(Debug, Clone)]
pub struct PpaSigningKey {
    /// The armored GPG key data
    pub key_data: String,
    /// The key fingerprint
    pub fingerprint: String,
}

/// Get a PPA by user and name
fn get_ppa(client: &Client, user: &str, name: &str) -> Result<ArchiveFull, Error> {
    let service_root = v1_0::service_root(client)?;

    let people = service_root
        .people()
        .ok_or_else(|| Error::UnexpectedResponse("No people collection".to_string()))?;

    let person_or_team = people.get_by_name(client, user).map_err(|e| {
        // Check if this is a 404 (user not found) or another error
        if matches!(e, WadlError::UnhandledStatus(s) if s.as_u16() == 404) {
            Error::UserNotFound(user.to_string())
        } else {
            Error::Api(e)
        }
    })?;

    // Note: get_ppaby_name is on Person/Team, not PersonFull/TeamFull
    let ppa = match person_or_team {
        PersonOrTeam::Person(person) => person.get_ppaby_name(client, None, name).map_err(|e| {
            if matches!(e, WadlError::UnhandledStatus(s) if s.as_u16() == 404) {
                Error::PpaNotFound {
                    user: user.to_string(),
                    name: name.to_string(),
                }
            } else {
                Error::Api(e)
            }
        })?,
        PersonOrTeam::Team(team) => team.get_ppaby_name(client, None, name).map_err(|e| {
            if matches!(e, WadlError::UnhandledStatus(s) if s.as_u16() == 404) {
                Error::PpaNotFound {
                    user: user.to_string(),
                    name: name.to_string(),
                }
            } else {
                Error::Api(e)
            }
        })?,
    };

    Ok(ppa)
}

/// Validate a PPA using launchpadlib
///
/// This provides authenticated access to Launchpad, which is required for private PPAs.
pub fn validate_ppa(ppa_info: &PpaInfo, auth_required: bool) -> Result<PpaValidationResult, Error> {
    let client = if auth_required {
        Client::authenticated(None, CONSUMER_KEY)
            .map_err(|e| Error::Authentication(e.to_string()))?
    } else {
        Client::anonymous(CONSUMER_KEY)
    };

    match get_ppa(&client, &ppa_info.user, &ppa_info.name) {
        Ok(ppa) => Ok(PpaValidationResult {
            exists: true,
            is_private: ppa.private,
            publishes_debug_symbols: false, // Not available in API
            display_name: ppa.displayname,
        }),
        Err(Error::PpaNotFound { .. }) | Err(Error::UserNotFound(_)) => Ok(PpaValidationResult {
            exists: false,
            is_private: false,
            publishes_debug_symbols: false,
            display_name: String::new(),
        }),
        Err(e) => Err(e),
    }
}

/// Get the subscription URL for a private PPA
///
/// This requires authentication with Launchpad and returns the URL that includes
/// the user's subscription credentials.
pub fn get_private_ppa_url(ppa_info: &PpaInfo) -> Result<Url, Error> {
    let client = Client::authenticated(None, CONSUMER_KEY)
        .map_err(|e| Error::Authentication(e.to_string()))?;

    // Get the service root and the current user
    let service_root = v1_0::service_root(&client)?;

    let me = service_root.me().ok_or(Error::NotAuthenticated)?;

    // Get the PPA
    let ppa = get_ppa(&client, &ppa_info.user, &ppa_info.name)?;

    // Get the Archive reference from the PPA
    let archive = ppa.self_().ok_or(Error::MissingSelfLink)?;

    // Get the subscription URL for this archive
    let subscription_url = me
        .get_archive_subscription_url(&client, &archive)
        .map_err(Error::SubscriptionUrl)?;

    Url::parse(&subscription_url).map_err(|e| Error::InvalidSubscriptionUrl {
        url: subscription_url,
        error: e,
    })
}

/// Download PPA signing key using launchpadlib
///
/// This provides authenticated access which may be required for private PPAs.
/// Returns the key data and fingerprint for verification and logging.
pub fn download_ppa_signing_key(
    ppa_info: &PpaInfo,
    auth_required: bool,
) -> Result<PpaSigningKey, Error> {
    let client = if auth_required {
        Client::authenticated(None, CONSUMER_KEY)
            .map_err(|e| Error::Authentication(e.to_string()))?
    } else {
        Client::anonymous(CONSUMER_KEY)
    };

    let ppa = get_ppa(&client, &ppa_info.user, &ppa_info.name)?;

    let fingerprint = ppa
        .signing_key_fingerprint
        .filter(|fp| !fp.is_empty())
        .ok_or(Error::NoSigningKey)?;

    // Download the key data from the keyserver
    // The key is available at: https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x{fingerprint}
    let key_url = format!(
        "https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x{}",
        fingerprint
    );

    let http_client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(Error::HttpClient)?;

    let response = http_client
        .get(&key_url)
        .send()
        .map_err(Error::KeyDownload)?;

    if !response.status().is_success() {
        return Err(Error::KeyserverError(response.status()));
    }

    let key_data = response.text().map_err(Error::KeyRead)?;

    // Verify the key fingerprint matches
    #[cfg(feature = "key-management")]
    crate::key_management::verify_key_fingerprint(&key_data, &fingerprint)
        .map_err(|e| Error::KeyVerification(e.to_string()))?;

    Ok(PpaSigningKey {
        key_data,
        fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ppa_format() {
        // Valid PPA
        let ppa = PpaInfo::parse("ppa:user/repo").unwrap();
        assert_eq!(ppa.user, "user");
        assert_eq!(ppa.name, "repo");

        // Invalid formats
        assert!(PpaInfo::parse("not-a-ppa").is_err());
        assert!(PpaInfo::parse("ppa:invalid").is_err());
        assert!(PpaInfo::parse("ppa:too/many/parts").is_err());
    }

    #[test]
    fn test_validate_ppa_components() {
        assert!(validate_ppa_components(&["main".to_string()]).is_ok());
        assert!(validate_ppa_components(&["main/debug".to_string()]).is_ok());
        assert!(validate_ppa_components(&["invalid".to_string()]).is_err());
    }

    #[test]
    fn test_ppa_filename() {
        let ppa = PpaInfo {
            user: "test-user".to_string(),
            name: "test-repo".to_string(),
        };

        assert_eq!(ppa.filename("list"), "test-user-ubuntu-test-repo.list");
        assert_eq!(
            ppa.filename("sources"),
            "test-user-ubuntu-test-repo.sources"
        );

        // Test with empty extension
        assert_eq!(ppa.filename(""), "test-user-ubuntu-test-repo.");

        // Test with special characters (they remain as-is)
        let ppa_special = PpaInfo {
            user: "user_123".to_string(),
            name: "repo-name".to_string(),
        };
        assert_eq!(
            ppa_special.filename("list"),
            "user_123-ubuntu-repo-name.list"
        );
    }
}
