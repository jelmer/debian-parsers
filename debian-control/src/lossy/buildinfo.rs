//! Parser for Debian buildinfo files
//!
//! The buildinfo file format is a Debian-specific format that is used to store
//! information about the build environment of a package. See <https://wiki.debian.org/Buildinfo> for
//! more information.
use crate::lossy::relations::Relations;
use deb822_fast::FromDeb822Paragraph;
use deb822_fast::{FromDeb822, ToDeb822};
use std::collections::HashMap;
use std::path::PathBuf;

fn deserialize_env(s: &str) -> Result<HashMap<String, String>, String> {
    let mut env = HashMap::new();
    for line in s.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = match line.split_once("=") {
            Some((key, value)) => {
                // Strip surrounding double quotes, but only when there are two
                // of them: a lone `"` has len()==1 and would panic on 1..0.
                if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
                    let value = value[1..value.len() - 1].to_string();
                    (key, value)
                } else {
                    (key, value.to_string())
                }
            }
            None => {
                // If there is no '=', then the line is invalid
                return Err("Invalid environment variable".to_string());
            }
        };
        env.insert(key.to_string(), value.to_string());
    }
    Ok(env)
}

fn serialize_env(env: &HashMap<String, String>) -> String {
    let mut s = String::new();
    for (key, value) in env {
        s.push_str(&format!("{}={}\n", key, value));
    }
    s
}

fn deserialize_version(s: &str) -> Result<debversion::Version, String> {
    s.parse().map_err(|e: debversion::ParseError| e.to_string())
}

fn serialize_version(version: &debversion::Version) -> String {
    version.to_string()
}

fn serialize_pathbuf(path: &std::path::Path) -> String {
    path.display().to_string()
}

fn deserialize_pathbuf(s: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(s))
}

#[derive(FromDeb822, ToDeb822)]
/// The buildinfo file.
pub struct Buildinfo {
    #[deb822(field = "Format")]
    /// The format of the buildinfo file.
    format: String,

    #[deb822(field = "Build-Architecture")]
    /// The architecture the package is built on.
    build_architecture: String,

    #[deb822(field = "Source")]
    /// The name of the source package.
    source: String,

    #[deb822(field = "Binary")]
    /// Folded list of binary packages built from the source package.
    binary: Option<String>,

    #[deb822(field = "Architecture")]
    /// The architecture the package is built for.
    architecture: String,

    #[deb822(
        field = "Version",
        deserialize_with = deserialize_version,
        serialize_with = serialize_version
    )]
    /// The version number of a package.
    version: debversion::Version,

    #[deb822(field = "Binary-Only-Changes")]
    binary_only_changes: Option<String>,

    #[deb822(field = "Checksums-Sha256")]
    /// The SHA256 checksums of the files in the package.
    // TODO: Parse properly
    checksums_sha256: Option<String>,

    #[deb822(field = "Checksums-Sha1")]
    /// The SHA1 checksums of the files in the package.
    // TODO: Parse properly
    checksums_sha1: Option<String>,

    #[deb822(field = "Checksums-Md5")]
    /// The MD5 checksums of the files in the package.
    // TODO: Parse properly
    checksums_md5: Option<String>,

    #[deb822(field = "Build-Origin")]
    /// The origin of the build.
    build_origin: Option<String>,

    #[deb822(field = "Build-Date")]
    /// The date the package was built.
    build_date: Option<String>,

    #[deb822(field = "Build-Tainted-By")]
    /// The reason the build is tainted.
    build_tainted_by: Option<String>,

    #[deb822(field = "Build-Path", deserialize_with = deserialize_pathbuf, serialize_with = serialize_pathbuf)]
    /// Absolute path of the directory in which the package was built.
    build_path: Option<PathBuf>,

    #[deb822(
        field = "Environment",
        deserialize_with = deserialize_env,
        serialize_with = serialize_env
    )]
    /// Environment variables used during the build.
    environment: Option<HashMap<String, String>>,

    #[deb822(field = "Installed-Build-Depends")]
    /// The packages that this package depends on during build.
    installed_build_depends: Option<Relations>,
}

impl std::str::FromStr for Buildinfo {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let para: deb822_fast::Paragraph =
            s.parse().map_err(|e: deb822_fast::Error| e.to_string())?;
        Self::from_paragraph(&para)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_buildinfo() {
        let input = include_str!("../../testdata/ruff.buildinfo");

        let buildinfo = Buildinfo::from_str(input).unwrap();

        assert_eq!(buildinfo.format, "1.0");
    }

    #[test]
    fn test_environment_value_is_lone_quote() {
        // Regression: a value of just `"` used to panic in deserialize_env
        // because it tried to slice 1..0 after detecting "starts/ends with
        // double quote" on a one-byte string.
        let line = "FOO=\"";
        let env = super::deserialize_env(line).unwrap();
        assert_eq!(env.get("FOO"), Some(&"\"".to_string()));
    }
}
