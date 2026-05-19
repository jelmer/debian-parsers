#![deny(missing_docs)]
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]
// Until we drop support for PyO3 0.22
#![allow(deprecated)]

use lazy_regex::{regex_captures, regex_replace};
use num_bigint::BigInt;
use std::cmp::Ordering;
use std::str::FromStr;

pub mod upstream;
pub mod vcs;
pub mod vendor;

/// A Debian version string
///
///
#[derive(Debug, Clone)]
pub struct Version {
    /// The epoch of the version, if any
    pub epoch: Option<u32>,

    /// The upstream version
    pub upstream_version: String,

    /// The Debian revision, if any
    pub debian_revision: Option<String>,
}

fn non_digit_cmp(va: &str, vb: &str) -> Ordering {
    fn order(x: char) -> i32 {
        match x {
            '~' => -1,
            '0'..='9' => unreachable!(),
            'A'..='Z' | 'a'..='z' => x as i32,
            _ => x as i32 + 256,
        }
    }

    va.chars()
        .map(order)
        .chain(std::iter::repeat(0))
        .zip(vb.chars().map(order).chain(std::iter::repeat(0)))
        .take(va.len().max(vb.len()))
        .find_map(|(a, b)| match a.cmp(&b) {
            Ordering::Equal => None,
            other => Some(other),
        })
        .unwrap_or(Ordering::Equal)
}

#[test]
fn test_non_digit_cmp() {
    assert_eq!(non_digit_cmp("a", "b"), Ordering::Less);
    assert_eq!(non_digit_cmp("b", "a"), Ordering::Greater);
    assert_eq!(non_digit_cmp("a", "a"), Ordering::Equal);
    assert_eq!(non_digit_cmp("a", "-"), Ordering::Less);
    assert_eq!(non_digit_cmp("a", "+"), Ordering::Less);
    assert_eq!(non_digit_cmp("a", ""), Ordering::Greater);
    assert_eq!(non_digit_cmp("", "a"), Ordering::Less);
    assert_eq!(non_digit_cmp("", ""), Ordering::Equal);
    assert_eq!(non_digit_cmp("~", ""), Ordering::Less);
    assert_eq!(non_digit_cmp("~~", "~"), Ordering::Less);
    assert_eq!(non_digit_cmp("~~", "~~a"), Ordering::Less);
    assert_eq!(non_digit_cmp("~~a", "~"), Ordering::Less);
    assert_eq!(non_digit_cmp("~", "a"), Ordering::Less);
    // Test special characters that exercise the arithmetic on line 36
    assert_eq!(non_digit_cmp("!", "@"), Ordering::Less); // ! = 33, @ = 64 + 256 = 320
    assert_eq!(non_digit_cmp("@", "!"), Ordering::Greater);
    assert_eq!(non_digit_cmp("#", "$"), Ordering::Less); // # = 35, $ = 36
    assert_eq!(non_digit_cmp("|", "}"), Ordering::Less); // | = 124, } = 125
}

fn drop_leading_zeroes(s: &str) -> &str {
    // Drop leading zeroes while the next character is a digit
    let bytes = s.as_bytes();
    let mut start = 0;
    while start + 1 < bytes.len() && bytes[start] == b'0' && bytes[start + 1].is_ascii_digit() {
        start += 1;
    }
    &s[start..]
}

fn version_cmp_part(mut a: &str, mut b: &str) -> Ordering {
    while !a.is_empty() || !b.is_empty() {
        // First, create a for the non-digit leading part of each string
        let a_non_digit = &a[..a
            .chars()
            .position(|c| c.is_ascii_digit())
            .unwrap_or(a.len())];
        let b_non_digit = &b[..b
            .chars()
            .position(|c| c.is_ascii_digit())
            .unwrap_or(b.len())];

        // Compare the non-digit leading part
        match non_digit_cmp(a_non_digit, b_non_digit) {
            Ordering::Equal => (),
            ordering => return ordering,
        }

        // Remove the non-digit leading part from the strings
        a = &a[a_non_digit.len()..];
        b = &b[b_non_digit.len()..];

        // Then, create a slice for the digit part of each string
        let a_digit = &a[..a
            .chars()
            .position(|c| !c.is_ascii_digit())
            .unwrap_or(a.len())];
        let b_digit = &b[..b
            .chars()
            .position(|c| !c.is_ascii_digit())
            .unwrap_or(b.len())];

        // Compare the digit part
        let ordering = match (a_digit.len(), b_digit.len()) {
            (0, 0) => Ordering::Equal,
            (0, _) => Ordering::Less,
            (_, 0) => Ordering::Greater,
            // For small numbers that fit in u64, avoid BigInt allocation
            (a_len, b_len) if a_len <= 19 && b_len <= 19 => {
                match (a_digit.parse::<u64>(), b_digit.parse::<u64>()) {
                    (Ok(a_num), Ok(b_num)) => a_num.cmp(&b_num),
                    // Fallback to BigInt if parsing fails (shouldn't happen with valid version strings)
                    _ => a_digit
                        .parse::<BigInt>()
                        .unwrap()
                        .cmp(&b_digit.parse::<BigInt>().unwrap()),
                }
            }
            // For very long digit sequences, use BigInt
            _ => a_digit
                .parse::<BigInt>()
                .unwrap()
                .cmp(&b_digit.parse::<BigInt>().unwrap()),
        };

        match ordering {
            Ordering::Equal => (),
            ordering => return ordering,
        }

        // Remove the digit part from the strings
        a = &a[a_digit.len()..];
        b = &b[b_digit.len()..];
    }
    Ordering::Equal
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let self_norm = self.explicit();
        let other_norm = other.explicit();
        if self_norm.0 != other_norm.0 {
            return std::cmp::Ord::cmp(&self_norm.0, &other_norm.0);
        }

        match version_cmp_part(self_norm.1, other_norm.1) {
            Ordering::Equal => (),
            ordering => return ordering,
        }

        version_cmp_part(self_norm.2, other_norm.2)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.partial_cmp(other) == Some(std::cmp::Ordering::Equal)
    }
}

impl Eq for Version {}

/// Error parsing a version string
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

impl FromStr for Version {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (_, epoch, upstream_version, debian_revision) = if let Some(c) = regex_captures!(
            r"^(?:(\d+):)?([A-Za-z0-9.+:~-]+?)(?:-([A-Za-z0-9+.~]+))?$",
            text
        ) {
            c
        } else {
            return Err(ParseError(format!("Invalid version string: {}", text)));
        };

        let epoch = Some(epoch)
            .filter(|e| !e.is_empty())
            .map(|e| {
                e.parse()
                    .map_err(|e| ParseError(format!("Error parsing epoch: {}", e)))
            })
            .transpose()?;

        let debian_revision = if debian_revision.is_empty() {
            None
        } else {
            Some(debian_revision.to_string())
        };

        Ok(Version {
            epoch,
            upstream_version: upstream_version.to_string(),
            debian_revision,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if let Some(epoch) = self.epoch.as_ref() {
            write!(f, "{}:", epoch)?;
        }
        f.write_str(&self.upstream_version)?;
        if let Some(debian_revision) = self.debian_revision.as_ref() {
            write!(f, "-{}", debian_revision)?;
        }
        Ok(())
    }
}

impl std::hash::Hash for Version {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (
            self.epoch,
            self.upstream_version.as_str(),
            self.debian_revision.as_deref(),
        )
            .hash(state);
    }
}

impl Version {
    /// Return explicit tuple of this version
    ///
    /// This will return an explicit 0 for epochs and debian revisions
    /// that are not set.
    fn explicit(&self) -> (u32, &str, &str) {
        (
            self.epoch.unwrap_or(0),
            self.upstream_version.as_str(),
            self.debian_revision.as_deref().unwrap_or("0"),
        )
    }

    /// Parse a version string with lenient parsing rules.
    ///
    /// This method accepts version strings that may not strictly comply with Debian Policy,
    /// such as versions containing underscores. While the standard `FromStr` implementation
    /// enforces strict Debian Policy compliance, this method is more permissive to handle
    /// real-world packages that may use non-standard characters.
    ///
    /// # Examples
    ///
    /// ```
    /// use debversion::Version;
    /// // This version contains underscores, which violates Debian Policy
    /// // but may appear in real-world packages
    /// let v = Version::parse_lenient("2.0.37+cvs.JCW_PRE2_2037-1").unwrap();
    /// assert_eq!(v.upstream_version, "2.0.37+cvs.JCW_PRE2_2037");
    /// assert_eq!(v.debian_revision, Some("1".to_string()));
    /// ```
    pub fn parse_lenient(text: &str) -> Result<Self, ParseError> {
        let (_, epoch, upstream_version, debian_revision) = if let Some(c) = regex_captures!(
            r"^(?:(\d+):)?([A-Za-z0-9.+:~_-]+?)(?:-([A-Za-z0-9+.~_]+))?$",
            text
        ) {
            c
        } else {
            return Err(ParseError(format!("Invalid version string: {}", text)));
        };

        let epoch = Some(epoch)
            .filter(|e| !e.is_empty())
            .map(|e| {
                e.parse()
                    .map_err(|e| ParseError(format!("Error parsing epoch: {}", e)))
            })
            .transpose()?;

        let debian_revision = if debian_revision.is_empty() {
            None
        } else {
            Some(debian_revision.to_string())
        };

        Ok(Version {
            epoch,
            upstream_version: upstream_version.to_string(),
            debian_revision,
        })
    }

    /// Is this a binNMU?
    ///
    /// A binNMU is a binary-only NMU (Non-Maintainer Upload) where the source package is not
    /// changed.
    ///
    /// Note that this checks for the presence of the `+b[:digit:]` suffix, which is not part of the Debian
    /// Policy Manual, but it is commonly used to indicate a binNMU.
    ///
    /// # Examples
    ///
    /// ```
    /// use debversion::Version;
    /// assert!("1.0+b1".parse::<Version>().unwrap().is_bin_nmu());
    /// assert!("1.0-1+b1".parse::<Version>().unwrap().is_bin_nmu());
    /// assert!(!"1.0-1".parse::<Version>().unwrap().is_bin_nmu());
    /// assert!(!"1.0".parse::<Version>().unwrap().is_bin_nmu());
    /// ```
    pub fn is_bin_nmu(&self) -> bool {
        self.bin_nmu_count().is_some()
    }

    /// Return the binNMU count of this version
    ///
    /// This will return the binNMU count of this version, or None if this is not a binNMU.
    pub fn bin_nmu_count(&self) -> Option<i32> {
        fn bin_nmu_suffix(s: &str) -> Option<i32> {
            s.split_once("+b").and_then(|(_, rest)| rest.parse().ok())
        }
        if let Some(debian_revision) = self.debian_revision.as_ref() {
            bin_nmu_suffix(debian_revision)
        } else {
            bin_nmu_suffix(self.upstream_version.as_str())
        }
    }

    /// Create a binNMU version from this version
    ///
    /// This will increment the binNMU suffix by one, or add a `+b1` suffix if there is no binNMU
    /// suffix.
    pub fn increment_bin_nmu(self) -> Version {
        fn increment_bin_nmu_suffix(s: &str) -> String {
            match s.split_once("+b") {
                Some((prefix, rest)) => match rest.parse::<i32>() {
                    Ok(num) => format!("{}+b{}", prefix, num + 1),
                    Err(_) => format!("{}+b1", s),
                },
                None => format!("{}+b1", s),
            }
        }

        if let Some(debian_revision) = self.debian_revision.as_ref() {
            Version {
                epoch: self.epoch,
                upstream_version: self.upstream_version,
                debian_revision: Some(increment_bin_nmu_suffix(debian_revision)),
            }
        } else {
            Version {
                epoch: self.epoch,
                upstream_version: increment_bin_nmu_suffix(&self.upstream_version),
                debian_revision: self.debian_revision,
            }
        }
    }

    /// Check if this version is a sourceful NMU
    ///
    /// A sourceful NMU is a Non-Maintainer Upload where the source package is changed.
    /// This is indicated by the presence of a `+nmu[:digit:]` suffix.
    /// This is not part of the Debian Policy Manual, but it is commonly used to indicate a
    /// sourceful NMU.
    pub fn is_nmu(&self) -> bool {
        self.nmu_count().is_some()
    }

    /// Return the sourceful NMU count of this version
    ///
    /// This will return the sourceful NMU count of this version, or None if this is not a
    /// sourceful NMU.
    pub fn nmu_count(&self) -> Option<i32> {
        fn nmu_suffix(s: &str) -> Option<i32> {
            s.split_once("+nmu").and_then(|(_, rest)| rest.parse().ok())
        }
        if let Some(debian_revision) = self.debian_revision.as_ref() {
            nmu_suffix(debian_revision)
        } else {
            nmu_suffix(self.upstream_version.as_str())
        }
    }

    /// Return canonicalized version of this version
    ///
    /// # Examples
    ///
    /// ```
    /// use debversion::Version;
    /// assert_eq!("1.0-0".parse::<Version>().unwrap().canonicalize(), "1.0".parse::<Version>().unwrap());
    /// assert_eq!("1.0-1".parse::<Version>().unwrap().canonicalize(), "1.0-1".parse::<Version>().unwrap());
    /// ```
    pub fn canonicalize(&self) -> Version {
        let epoch = match self.epoch {
            Some(0) => None,
            epoch => epoch,
        };

        let upstream_version_stripped = drop_leading_zeroes(&self.upstream_version);
        let upstream_version = if upstream_version_stripped == self.upstream_version {
            self.upstream_version.clone()
        } else {
            upstream_version_stripped.to_string()
        };

        let debian_revision = match self.debian_revision.as_ref() {
            Some(r) if r.chars().all(|c| c == '0') => None,
            None => None,
            Some(revision) => {
                let stripped = drop_leading_zeroes(revision);
                if stripped == revision {
                    Some(revision.clone())
                } else {
                    Some(stripped.to_string())
                }
            }
        };

        Version {
            epoch,
            upstream_version,
            debian_revision,
        }
    }

    /// Increment the Debian revision.
    ///
    /// For native packages, increment the upstream version number.
    /// For other packages, increment the debian revision.
    pub fn increment_debian(&mut self) {
        if let Some(ref mut debian_revision) = self.debian_revision {
            *debian_revision = regex_replace!(r"\d+$", debian_revision, |x: &str| {
                (x.parse::<i32>().unwrap() + 1).to_string()
            })
            .into_owned();
        } else {
            self.upstream_version = regex_replace!(r"\d+$", &self.upstream_version, |x: &str| {
                (x.parse::<i32>().unwrap() + 1).to_string()
            })
            .into_owned();
        }
    }

    /// Return true if this is a native package
    pub fn is_native(&self) -> bool {
        self.debian_revision.is_none()
    }
}

#[cfg(feature = "sqlx")]
use sqlx::{postgres::PgTypeInfo, Postgres};

#[cfg(feature = "sqlx")]
impl sqlx::Type<Postgres> for Version {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("debversion")
    }
}

#[cfg(feature = "sqlx")]
impl sqlx::Encode<'_, Postgres> for Version {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        let version_str = self.to_string();
        sqlx::Encode::<Postgres>::encode_by_ref(&version_str.as_str(), buf)
    }
}

#[cfg(feature = "sqlx")]
impl sqlx::Decode<'_, Postgres> for Version {
    fn decode(
        value: sqlx::postgres::PgValueRef<'_>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let s: &str = sqlx::Decode::<Postgres>::decode(value)?;
        Ok(s.parse::<Version>()?)
    }
}

#[cfg(all(feature = "sqlx", test))]
mod sqlx_tests {
    #[test]
    fn type_info() {
        use super::Version;
        use sqlx::postgres::PgTypeInfo;
        use sqlx::Type;

        assert_eq!(PgTypeInfo::with_name("debversion"), Version::type_info());
    }
}

#[cfg(feature = "python-debian")]
use pyo3::prelude::*;

#[cfg(feature = "python-debian")]
impl FromPyObject<'_, '_> for Version {
    type Error = PyErr;

    fn extract(ob: pyo3::Borrowed<'_, '_, PyAny>) -> PyResult<Self> {
        let debian_support = Python::import(ob.py(), "debian.debian_support")?;
        let version_cls = debian_support.getattr("Version")?;
        if !ob.is_instance(&version_cls)? {
            return Err(pyo3::exceptions::PyTypeError::new_err("Expected a Version"));
        }
        Ok(Version {
            epoch: ob
                .getattr("epoch")?
                .extract::<Option<String>>()?
                .map(|s| s.parse().unwrap()),
            upstream_version: ob.getattr("upstream_version")?.extract::<String>()?,
            debian_revision: ob.getattr("debian_revision")?.extract::<Option<String>>()?,
        })
    }
}

#[cfg(feature = "python-debian")]
impl<'py> IntoPyObject<'py> for Version {
    type Target = PyAny;

    type Output = Bound<'py, Self::Target>;

    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        let debian_support = py.import("debian.debian_support").unwrap();
        let version_cls = debian_support.getattr("Version").unwrap();
        version_cls.call1((self.to_string(),))
    }
}

#[cfg(feature = "python-debian")]
mod python_tests {
    #[test]
    fn test_from_pyobject() {
        use super::Version;
        use pyo3::prelude::*;
        use std::ffi::CString;

        Python::attach(|py| {
            let globals = pyo3::types::PyDict::new(py);
            globals
                .set_item(
                    "debian_support",
                    py.import("debian.debian_support").unwrap(),
                )
                .unwrap();
            let v = py
                .eval(
                    &CString::new("debian_support.Version('1.0-1')").unwrap(),
                    Some(&globals),
                    None,
                )
                .unwrap()
                .extract::<Version>()
                .unwrap();
            assert_eq!(
                v,
                Version {
                    epoch: None,
                    upstream_version: "1.0".to_string(),
                    debian_revision: Some("1".to_string())
                }
            );
        });
    }

    #[test]
    fn test_to_pyobject() {
        use super::Version;
        use pyo3::prelude::*;

        Python::attach(|py| {
            let v = Version {
                epoch: Some(1),
                upstream_version: "1.0".to_string(),
                debian_revision: Some("1".to_string()),
            };
            let v = v.into_pyobject(py).unwrap();
            let expected: Version = "1:1.0-1".parse().unwrap();
            assert_eq!(v.get_type().name().unwrap(), "Version");
            assert_eq!(v.unbind().extract::<Version>(py).unwrap(), expected);
        });
    }

    #[test]
    fn test_from_pyobject_error() {
        use super::Version;
        use pyo3::prelude::*;
        use std::ffi::CString;

        Python::attach(|py| {
            // Test that extracting from a non-Version object fails
            let string_obj = py
                .eval(&CString::new("'not a version'").unwrap(), None, None)
                .unwrap();
            let result = string_obj.extract::<Version>();
            assert!(result.is_err());
        });
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let concatenated: String = String::deserialize(deserializer)?;
        concatenated.parse().map_err(serde::de::Error::custom)
    }
}

/// Trait for converting an argument into a Version
pub trait AsVersion {
    /// Convert the argument into a Version
    fn into_version(self) -> Result<Version, ParseError>;
}

impl AsVersion for &str {
    fn into_version(self) -> Result<Version, ParseError> {
        self.parse()
    }
}

impl AsVersion for String {
    fn into_version(self) -> Result<Version, ParseError> {
        self.parse()
    }
}

impl AsVersion for Version {
    fn into_version(self) -> Result<Version, ParseError> {
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{version_cmp_part, ParseError, Version};
    use std::cmp::Ordering;

    #[test]
    fn test_canonicalize() {
        assert_eq!(
            "1.0-1".parse::<Version>().unwrap().canonicalize(),
            "1.0-1".parse::<Version>().unwrap()
        );
        assert_eq!(
            "1.0-0".parse::<Version>().unwrap().canonicalize(),
            "1.0".parse::<Version>().unwrap()
        );
        assert_eq!(
            "0:1.0-2".parse::<Version>().unwrap().canonicalize(),
            "1.0-2".parse::<Version>().unwrap()
        );
        assert_eq!(
            "0001.0-0".parse::<Version>().unwrap().canonicalize(),
            "1.0".parse::<Version>().unwrap()
        );
        assert_eq!(
            "000.1".parse::<Version>().unwrap().canonicalize(),
            "0.1".parse::<Version>().unwrap()
        );
    }

    #[test]
    fn test_explicit() {
        assert_eq!(
            (0, "1.0", "1"),
            "1.0-1".parse::<Version>().unwrap().explicit()
        );
        assert_eq!(
            (1, "1.0", "1"),
            "1:1.0-1".parse::<Version>().unwrap().explicit()
        );
        assert_eq!(
            (0, "1.0", "0"),
            "1.0".parse::<Version>().unwrap().explicit()
        );
        assert_eq!(
            (0, "1.0", "0"),
            "1.0-0".parse::<Version>().unwrap().explicit()
        );
        assert_eq!(
            (1, "1.0", "0"),
            "1:1.0-0".parse::<Version>().unwrap().explicit()
        );
        assert_eq!(
            (0, "000.1", "0"),
            "000.1".parse::<Version>().unwrap().explicit()
        );
    }

    macro_rules! assert_cmp(
        ($a:expr, $b:expr, $cmp:tt) => {
            assert_eq!($a.parse::<Version>().unwrap().cmp(&$b.parse::<Version>().unwrap()), std::cmp::Ordering::$cmp);
        }
    );

    #[test]
    fn test_version_cmp_part() {
        assert_eq!(version_cmp_part("1.0", "1.0"), Ordering::Equal);
        assert_eq!(version_cmp_part("0.1", "0.1"), Ordering::Equal);
        assert_eq!(version_cmp_part("000.1", "0.1"), Ordering::Equal);
        assert_eq!(version_cmp_part("1.0", "2.0"), Ordering::Less);
        assert_eq!(version_cmp_part("1.0", "0.0"), Ordering::Greater);
        assert_eq!(version_cmp_part("10.0", "2.0"), Ordering::Greater);
        assert_eq!(version_cmp_part("1.0~rc1", "1.0"), Ordering::Less);
    }

    #[test]
    fn test_cmp() {
        assert_cmp!("1.0-1", "1.0-1", Equal);
        assert_cmp!("1.0-1", "1.0-2", Less);
        assert_cmp!("1.0-2", "1.0-1", Greater);
        assert_cmp!("1.0-1", "1.0", Greater);
        assert_cmp!("1.0", "1.0-1", Less);
        assert_cmp!("2.50.0", "10.0.1", Less);

        // Epoch
        assert_cmp!("1:1.0-1", "1.0-1", Greater);
        assert_cmp!("1.0-1", "1:1.0-1", Less);
        assert_cmp!("1:1.0-1", "1:1.0-1", Equal);
        assert_cmp!("1:1.0-1", "2:1.0-1", Less);
        assert_cmp!("2:1.0-1", "1:1.0-1", Greater);

        // ~ symbol
        assert_cmp!("1.0~rc1-1", "1.0-1", Less);
        assert_cmp!("1.0-1", "1.0~rc1-1", Greater);
        assert_cmp!("1.0~rc1-1", "1.0~rc1-1", Equal);
        assert_cmp!("1.0~rc1-1", "1.0~rc2-1", Less);
        assert_cmp!("1.0~rc2-1", "1.0~rc1-1", Greater);

        // letters
        assert_cmp!("1.0a-1", "1.0-1", Greater);
        assert_cmp!("1.0-1", "1.0a-1", Less);
        assert_cmp!("1.0a-1", "1.0a-1", Equal);

        // Bug 27
        assert_cmp!("23.13.9-7", "0.6.45-2", Greater);
    }

    #[test]
    fn test_parse() {
        assert_eq!(
            Version {
                epoch: None,
                upstream_version: "1.0".to_string(),
                debian_revision: Some("1".to_string())
            },
            "1.0-1".parse().unwrap()
        );

        assert_eq!(
            Version {
                epoch: None,
                upstream_version: "1.0".to_string(),
                debian_revision: None
            },
            "1.0".parse().unwrap()
        );

        assert_eq!(
            Version {
                epoch: Some(1),
                upstream_version: "1.0".to_string(),
                debian_revision: Some("1".to_string())
            },
            "1:1.0-1".parse().unwrap()
        );
        assert_eq!(
            "1:;a".parse::<Version>().unwrap_err(),
            ParseError("Invalid version string: 1:;a".to_string())
        );
    }

    #[test]
    fn test_parse_error_display() {
        let error = ParseError("test error message".to_string());
        assert_eq!(format!("{}", error), "test error message");
        assert_eq!(error.to_string(), "test error message");
    }

    #[test]
    fn test_to_string() {
        assert_eq!(
            "1.0-1",
            Version {
                epoch: None,
                upstream_version: "1.0".to_string(),
                debian_revision: Some("1".to_string())
            }
            .to_string()
        );
        assert_eq!(
            "1.0",
            Version {
                epoch: None,
                upstream_version: "1.0".to_string(),
                debian_revision: None,
            }
            .to_string()
        );
    }

    #[test]
    fn test_eq() {
        assert_eq!(
            "1.0-1".parse::<Version>().unwrap(),
            "1.0-1".parse::<Version>().unwrap()
        );
    }

    #[test]
    fn test_hash() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        let mut hasher3 = DefaultHasher::new();

        "1.0-1".parse::<Version>().unwrap().hash(&mut hasher1);
        "1.0-1".parse::<Version>().unwrap().hash(&mut hasher2);
        "0:1.0-1".parse::<Version>().unwrap().hash(&mut hasher3);

        let hash1 = hasher1.finish();
        let hash2 = hasher2.finish();
        let hash3 = hasher3.finish();

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn to_string() {
        assert_eq!(
            "1.0-1",
            Version {
                epoch: None,
                upstream_version: "1.0".to_string(),
                debian_revision: Some("1".to_string())
            }
            .to_string()
        );
        assert_eq!(
            "1.0",
            Version {
                epoch: None,
                upstream_version: "1.0".to_string(),
                debian_revision: None,
            }
            .to_string()
        );
        assert_eq!(
            "1:1.0",
            Version {
                epoch: Some(1),
                upstream_version: "1.0".to_string(),
                debian_revision: None,
            }
            .to_string()
        );
    }

    #[test]
    fn partial_eq() {
        assert!("1.0-1"
            .parse::<Version>()
            .unwrap()
            .eq(&"1.0-1".parse::<Version>().unwrap()));
    }

    #[test]
    fn increment() {
        let mut v = "1.0-1".parse::<Version>().unwrap();
        v.increment_debian();

        assert_eq!("1.0-2".parse::<Version>().unwrap(), v);

        let mut v = "1.0".parse::<Version>().unwrap();
        v.increment_debian();
        assert_eq!("1.1".parse::<Version>().unwrap(), v);

        let mut v = "1.0ubuntu1".parse::<Version>().unwrap();
        v.increment_debian();
        assert_eq!("1.0ubuntu2".parse::<Version>().unwrap(), v);

        let mut v = "1.0-0ubuntu1".parse::<Version>().unwrap();
        v.increment_debian();
        assert_eq!("1.0-0ubuntu2".parse::<Version>().unwrap(), v);
    }

    #[test]
    fn is_native() {
        assert!(!"1.0-1".parse::<Version>().unwrap().is_native());
        assert!("1.0".parse::<Version>().unwrap().is_native());
        assert!(!"1.0-0".parse::<Version>().unwrap().is_native());
    }

    #[test]
    fn test_is_binnmu() {
        assert!("1.0+b1".parse::<Version>().unwrap().is_bin_nmu());
        assert!("1.0-1+b1".parse::<Version>().unwrap().is_bin_nmu());
        assert!(!"1.0-1".parse::<Version>().unwrap().is_bin_nmu());
        assert!(!"1.0".parse::<Version>().unwrap().is_bin_nmu());
    }

    #[test]
    fn test_bin_nmu_count() {
        assert_eq!(
            Some(1),
            "1.0+b1".parse::<Version>().unwrap().bin_nmu_count()
        );
        assert_eq!(
            Some(1),
            "1.0-1+b1".parse::<Version>().unwrap().bin_nmu_count()
        );
        assert_eq!(None, "1.0-1".parse::<Version>().unwrap().bin_nmu_count());
        assert_eq!(None, "1.0".parse::<Version>().unwrap().bin_nmu_count());
    }

    #[test]
    fn test_increment_bin_nmu() {
        assert_eq!(
            "1.0+b2".parse::<Version>().unwrap(),
            "1.0+b1".parse::<Version>().unwrap().increment_bin_nmu()
        );
        assert_eq!(
            "1.0-1+b2".parse::<Version>().unwrap(),
            "1.0-1+b1".parse::<Version>().unwrap().increment_bin_nmu()
        );
        assert_eq!(
            "1.0+b1".parse::<Version>().unwrap(),
            "1.0".parse::<Version>().unwrap().increment_bin_nmu()
        );
        assert_eq!(
            "1.0-1+b1".parse::<Version>().unwrap(),
            "1.0-1".parse::<Version>().unwrap().increment_bin_nmu()
        );
    }

    #[test]
    fn test_nmu_count() {
        assert_eq!(Some(1), "1.0+nmu1".parse::<Version>().unwrap().nmu_count());
        assert_eq!(
            Some(1),
            "1.0-1+nmu1".parse::<Version>().unwrap().nmu_count()
        );
        assert_eq!(None, "1.0-1".parse::<Version>().unwrap().nmu_count());
        assert_eq!(None, "1.0".parse::<Version>().unwrap().nmu_count());
    }

    #[test]
    fn test_is_nmu() {
        assert!("1.0+nmu1".parse::<Version>().unwrap().is_nmu());
        assert!("1.0-1+nmu1".parse::<Version>().unwrap().is_nmu());
        assert!(!"1.0-1".parse::<Version>().unwrap().is_nmu());
        assert!(!"1.0".parse::<Version>().unwrap().is_nmu());
    }

    #[test]
    fn test_comparing_very_long_versions() {
        // These are actual version numbers seen in actual apt repositories
        let a = "1:11.1.0~++20210314110124+1fdec59bffc1-1~exp1~20210314220751.162";
        let b = "1:11.1.0~++20211011013104+1fdec59bffc1-1~exp1~20211011133507.6";
        assert_cmp!(a, b, Less);
    }

    #[test]
    fn test_drop_leading_zeroes() {
        use super::drop_leading_zeroes;

        // Test basic cases
        assert_eq!(drop_leading_zeroes("1.0"), "1.0");
        assert_eq!(drop_leading_zeroes("001.0"), "1.0");
        assert_eq!(drop_leading_zeroes("000.1"), "0.1");

        // Test edge cases for missed mutants
        assert_eq!(drop_leading_zeroes("0"), "0");
        assert_eq!(drop_leading_zeroes("00"), "0");
        assert_eq!(drop_leading_zeroes("0a"), "0a");
        assert_eq!(drop_leading_zeroes("01a"), "1a");

        // Test single character
        assert_eq!(drop_leading_zeroes("a"), "a");

        // Test empty string
        assert_eq!(drop_leading_zeroes(""), "");
    }

    #[test]
    fn test_version_cmp_part_edge_cases() {
        // Test cases that should hit the missed mutants in version_cmp_part

        // Test empty strings
        assert_eq!(version_cmp_part("", ""), Ordering::Equal);
        assert_eq!(version_cmp_part("", "1"), Ordering::Less);
        assert_eq!(version_cmp_part("1", ""), Ordering::Greater);

        // Test digit comparison edge cases
        assert_eq!(version_cmp_part("1", "1"), Ordering::Equal);
        assert_eq!(version_cmp_part("01", "1"), Ordering::Equal);

        // Test very long digit sequences to hit BigInt path
        let long_a = "123456789012345678901234567890";
        let long_b = "123456789012345678901234567891";
        assert_eq!(version_cmp_part(long_a, long_b), Ordering::Less);

        // Test mixed digit/non-digit sequences
        assert_eq!(version_cmp_part("1a2", "1a3"), Ordering::Less);
        assert_eq!(version_cmp_part("1a2", "1b1"), Ordering::Less);

        // Test tilde handling
        assert_eq!(version_cmp_part("1~", "1"), Ordering::Less);
        assert_eq!(version_cmp_part("~1", "1"), Ordering::Less);
    }

    #[test]
    fn test_canonicalize_edge_cases() {
        // Test cases that should hit missed mutants in canonicalize

        // Test epoch handling
        let v1 = Version {
            epoch: Some(0),
            upstream_version: "1.0".to_string(),
            debian_revision: None,
        };
        let canonical = v1.canonicalize();
        assert_eq!(canonical.epoch, None);

        // Test debian revision all zeros
        let v2 = Version {
            epoch: None,
            upstream_version: "1.0".to_string(),
            debian_revision: Some("000".to_string()),
        };
        let canonical2 = v2.canonicalize();
        assert_eq!(canonical2.debian_revision, None);

        // Test debian revision with leading zeros
        let v3 = Version {
            epoch: None,
            upstream_version: "1.0".to_string(),
            debian_revision: Some("001".to_string()),
        };
        let canonical3 = v3.canonicalize();
        assert_eq!(canonical3.debian_revision, Some("1".to_string()));

        // Test upstream version with leading zeros
        let v4 = Version {
            epoch: None,
            upstream_version: "001.0".to_string(),
            debian_revision: None,
        };
        let canonical4 = v4.canonicalize();
        assert_eq!(canonical4.upstream_version, "1.0");

        // Test unchanged cases
        let v5 = Version {
            epoch: Some(1),
            upstream_version: "1.0".to_string(),
            debian_revision: Some("1".to_string()),
        };
        let canonical5 = v5.canonicalize();
        assert_eq!(canonical5.upstream_version, "1.0");
        assert_eq!(canonical5.debian_revision, Some("1".to_string()));
    }

    #[test]
    fn test_partial_eq_false() {
        // Test PartialEq returning false to catch the missed mutant
        assert!("1.0-1"
            .parse::<Version>()
            .unwrap()
            .ne(&"1.0-2".parse::<Version>().unwrap()));

        assert!("1.0-1"
            .parse::<Version>()
            .unwrap()
            .ne(&"2.0-1".parse::<Version>().unwrap()));

        assert!("1:1.0-1"
            .parse::<Version>()
            .unwrap()
            .ne(&"2:1.0-1".parse::<Version>().unwrap()));
    }

    #[test]
    fn test_non_digit_cmp_edge_cases() {
        use super::non_digit_cmp;

        // Test tilde vs regular chars
        assert_eq!(non_digit_cmp("~", "a"), Ordering::Less);
        assert_eq!(non_digit_cmp("~", "A"), Ordering::Less);
        assert_eq!(non_digit_cmp("~", "!"), Ordering::Less);

        // Test special character ordering
        assert_eq!(non_digit_cmp("!", "@"), Ordering::Less);
        assert_eq!(non_digit_cmp("@", "A"), Ordering::Greater);
        assert_eq!(non_digit_cmp("Z", "["), Ordering::Less);

        // Test empty strings
        assert_eq!(non_digit_cmp("", ""), Ordering::Equal);
        assert_eq!(non_digit_cmp("", "a"), Ordering::Less);
        assert_eq!(non_digit_cmp("a", ""), Ordering::Greater);
    }

    #[test]
    fn test_parse_lenient() {
        // Test parsing version with underscores (violates Debian Policy but exists in the wild)
        let v = Version::parse_lenient("2.0.37+cvs.JCW_PRE2_2037-1").unwrap();
        assert_eq!(v.epoch, None);
        assert_eq!(v.upstream_version, "2.0.37+cvs.JCW_PRE2_2037");
        assert_eq!(v.debian_revision, Some("1".to_string()));

        // Test with epoch and underscores
        let v2 = Version::parse_lenient("1:3.5_beta-2").unwrap();
        assert_eq!(v2.epoch, Some(1));
        assert_eq!(v2.upstream_version, "3.5_beta");
        assert_eq!(v2.debian_revision, Some("2".to_string()));

        // Test with underscores in debian revision
        let v3 = Version::parse_lenient("1.0-ubuntu_1").unwrap();
        assert_eq!(v3.epoch, None);
        assert_eq!(v3.upstream_version, "1.0");
        assert_eq!(v3.debian_revision, Some("ubuntu_1".to_string()));

        // Test that standard versions still work with lenient parsing
        let v4 = Version::parse_lenient("1.0-1").unwrap();
        assert_eq!(v4.epoch, None);
        assert_eq!(v4.upstream_version, "1.0");
        assert_eq!(v4.debian_revision, Some("1".to_string()));
    }

    #[test]
    fn test_parse_strict_rejects_underscores() {
        // Verify that the strict parser (FromStr) still rejects underscores
        assert!("2.0.37+cvs.JCW_PRE2_2037-1".parse::<Version>().is_err());
        assert!("3.5_beta-1".parse::<Version>().is_err());
        assert!("1.0-ubuntu_1".parse::<Version>().is_err());

        // But standard versions should still parse fine
        assert!("1.0-1".parse::<Version>().is_ok());
        assert!("1:2.0+really1.0-1".parse::<Version>().is_ok());
    }
}
