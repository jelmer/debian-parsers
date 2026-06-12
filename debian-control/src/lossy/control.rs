use crate::fields::Priority;
use crate::lossy::relations::Relations;
use deb822_fast::{FromDeb822, ToDeb822};
use deb822_fast::{FromDeb822Paragraph, ToDeb822Paragraph};

fn deserialize_yesno(s: &str) -> Result<bool, String> {
    match s {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err(format!("invalid value for yesno: {}", s)),
    }
}

fn serialize_yesno(b: &bool) -> String {
    if *b {
        "yes".to_string()
    } else {
        "no".to_string()
    }
}

/// The source package.
#[derive(FromDeb822, ToDeb822, Default, Clone, PartialEq)]
pub struct Source {
    #[deb822(field = "Source")]
    /// The name of the source package.
    pub name: String,
    #[deb822(field = "Build-Depends")]
    /// The packages that this package depends on during build.
    pub build_depends: Option<Relations>,
    #[deb822(field = "Build-Depends-Indep")]
    /// The packages that this package depends on during build.
    pub build_depends_indep: Option<Relations>,
    #[deb822(field = "Build-Depends-Arch")]
    /// The packages that this package depends on during build.
    pub build_depends_arch: Option<Relations>,
    #[deb822(field = "Build-Conflicts")]
    /// The packages that this package conflicts with during build.
    pub build_conflicts: Option<Relations>,
    #[deb822(field = "Build-Conflicts-Indep")]
    /// The packages that this package conflicts with during build.
    pub build_conflicts_indep: Option<Relations>,
    #[deb822(field = "Build-Conflicts-Arch")]
    /// The packages that this package conflicts with during build.
    pub build_conflicts_arch: Option<Relations>,
    #[deb822(field = "Standards-Version")]
    /// The version of the Debian Policy Manual that the package complies with.
    pub standards_version: Option<String>,
    #[deb822(field = "Homepage")]
    /// The homepage of the package.
    pub homepage: Option<url::Url>,
    #[deb822(field = "Section")]
    /// The section of the package.
    pub section: Option<String>,
    #[deb822(field = "Priority")]
    /// The priority of the package.
    pub priority: Option<Priority>,
    #[deb822(field = "Maintainer")]
    /// The maintainer of the package.
    pub maintainer: Option<String>,
    #[deb822(field = "Uploaders")]
    /// The uploaders of the package.
    pub uploaders: Option<String>,
    #[deb822(field = "Architecture")]
    /// The architecture the package is built for.
    pub architecture: Option<String>,
    #[deb822(field = "Rules-Requires-Root", deserialize_with = deserialize_yesno, serialize_with = serialize_yesno)]
    /// Whether the package's build rules require root.
    pub rules_requires_root: Option<bool>,
    #[deb822(field = "Testsuite")]
    /// The name of the test suite.
    pub testsuite: Option<String>,
    #[deb822(field = "Vcs-Git")]
    /// The URL of the Git repository.
    pub vcs_git: Option<crate::vcs::ParsedVcs>,
    #[deb822(field = "Vcs-Browser")]
    /// The URL to the web interface of the VCS.
    pub vcs_browser: Option<url::Url>,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let para: deb822_fast::Paragraph = self.to_paragraph();
        write!(f, "{}", para)?;
        Ok(())
    }
}

impl Source {
    /// Return whether this package is maintained by the Debian QA team.
    ///
    /// Orphaned packages have their `Maintainer` field set to
    /// `Debian QA Group <packages@qa.debian.org>`.
    pub fn is_qa_package(&self) -> bool {
        self.maintainer
            .as_deref()
            .and_then(|m| crate::parse_identity(m).ok())
            .map(|(_, email)| email.eq_ignore_ascii_case("packages@qa.debian.org"))
            .unwrap_or(false)
    }
}

/// A binary package.
#[derive(FromDeb822, ToDeb822, Default, Clone, PartialEq)]
pub struct Binary {
    #[deb822(field = "Package")]
    /// The name of the package.
    pub name: String,
    #[deb822(field = "Depends")]
    /// The packages that this package depends on.
    pub depends: Option<Relations>,
    #[deb822(field = "Recommends")]
    /// The packages that this package recommends.
    pub recommends: Option<Relations>,
    #[deb822(field = "Suggests")]
    /// The packages that this package suggests.
    pub suggests: Option<Relations>,
    #[deb822(field = "Enhances")]
    /// The packages that this package enhances.
    pub enhances: Option<Relations>,
    #[deb822(field = "Pre-Depends")]
    /// The packages that this package depends on before it is installed.
    pub pre_depends: Option<Relations>,
    #[deb822(field = "Breaks")]
    /// The packages that this package breaks.
    pub breaks: Option<Relations>,
    #[deb822(field = "Conflicts")]
    /// The packages that this package conflicts with.
    pub conflicts: Option<Relations>,
    #[deb822(field = "Replaces")]
    /// The packages that this package replaces.
    pub replaces: Option<Relations>,
    #[deb822(field = "Provides")]
    /// The packages that this package provides.
    pub provides: Option<Relations>,
    #[deb822(field = "Built-Using")]
    /// The packages that this package is built using.
    pub built_using: Option<Relations>,
    #[deb822(field = "Static-Built-Using")]
    /// The packages that this package is statically built using.
    pub static_built_using: Option<Relations>,
    #[deb822(field = "Architecture")]
    /// The architecture the package is built for.
    pub architecture: Option<String>,
    #[deb822(field = "Section")]
    /// The section of the package.
    pub section: Option<String>,
    #[deb822(field = "Priority")]
    /// The priority of the package.
    pub priority: Option<Priority>,
    #[deb822(field = "Multi-Arch")]
    /// The multi-arch field.
    pub multi_arch: Option<crate::fields::MultiArch>,
    #[deb822(field = "Essential", deserialize_with = deserialize_yesno, serialize_with = serialize_yesno)]
    /// Whether the package is essential.
    pub essential: Option<bool>,
    #[deb822(field = "Description")]
    /// The description of the package. The first line is the short description, and the rest is the long description.
    pub description: Option<String>,
}

impl std::fmt::Display for Binary {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let para: deb822_fast::Paragraph = self.to_paragraph();
        write!(f, "{}", para)?;
        Ok(())
    }
}

/// A control file.
#[derive(Clone, PartialEq)]
pub struct Control {
    /// The source package.
    pub source: Source,
    /// The binary packages.
    pub binaries: Vec<Binary>,
}

impl std::fmt::Display for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.source)?;
        for binary in &self.binaries {
            f.write_str("\n")?;
            write!(f, "{}", binary)?;
        }
        Ok(())
    }
}

impl std::str::FromStr for Control {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let deb822: deb822_fast::Deb822 = s.parse().map_err(|e| format!("parse error: {}", e))?;

        let mut source: Option<Source> = None;
        let mut binaries: Vec<Binary> = Vec::new();

        for para in deb822.iter() {
            if para.get("Package").is_some() {
                let binary: Binary = Binary::from_paragraph(para)?;
                binaries.push(binary);
            } else if para.get("Source").is_some() {
                if source.is_some() {
                    return Err("more than one source paragraph".to_string());
                }
                source = Some(Source::from_paragraph(para)?);
            } else {
                return Err("paragraph without Source or Package field".to_string());
            }
        }

        Ok(Control {
            source: source.ok_or_else(|| "no source paragraph".to_string())?,
            binaries,
        })
    }
}

impl Default for Control {
    fn default() -> Self {
        Self::new()
    }
}

impl Control {
    /// Create a new control file.
    pub fn new() -> Self {
        Self {
            source: Source::default(),
            binaries: Vec::new(),
        }
    }

    /// Add a new binary package to the control file.
    pub fn add_binary(&mut self, name: &str) -> &mut Binary {
        let binary = Binary {
            name: name.to_string(),
            ..Default::default()
        };
        self.binaries.push(binary);
        self.binaries.last_mut().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relations::VersionConstraint;
    #[test]
    fn test_parse() {
        let control: Control = r#"Source: foo
Section: libs
Priority: optional
Build-Depends: bar (>= 1.0.0), baz (>= 1.0.0)
Homepage: https://example.com

"#
        .parse()
        .unwrap();
        let source = &control.source;

        assert_eq!(source.name, "foo".to_owned());
        assert_eq!(source.section, Some("libs".to_owned()));
        assert_eq!(source.priority, Some(super::Priority::Optional));
        assert_eq!(
            source.homepage,
            Some("https://example.com".parse().unwrap())
        );
        let bd = source.build_depends.as_ref().unwrap();
        let mut entries = bd.iter().collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        let rel = entries[0].pop().unwrap();
        assert_eq!(rel.name, "bar");
        assert_eq!(
            rel.version,
            Some((
                VersionConstraint::GreaterThanEqual,
                "1.0.0".parse().unwrap()
            ))
        );
        let rel = entries[1].pop().unwrap();
        assert_eq!(rel.name, "baz");
        assert_eq!(
            rel.version,
            Some((
                VersionConstraint::GreaterThanEqual,
                "1.0.0".parse().unwrap()
            ))
        );
    }

    #[test]
    fn test_description() {
        let control: Control = r#"Source: foo

Package: foo
Description: this is the short description
 And the longer one
 .
 is on the next lines
"#
        .parse()
        .unwrap();
        let binary = &control.binaries[0];
        assert_eq!(
            binary.description,
            Some(
                "this is the short description\nAnd the longer one\n.\nis on the next lines"
                    .to_owned()
            )
        );
    }

    #[test]
    fn test_is_qa_package() {
        let control: Control = r#"Source: foo
Maintainer: Debian QA Group <packages@qa.debian.org>
"#
        .parse()
        .unwrap();
        assert!(control.source.is_qa_package());

        let control: Control = r#"Source: foo
Maintainer: Jane Packager <jane@example.com>
"#
        .parse()
        .unwrap();
        assert!(!control.source.is_qa_package());

        let control: Control = r#"Source: foo
"#
        .parse()
        .unwrap();
        assert!(!control.source.is_qa_package());
    }
}
