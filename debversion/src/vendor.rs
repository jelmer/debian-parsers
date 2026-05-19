//! Vendor enum and related functions.
// Ideally we wouldn't have a list like this, but unfortunately we do.

use std::borrow::Cow;

/// Vendor enum.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Vendor {
    /// Debian
    Debian,

    /// Ubuntu (including derivatives)
    Ubuntu,

    /// Kali Linux
    Kali,
}

/// Error type for unknown vendor
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownVendorError(Cow<'static, str>);

impl std::fmt::Display for UnknownVendorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unknown vendor: {}", self.0)
    }
}

impl std::error::Error for UnknownVendorError {}

impl std::str::FromStr for Vendor {
    type Err = UnknownVendorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Case-insensitive comparison
        match s.to_lowercase().as_str() {
            "debian" => Ok(Vendor::Debian),
            "ubuntu" => Ok(Vendor::Ubuntu),
            "kali" => Ok(Vendor::Kali),
            _ => Err(UnknownVendorError(Cow::Owned(s.to_string()))),
        }
    }
}

impl std::fmt::Display for Vendor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Vendor::Debian => write!(f, "Debian"),
            Vendor::Ubuntu => write!(f, "Ubuntu"),
            Vendor::Kali => write!(f, "Kali"),
        }
    }
}

/// Get the initial Debian revision for a given vendor.
pub const fn initial_debian_revision(vendor: Vendor) -> &'static str {
    match vendor {
        Vendor::Debian => "1",
        Vendor::Ubuntu => "0ubuntu1",
        Vendor::Kali => "0kali1",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vendor_from_str() {
        use std::str::FromStr;

        assert_eq!(Vendor::from_str("debian").unwrap(), Vendor::Debian);
        assert_eq!(Vendor::from_str("Debian").unwrap(), Vendor::Debian);
        assert_eq!(Vendor::from_str("ubuntu").unwrap(), Vendor::Ubuntu);
        assert_eq!(Vendor::from_str("Ubuntu").unwrap(), Vendor::Ubuntu);
        assert_eq!(Vendor::from_str("kali").unwrap(), Vendor::Kali);
        assert_eq!(Vendor::from_str("Kali").unwrap(), Vendor::Kali);

        assert!(Vendor::from_str("unknown").is_err());
        assert_eq!(
            Vendor::from_str("unknown").unwrap_err().to_string(),
            "Unknown vendor: unknown"
        );
    }

    #[test]
    fn test_vendor_display() {
        assert_eq!(Vendor::Debian.to_string(), "Debian");
        assert_eq!(Vendor::Ubuntu.to_string(), "Ubuntu");
        assert_eq!(Vendor::Kali.to_string(), "Kali");
    }

    #[test]
    fn test_initial_debian_revision() {
        assert_eq!(initial_debian_revision(Vendor::Debian), "1");
        assert_eq!(initial_debian_revision(Vendor::Ubuntu), "0ubuntu1");
        assert_eq!(initial_debian_revision(Vendor::Kali), "0kali1");
    }
}
