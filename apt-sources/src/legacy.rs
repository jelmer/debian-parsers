//! A module for parsing and manipulating APT source files that
//! use the pre-DEB822 single line format to hold package repositories specifications.
//!
//! # Examples
//! ```
//! # use url::Url;
//! # use apt_sources::legacy::LegacyRepositories;
//! # use std::str::FromStr;
//! let single_line = "deb http://archive.ubuntu.com/ubuntu jammy main restricted";
//! let repositories = LegacyRepositories::from_str(single_line)
//!     .expect("Shall not fail for correct list entry!");
//! assert_eq!(repositories.len(), 1);
//! let repository = repositories.iter().nth(0).expect("Shall not fail for first line");
//! assert_eq!(repository.uri, "http://archive.ubuntu.com/ubuntu".parse::<Url>().unwrap());
//! ```
use super::RepositoryError;
use super::RepositoryType;
use super::Signature;
use super::YesNoForce;
use itertools::Itertools;
use regex::Regex;
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Display;
use std::ops::Deref;
use std::ops::Not;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;
use url::Url;

/// A structure representing APT repository as declared by one-line-style `.list` file:
/// ```text
/// type [option=value option=value...] uri suite [component] [component] [...]
/// ```
/// According to `sources.list(5)` man pages, only four fields are mandatory:
/// * `type` either `deb` or `deb-src`
/// * `uri` to repository holding valid APT structure
/// * `suite` usually being distribution codename
/// * `component` most of the time `main`, but it's a section of the repository; multiple values allowed
///
/// The disabled field is just commented out with `#` followed by whitespaces at the beginning of the valid line
///
/// The manpage specifies following optional fields
/// * `arch`           comma separated list of binary architectures
/// * `lang`           comma separated list of supported natural languages
/// * `target`
/// * `pdiffs`         is a yes/no field
/// * `by-hash`        is a yes/no/force field
/// * `allow-insecure` is a yes/no field, default no
/// * `allow-weak`     is a yes/no field, default no
/// * `allow-downgrade-to-insecure` is a yes/no field, default no
/// * `trusted`        us a yes/no field
/// * `signed-by`      is a path to the key or fingerprint; optionally followed by exclamation mark
/// * `check-valid-until` is a yes/no field
/// * `valid-until-min`
/// * `valid-until-max`
/// * `check-date`     is a yes/no field
/// * `date-max-future`
/// * `inrelease-path` relative path
/// * `snapshot`       either `enable` or a snapshot ID
///
/// Note: this module doesn't support undocumented options.
#[derive(Clone, PartialEq, /*Eq,*/ Debug)]
pub struct LegacyRepository {
    /// This doesn't represent real field, but rather commented or uncommented line
    enabled: bool,
    /// Legacy lists format support one type per line
    pub typ: RepositoryType,
    /// Single repo address; according to Debian that's URI, but this type is more advanced than URI from `http` crate
    pub uri: Url,
    /// The distribution name as codename or suite type (like `stable` or `testing`)
    pub suite: String,
    /// (Optional) Section of the repository, usually `main`, `contrib` or `non-free`
    /// return `None` if repository is Flat Repository Format (<https://wiki.debian.org/DebianRepository/Format#Flat_Repository_Format>)
    pub components: Vec<String>,

    /// (Optional) Architectures binaries from this repository run on
    pub architectures: Vec<String>, // arch
    /// (Optional) Translations support to download
    pub languages: Vec<String>, // lang
    /// (Optional) Download targets to acquire from this source
    pub targets: Vec<String>, // target
    /// (Optional) Controls if APT should try PDiffs instead of downloading indexes entirely; if not set defaults to configuration option `Acquire::PDiffs`
    pub pdiffs: Option<bool>, // pdiffs
    /// (Optional) Controls if APT should try to acquire indexes via a URI constructed from a hashsum of the expected file
    pub by_hash: Option<YesNoForce>, // by-hash
    /// (Optional) If yes circumvents parts of `apt-secure`, don't thread lightly
    pub allow_insecure: bool, // allow-insecure, default no
    /// (Optional) If yes circumvents parts of `apt-secure`, don't thread lightly
    pub allow_weak: bool, // allow-weak, default no
    /// (Optional) If yes circumvents parts of `apt-secure`, don't thread lightly
    pub allow_downgrade_to_insecure: bool, // allow-downgrade-to-insecure, default no
    /// (Optional) If set forces whether APT considers source as trusted or no (default not present is a third state)
    pub trusted: Option<bool>, // trusted
    /// (Optional) Contains either absolute path to GPG keyring or embedded GPG public key block, if not set APT uses all trusted keys;
    /// I can't find example of using with fingerprints
    pub signature: Option<Signature>, // signed-by
}

impl Default for LegacyRepository {
    fn default() -> Self {
        Self {
            enabled: true,
            typ: RepositoryType::Binary,
            uri: "http://nowhere.com".parse().unwrap(),
            suite: "none".to_string(),
            components: vec![],
            architectures: vec![],
            languages: vec![],
            targets: vec![],
            pdiffs: None,
            by_hash: None,
            allow_insecure: false,
            allow_weak: false,
            allow_downgrade_to_insecure: false,
            trusted: None,
            signature: None,
        }
    }
}

impl LegacyRepository {
    /// In the ideal world we'd manage to use deserialization from a new format handling, but I'm not there yet to lift this for this format
    fn assign_option_field(&mut self, key: &str, value: &str) -> Result<(), RepositoryError> {
        match key {
            "arch" => self.architectures = value.split(',').map(|s| s.to_string()).collect(),
            "lang" => self.languages = value.split(',').map(|s| s.to_string()).collect(),
            "target" => self.targets = value.split(',').map(|s| s.to_string()).collect(),
            "pdiffs" => self.pdiffs = Some(super::deserialize_yesno(value)?),
            "by-hash" => self.by_hash = Some(YesNoForce::from_str(value)?),
            "allow-insecure" => self.allow_insecure = super::deserialize_yesno(value)?, // , default no
            "allow-weak" => self.allow_weak = super::deserialize_yesno(value)?, // , default no
            "allow-downgrade-to-insecure" => {
                self.allow_downgrade_to_insecure = super::deserialize_yesno(value)?
            } // , default no
            "trusted" => self.trusted = Some(super::deserialize_yesno(value)?), // default not present is a third state
            "signed-by" => self.signature = Some(Signature::KeyPath(PathBuf::from(value))),
            any => return Err(RepositoryError::UnrecognizedFieldName(any.to_string())),
        };
        Ok(())
    }
}

/// Container for multiple `LegacyRepository` specifications as single `.list` file may contain as per specification
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyRepositories(Vec<LegacyRepository>);

impl LegacyRepositories {
    /// Creates empty container of repositories
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    /// Creates repositories from container consisting `Repository` instances
    pub fn new<Container>(container: Container) -> Self
    where
        Container: Into<Vec<LegacyRepository>>,
    {
        Self(container.into())
    }

    /// Provides iterator over individual repositories in the whole file
    pub fn repositories(&self) -> impl Iterator<Item = &LegacyRepository> {
        // TODO: that's by ref, not compatible with lossless
        self.0.iter()
    }

    /// Push a new repository
    pub fn push(&mut self, repo: LegacyRepository) {
        self.0.push(repo);
    }

    /// Retain repositories matching a predicate
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&LegacyRepository) -> bool,
    {
        self.0.retain(f);
    }

    /// Get mutable iterator over repositories
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, LegacyRepository> {
        self.0.iter_mut()
    }

    /// Extend with an iterator of repositories
    pub fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = LegacyRepository>,
    {
        self.0.extend(iter);
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

static RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?xm)^
        (?P<type>deb|deb-src)\s+                   # Catch repository type
        (\[(?P<options>[^]]*)]\s+)?                  # Catch options
        (?P<uri>\S+)\s+                            # Catch repository URI
        (?P<suite>\S+)\s+                          # Catch suite/distribution
        (?P<components>(?:(?P<component>\w+)\s?)+) # Catch components (multiple)
        $",
    )
    .expect("Tested correct regular expression shall not fail!")
});

/// It only make sense to convert multiple lines at once as typical `.list` file has one uncommented
/// and one commented (`deb-src`) entry
impl FromStr for LegacyRepositories {
    type Err = RepositoryError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let elements = RE
            .captures_iter(text)
            .map(|caps| {
                let mut repository = LegacyRepository::default();
                repository.typ = RepositoryType::from_str(&caps["type"])?;
                let options = caps.name("options").map(|o| o.as_str()).unwrap_or("");
                options
                    .trim_matches(|c| c == '[' || c == ']')
                    .split_whitespace()
                    .map(|o| {
                        o.splitn(2, '=')
                            .collect_tuple::<(&str, &str)>()
                            .ok_or(RepositoryError::InvalidFormat)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .try_for_each(|(k, v)| repository.assign_option_field(k, v))?;
                repository.uri = Url::from_str(&caps["uri"])?;
                repository.suite = caps["suite"].to_owned();
                repository
                    .components
                    .extend(caps["components"].split_whitespace().map(|c| c.to_owned()));
                <Result<LegacyRepository, Self::Err>>::Ok(repository)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self(elements))
    }
}

impl Deref for LegacyRepositories {
    type Target = Vec<LegacyRepository>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&LegacyRepository> for super::Repository {
    fn from(original: &LegacyRepository) -> Self {
        Self {
            enabled: Some(original.enabled), // TODO: more valid one would be if true -> None else `Some(false)`...
            types: HashSet::from([original.typ.clone()]),
            uris: vec![original.uri.clone()],
            suites: vec![original.suite.clone()],
            components: original.components.clone().into(),
            architectures: (!original.architectures.is_empty())
                .then_some(original.architectures.clone()),
            languages: (!original.languages.is_empty()).then_some(original.languages.clone()),
            targets: (!original.targets.is_empty()).then_some(original.targets.clone()),
            pdiffs: original.pdiffs,
            by_hash: original.by_hash,
            allow_insecure: original.allow_insecure.then_some(true),
            allow_weak: original.allow_weak.then_some(true),
            allow_downgrade_to_insecure: original.allow_downgrade_to_insecure.then_some(true),
            trusted: original.trusted,
            signature: original.signature.clone(),
            x_repolib_name: None,
            description: None,
        }
    }
}

impl From<LegacyRepository> for super::Repository {
    fn from(original: LegacyRepository) -> Self {
        Self {
            enabled: Some(original.enabled), // TODO: more valid one would be if true -> None else `Some(false)`...
            types: HashSet::from([original.typ]),
            uris: vec![original.uri],
            suites: vec![original.suite],
            components: original.components.into(),
            architectures: (!original.architectures.is_empty()).then_some(original.architectures),
            languages: (!original.languages.is_empty()).then_some(original.languages),
            targets: (!original.targets.is_empty()).then_some(original.targets),
            pdiffs: original.pdiffs,
            by_hash: original.by_hash,
            allow_insecure: original.allow_insecure.then_some(true),
            allow_weak: original.allow_weak.then_some(true),
            allow_downgrade_to_insecure: original.allow_downgrade_to_insecure.then_some(true),
            trusted: original.trusted,
            signature: original.signature,
            x_repolib_name: None,
            description: None,
        }
    }
}

impl From<&LegacyRepositories> for super::Repositories {
    fn from(original: &LegacyRepositories) -> Self {
        Self(original.iter().map(|v| v.into()).collect())
    }
}

impl From<LegacyRepositories> for super::Repositories {
    fn from(original: LegacyRepositories) -> Self {
        Self(original.0.into_iter().map(|v| v.into()).collect())
    }
}

impl From<&super::Repository> for LegacyRepositories {
    /// Convert a DEB822 Repository to legacy format lines.
    /// Since a Repository can have multiple types/uris/suites, this may produce multiple lines.
    fn from(repo: &super::Repository) -> Self {
        let mut repos = Vec::new();

        for typ in &repo.types {
            for uri in &repo.uris {
                for suite in &repo.suites {
                    repos.push(LegacyRepository {
                        enabled: repo.enabled.unwrap_or(true),
                        typ: typ.clone(),
                        uri: uri.clone(),
                        suite: suite.clone(),
                        components: repo.components.clone().unwrap_or_default(),
                        architectures: repo.architectures.clone().unwrap_or_default(),
                        languages: repo.languages.clone().unwrap_or_default(),
                        targets: repo.targets.clone().unwrap_or_default(),
                        pdiffs: repo.pdiffs,
                        by_hash: repo.by_hash,
                        allow_insecure: repo.allow_insecure.unwrap_or(false),
                        allow_weak: repo.allow_weak.unwrap_or(false),
                        allow_downgrade_to_insecure: repo
                            .allow_downgrade_to_insecure
                            .unwrap_or(false),
                        trusted: repo.trusted,
                        signature: repo.signature.clone(),
                    });
                }
            }
        }

        LegacyRepositories(repos)
    }
}

fn option_output<O: AsRef<str> + Display>(name: &str, option: &[O]) -> Cow<'static, str> {
    if option.is_empty() {
        Cow::Borrowed("")
    } else {
        Cow::Owned(format!("{name}={}", option.iter().join(",")))
    }
}

impl Display for LegacyRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.typ)?;
        // TODO: all options if any

        let options = vec![
            option_output("arch", &self.architectures),
            option_output("lang", &self.languages),
            option_output("target", &self.targets),
            self.pdiffs
                .map(|p| Cow::Owned(format!("pdiff={}", if p { "yes" } else { "no" })))
                .unwrap_or(Cow::Borrowed("")),
            self.by_hash
                .map(|p| Cow::Owned(format!("by-hash={p}")))
                .unwrap_or(Cow::Borrowed("")),
            if self.allow_insecure {
                Cow::Owned("allow-insecure=yes".to_string())
            } else {
                Cow::Borrowed("")
            },
            if self.allow_weak {
                Cow::Owned("allow-weak=yes".to_string())
            } else {
                Cow::Borrowed("")
            },
            if self.allow_downgrade_to_insecure {
                Cow::Owned("allow-downgrade-to-insecure=yes".to_string())
            } else {
                Cow::Borrowed("")
            },
            self.trusted
                .map(|t| Cow::Owned(format!("trusted={}", if t { "yes" } else { "no" })))
                .unwrap_or(Cow::Borrowed("")),
            self.signature
                .as_ref()
                .map(|s| {
                    if let Signature::KeyPath(ref p) = s {
                        Cow::Owned(format!("signed-by={}", p.display()))
                    } else {
                        panic!("Short format not supported!") // TODO: design bug of LegacyRepository!
                    }
                })
                .unwrap_or(Cow::Borrowed("")),
        ];
        let options = options.iter().filter(|s| !s.is_empty()).join(" ");
        options.is_empty().not().then(|| write!(f, " [{options}]"));
        write!(f, " {}", self.uri)?;
        write!(f, " {}", self.suite)?;
        write!(f, " {}", self.components.join(" "))
    }
}

impl Display for LegacyRepositories {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, repo) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{}", repo)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Repository;

    use super::*;
    use indoc::indoc;

    const LONG_SAMPLE: &str = indoc!("
        deb [arch=arm64 signed-by=/usr/share/keyrings/rcn-ee-archive-keyring.gpg] http://debian.beagleboard.org/arm64/ jammy main
    ");
    const SHORT_SAMPLE: &str = indoc!(
        "
        deb http://archive.ubuntu.com/ubuntu jammy main restricted
        deb-src http://archive.ubuntu.com/ubuntu jammy main restricted
    "
    );
    const COMMENTED_SAMPLE: &str = indoc!(
        "
        deb http://archive.ubuntu.com/ubuntu jammy main restricted
        # deb-src http://archive.ubuntu.com/ubuntu jammy main restricted
    "
    );

    fn golden_sample() -> Repository {
        // TODO: qualifies for lazy_static
        Repository {
            enabled: Some(true), // TODO: looks odd, as only `Enabled: no` in meaningful
            types: HashSet::from([RepositoryType::Binary]),
            architectures: Some(vec!["arm64".to_owned()]),
            uris: vec![Url::from_str("http://debian.beagleboard.org/arm64/").unwrap()],
            suites: vec!["jammy".to_owned()],
            components: Some(vec!["main".to_owned()]),
            signature: Some(Signature::KeyPath(PathBuf::from(
                "/usr/share/keyrings/rcn-ee-archive-keyring.gpg",
            ))),
            x_repolib_name: None,
            languages: None,
            targets: None,
            pdiffs: None,
            ..Default::default()
        }
    }

    #[test]
    fn test_legacy_repositories_from_str() {
        let repositories = LegacyRepositories::from_str(LONG_SAMPLE)
            .expect("Shall not fail for correct list entry!");

        assert_eq!(repositories.len(), 1);
        let repository = repositories.iter().nth(0).unwrap();

        assert_eq!(repository.enabled, true);
        assert_eq!(repository.typ, RepositoryType::Binary);
        assert_eq!(repository.architectures, vec!["arm64".to_owned()]);
        assert_eq!(
            repository.signature,
            Some(Signature::KeyPath(PathBuf::from(
                "/usr/share/keyrings/rcn-ee-archive-keyring.gpg"
            )))
        );
        assert_eq!(repository.typ, RepositoryType::Binary);
        assert_eq!(
            repository.uri,
            "http://debian.beagleboard.org/arm64/"
                .parse::<Url>()
                .unwrap()
        );
        assert_eq!(repository.suite, "jammy".to_owned());
        assert_eq!(repository.components, vec!["main".to_owned()]);
    }

    #[test]
    fn test_short_legacy_repositories_from_str() {
        let repositories = LegacyRepositories::from_str(SHORT_SAMPLE)
            .expect("Shall not fail for correct list entry!");

        assert_eq!(repositories.len(), 2);
        let bin_repository = repositories.iter().nth(0).unwrap();
        let src_repository = repositories.iter().nth(1).unwrap();

        assert_eq!(bin_repository.typ, RepositoryType::Binary);
        assert_eq!(src_repository.typ, RepositoryType::Source);
        assert_eq!(bin_repository.architectures.len(), 0);
        assert_eq!(src_repository.architectures.len(), 0);
        assert_eq!(bin_repository.components.len(), 2);
        assert_eq!(src_repository.components.len(), 2);
    }

    #[test]
    #[ignore = "commented lines support not yet implemented"]
    fn test_commented_legacy_repositories_from_str() {
        let repositories = LegacyRepositories::from_str(COMMENTED_SAMPLE)
            .expect("Shall not fail for correct list entry!");

        assert_eq!(repositories.len(), 2);
        let bin_repository = repositories.iter().nth(0).unwrap();
        let src_repository = repositories.iter().nth(1).unwrap();

        assert_eq!(bin_repository.enabled, true);
        assert_eq!(bin_repository.enabled, false);
        assert_eq!(bin_repository.typ, RepositoryType::Binary);
        assert_eq!(src_repository.typ, RepositoryType::Source);
        assert_eq!(bin_repository.architectures.len(), 0);
        assert_eq!(src_repository.architectures.len(), 0);
        assert_eq!(bin_repository.components.len(), 2);
        assert_eq!(src_repository.components.len(), 2);
    }

    #[test]
    fn test_conversion_from_legacy_to_deb822() {
        let repositories = LegacyRepositories::from_str(LONG_SAMPLE)
            .expect("Shall not fail for correct list entry!");

        assert_eq!(repositories.len(), 1);
        let legacy_repository = repositories.iter().nth(0).unwrap();

        let deb822_repository = Repository::from(legacy_repository);
        let golden_sample = golden_sample();

        assert_eq!(golden_sample, deb822_repository);
    }

    #[test]
    fn test_moving_conversion_from_legacy_to_deb822() {
        let mut repositories = LegacyRepositories::from_str(LONG_SAMPLE)
            .expect("Shall not fail for correct list entry!");

        assert_eq!(repositories.len(), 1);
        let legacy_repository = repositories.0.pop().unwrap(); // TODO: To make it work for user we'd need `DerefMut` but I'm reluctant

        let deb822_repository = Repository::from(legacy_repository);
        let golden_sample = golden_sample();

        assert_eq!(golden_sample, deb822_repository);
    }

    #[test]
    fn test_display_of_simple_legacy_repository() {
        let sample = LegacyRepository {
            enabled: true,
            typ: RepositoryType::Binary,
            uri: "http://debian.beagleboard.org/arm64/".parse().unwrap(),
            suite: "jammy".to_string(),
            components: vec!["main".to_string()],
            architectures: vec![],
            languages: vec![],
            targets: vec![],
            pdiffs: None,
            by_hash: None,
            allow_insecure: false,
            allow_weak: false,
            allow_downgrade_to_insecure: false,
            trusted: None,
            signature: None,
        };
        let list_text = sample.to_string();

        assert_eq!(
            list_text,
            "deb http://debian.beagleboard.org/arm64/ jammy main"
        )
    }

    #[test]
    fn test_display_of_legacy_repository_with_options() {
        let sample = LegacyRepository {
            enabled: true,
            typ: RepositoryType::Binary,
            uri: "http://debian.beagleboard.org/arm64/".parse().unwrap(),
            suite: "jammy".to_string(),
            components: vec!["main".to_string()],
            architectures: vec!["amd64".to_string()],
            languages: vec![],
            targets: vec![],
            pdiffs: None,
            by_hash: None,
            allow_insecure: false,
            allow_weak: false,
            allow_downgrade_to_insecure: false,
            trusted: None,
            signature: Some(Signature::KeyPath(PathBuf::from(
                "/usr/share/keyrings/rcn-ee-archive-keyring.gpg",
            ))), // TODO: `.list` supports only key files, no way to fit PGP block
        };
        let list_text = sample.to_string();

        assert_eq!(
            list_text,
            "deb [arch=amd64 signed-by=/usr/share/keyrings/rcn-ee-archive-keyring.gpg] http://debian.beagleboard.org/arm64/ jammy main"
        )
    }

    #[test]
    fn test_conversion_from_deb822_to_legacy() {
        use std::collections::HashSet;

        let repo = Repository {
            enabled: Some(true),
            types: HashSet::from([RepositoryType::Binary, RepositoryType::Source]),
            uris: vec!["http://archive.ubuntu.com/ubuntu".parse().unwrap()],
            suites: vec!["jammy".to_string()],
            components: Some(vec!["main".to_string(), "universe".to_string()]),
            architectures: Some(vec!["amd64".to_string()]),
            ..Default::default()
        };

        let legacy = LegacyRepositories::from(&repo);
        assert_eq!(legacy.len(), 2); // One for deb, one for deb-src

        let legacy_str = legacy.to_string();
        assert!(legacy_str.contains("deb [arch=amd64]"));
        assert!(legacy_str.contains("deb-src [arch=amd64]"));
        assert!(legacy_str.contains("http://archive.ubuntu.com/ubuntu"));
        assert!(legacy_str.contains("jammy main universe"));
    }

    #[test]
    fn test_legacy_repositories_display() {
        let repos = LegacyRepositories(vec![
            LegacyRepository {
                enabled: true,
                typ: RepositoryType::Binary,
                uri: "http://example.com/ubuntu".parse().unwrap(),
                suite: "jammy".to_string(),
                components: vec!["main".to_string()],
                ..Default::default()
            },
            LegacyRepository {
                enabled: true,
                typ: RepositoryType::Source,
                uri: "http://example.com/ubuntu".parse().unwrap(),
                suite: "jammy".to_string(),
                components: vec!["main".to_string()],
                ..Default::default()
            },
        ]);

        let display = repos.to_string();
        assert_eq!(
            display,
            "deb http://example.com/ubuntu jammy main\ndeb-src http://example.com/ubuntu jammy main"
        );
    }

    #[test]
    fn test_allow_downgrade_to_insecure_parsing() {
        let input = "deb [allow-downgrade-to-insecure=yes] http://example.com/ubuntu jammy main\n";
        let repos = LegacyRepositories::from_str(input).unwrap();
        assert_eq!(repos.len(), 1);
        let repo = repos.iter().nth(0).unwrap();
        assert!(repo.allow_downgrade_to_insecure);
        assert!(!repo.allow_weak);
    }

    #[test]
    fn test_allow_downgrade_to_insecure_display() {
        let repo = LegacyRepository {
            enabled: true,
            typ: RepositoryType::Binary,
            uri: "http://example.com/ubuntu".parse().unwrap(),
            suite: "jammy".to_string(),
            components: vec!["main".to_string()],
            allow_downgrade_to_insecure: true,
            ..Default::default()
        };
        let text = repo.to_string();
        assert_eq!(
            text,
            "deb [allow-downgrade-to-insecure=yes] http://example.com/ubuntu jammy main"
        );
    }

    #[test]
    fn test_malformed_option_without_equals() {
        let input = "deb [badoption] http://example.com/ubuntu jammy main\n";
        let result = LegacyRepositories::from_str(input);
        assert!(result.is_err());
    }
}
