//! APT related structures
use crate::lossy::{Relations, SourceRelation};
use deb822_fast::{FromDeb822, FromDeb822Paragraph, ToDeb822, ToDeb822Paragraph};

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

fn deserialize_components(value: &str) -> Result<Vec<String>, String> {
    Ok(value.split_whitespace().map(|s| s.to_string()).collect())
}

fn join_whitespace(components: &[String]) -> String {
    components.join(" ")
}

fn deserialize_architectures(value: &str) -> Result<Vec<String>, String> {
    Ok(value.split_whitespace().map(|s| s.to_string()).collect())
}

#[derive(Debug, Clone, PartialEq, Eq, ToDeb822, FromDeb822)]
/// A Release file
pub struct Release {
    #[deb822(field = "Codename")]
    /// The codename of the release
    pub codename: Option<String>,

    #[deb822(
        field = "Components",
        deserialize_with = deserialize_components,
        serialize_with = join_whitespace
    )]
    /// Components supported by the release
    pub components: Vec<String>,

    #[deb822(
        field = "Architectures",
        deserialize_with = deserialize_architectures,
        serialize_with = join_whitespace
    )]
    /// Architectures supported by the release
    pub architectures: Vec<String>,

    #[deb822(field = "Description")]
    /// Description of the release
    pub description: Option<String>,

    #[deb822(field = "Origin")]
    /// Origin of the release
    pub origin: Option<String>,

    #[deb822(field = "Label")]
    /// Label of the release
    pub label: Option<String>,

    #[deb822(field = "Suite")]
    /// Suite of the release
    pub suite: Option<String>,

    #[deb822(field = "Version")]
    /// Version of the release
    pub version: Option<String>,

    #[deb822(field = "Date")]
    /// Date the release was published
    pub date: Option<String>,

    #[deb822(field = "NotAutomatic", deserialize_with = deserialize_yesno, serialize_with = serialize_yesno)]
    /// Whether the release is not automatic
    pub not_automatic: Option<bool>,

    #[deb822(field = "ButAutomaticUpgrades", deserialize_with = deserialize_yesno, serialize_with = serialize_yesno)]
    /// Indicates if packages retrieved from this release should be automatically upgraded
    pub but_automatic_upgrades: Option<bool>,

    #[deb822(field = "Acquire-By-Hash", deserialize_with = deserialize_yesno, serialize_with = serialize_yesno)]
    /// Whether packages files can be acquired by hash
    pub acquire_by_hash: Option<bool>,

    #[deb822(field = "MD5Sum", deserialize_with = deserialize_checksums::<crate::fields::Md5Checksum>, serialize_with = serialize_checksums::<crate::fields::Md5Checksum>)]
    /// MD5 checksums of repository index files
    pub checksums_md5: Option<Vec<crate::fields::Md5Checksum>>,

    #[deb822(field = "SHA1", deserialize_with = deserialize_checksums::<crate::fields::Sha1Checksum>, serialize_with = serialize_checksums::<crate::fields::Sha1Checksum>)]
    /// SHA-1 checksums of repository index files
    pub checksums_sha1: Option<Vec<crate::fields::Sha1Checksum>>,

    #[deb822(field = "SHA256", deserialize_with = deserialize_checksums::<crate::fields::Sha256Checksum>, serialize_with = serialize_checksums::<crate::fields::Sha256Checksum>)]
    /// SHA-256 checksums of repository index files
    pub checksums_sha256: Option<Vec<crate::fields::Sha256Checksum>>,

    #[deb822(field = "SHA512", deserialize_with = deserialize_checksums::<crate::fields::Sha512Checksum>, serialize_with = serialize_checksums::<crate::fields::Sha512Checksum>)]
    /// SHA-512 checksums of repository index files
    pub checksums_sha512: Option<Vec<crate::fields::Sha512Checksum>>,
}

impl std::str::FromStr for Release {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let para = s
            .parse::<deb822_fast::Paragraph>()
            .map_err(|e| e.to_string())?;
        FromDeb822Paragraph::from_paragraph(&para)
    }
}

impl std::fmt::Display for Release {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let para: deb822_fast::Paragraph = self.to_paragraph();
        write!(f, "{}", para)
    }
}

fn deserialize_binaries(value: &str) -> Result<Vec<String>, String> {
    Ok(value.split(",").map(|s| s.trim().to_string()).collect())
}

fn deserialize_testsuite_triggers(value: &str) -> Result<Vec<String>, String> {
    Ok(value.split(",").map(|s| s.trim().to_string()).collect())
}

fn join_lines(components: &[String]) -> String {
    components.join("\n")
}

fn deserialize_package_list(value: &str) -> Result<Vec<String>, String> {
    Ok(value
        .split('\n')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

fn deserialize_checksums<T>(value: &str) -> Result<Vec<T>, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<T>().map_err(|e| e.to_string()))
        .collect()
}

fn serialize_checksums<T: std::fmt::Display>(checksums: &[T]) -> String {
    checksums
        .iter()
        .map(|c| format!(" {}", c))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq, ToDeb822, FromDeb822)]
/// A source
pub struct Source {
    #[deb822(field = "Directory")]
    /// The directory of the source
    pub directory: String,

    #[deb822(field = "Description")]
    /// Description of the source
    pub description: Option<String>,

    #[deb822(field = "Version")]
    /// Version of the source
    pub version: debversion::Version,

    #[deb822(field = "Package")]
    /// Package of the source
    pub package: String,

    #[deb822(field = "Binary", deserialize_with = deserialize_binaries, serialize_with = join_whitespace)]
    /// Binaries of the source
    pub binaries: Option<Vec<String>>,

    #[deb822(field = "Maintainer")]
    /// Maintainer of the source
    pub maintainer: Option<String>,

    #[deb822(field = "Uploaders")]
    /// Uploaders of the source
    pub uploaders: Option<String>,

    #[deb822(field = "Build-Depends")]
    /// Build dependencies of the source
    pub build_depends: Option<Relations>,

    #[deb822(field = "Build-Depends-Indep")]
    /// Build dependencies independent of the architecture of the source
    pub build_depends_indep: Option<Relations>,

    #[deb822(field = "Build-Depends-Arch")]
    /// Build dependencies dependent on the architecture of the source
    pub build_depends_arch: Option<Relations>,

    #[deb822(field = "Build-Conflicts")]
    /// Build conflicts of the source
    pub build_conflicts: Option<Relations>,

    #[deb822(field = "Build-Conflicts-Indep")]
    /// Build conflicts independent of the architecture of the source
    pub build_conflicts_indep: Option<Relations>,

    #[deb822(field = "Build-Conflicts-Arch")]
    /// Build conflicts dependent on the architecture of the source
    pub build_conflicts_arch: Option<Relations>,

    #[deb822(field = "Standards-Version")]
    /// Standards version of the source
    pub standards_version: Option<String>,

    #[deb822(field = "Homepage")]
    /// Homepage of the source
    pub homepage: Option<String>,

    #[deb822(field = "Autobuild")]
    /// Whether the source should be autobuilt
    pub autobuild: Option<bool>,

    #[deb822(field = "Extra-Source-Only", deserialize_with = deserialize_yesno, serialize_with = serialize_yesno)]
    /// Whether this is a source-only upload
    pub extra_source_only: Option<bool>,

    #[deb822(field = "Testsuite")]
    /// Testsuite of the source
    pub testsuite: Option<String>,

    #[deb822(field = "Testsuite-Triggers", deserialize_with = deserialize_testsuite_triggers, serialize_with = join_whitespace)]
    /// The packages triggering the testsuite of the source
    pub testsuite_triggers: Option<Vec<String>>,

    #[deb822(field = "Vcs-Browser")]
    /// VCS browser of the source
    pub vcs_browser: Option<String>,

    #[deb822(field = "Vcs-Git")]
    /// VCS Git of the source
    pub vcs_git: Option<String>,

    #[deb822(field = "Vcs-Bzr")]
    /// VCS Bzr of the source
    pub vcs_bzr: Option<String>,

    #[deb822(field = "Vcs-Hg")]
    /// VCS Hg of the source
    pub vcs_hg: Option<String>,

    #[deb822(field = "Vcs-Svn")]
    /// VCS SVN of the source
    pub vcs_svn: Option<String>,

    #[deb822(field = "Vcs-Darcs")]
    /// VCS Darcs of the source
    pub vcs_darcs: Option<String>,

    #[deb822(field = "Vcs-Cvs")]
    /// VCS CVS of the source
    pub vcs_cvs: Option<String>,

    #[deb822(field = "Vcs-Arch")]
    /// VCS Arch of the source
    pub vcs_arch: Option<String>,

    #[deb822(field = "Vcs-Mtn")]
    /// VCS Mtn of the source
    pub vcs_mtn: Option<String>,

    #[deb822(field = "Dgit")]
    /// Dgit information (commit, suite, ref, url)
    pub dgit: Option<crate::fields::DgitInfo>,

    #[deb822(field = "Priority")]
    /// Priority of the source
    pub priority: Option<crate::fields::Priority>,

    #[deb822(field = "Section")]
    /// Section of the source
    pub section: Option<String>,

    #[deb822(field = "Format")]
    /// Format of the source
    pub format: Option<String>,

    #[deb822(field = "Architecture")]
    /// Architectures the source builds for
    pub architecture: Option<String>,

    #[deb822(field = "Package-List", deserialize_with = deserialize_package_list, serialize_with = join_lines)]
    /// Package list of the source
    pub package_list: Option<Vec<String>>,

    #[deb822(field = "Files", deserialize_with = deserialize_checksums::<crate::fields::Md5Checksum>, serialize_with = serialize_checksums::<crate::fields::Md5Checksum>)]
    /// MD5 checksums of source files
    pub files: Option<Vec<crate::fields::Md5Checksum>>,

    #[deb822(field = "Checksums-Sha1", deserialize_with = deserialize_checksums::<crate::fields::Sha1Checksum>, serialize_with = serialize_checksums::<crate::fields::Sha1Checksum>)]
    /// SHA-1 checksums of source files
    pub checksums_sha1: Option<Vec<crate::fields::Sha1Checksum>>,

    #[deb822(field = "Checksums-Sha256", deserialize_with = deserialize_checksums::<crate::fields::Sha256Checksum>, serialize_with = serialize_checksums::<crate::fields::Sha256Checksum>)]
    /// SHA-256 checksums of source files
    pub checksums_sha256: Option<Vec<crate::fields::Sha256Checksum>>,

    #[deb822(field = "Checksums-Sha512", deserialize_with = deserialize_checksums::<crate::fields::Sha512Checksum>, serialize_with = serialize_checksums::<crate::fields::Sha512Checksum>)]
    /// SHA-512 checksums of source files
    pub checksums_sha512: Option<Vec<crate::fields::Sha512Checksum>>,
}

impl std::str::FromStr for Source {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let para = s
            .parse::<deb822_fast::Paragraph>()
            .map_err(|e| e.to_string())?;

        FromDeb822Paragraph::from_paragraph(&para)
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let para: deb822_fast::Paragraph = self.to_paragraph();
        write!(f, "{}", para)
    }
}

/// A package
#[derive(Debug, Clone, PartialEq, Eq, ToDeb822, FromDeb822)]
pub struct Package {
    /// The name of the package
    #[deb822(field = "Package")]
    pub name: String,

    /// The version of the package
    #[deb822(field = "Version")]
    pub version: debversion::Version,

    /// The name and version of the source package, if different from `name`
    #[deb822(field = "Source")]
    pub source: Option<SourceRelation>,

    /// The architecture of the package
    #[deb822(field = "Architecture")]
    pub architecture: String,

    /// The maintainer of the package
    #[deb822(field = "Maintainer")]
    pub maintainer: Option<String>,

    /// The installed size of the package
    #[deb822(field = "Installed-Size")]
    pub installed_size: Option<usize>,

    /// Dependencies
    #[deb822(field = "Depends")]
    pub depends: Option<Relations>,

    /// Pre-Depends
    #[deb822(field = "Pre-Depends")]
    pub pre_depends: Option<Relations>,

    /// Recommends
    #[deb822(field = "Recommends")]
    pub recommends: Option<Relations>,

    /// Suggests
    #[deb822(field = "Suggests")]
    pub suggests: Option<Relations>,

    /// Enhances
    #[deb822(field = "Enhances")]
    pub enhances: Option<Relations>,

    /// Breaks
    #[deb822(field = "Breaks")]
    pub breaks: Option<Relations>,

    /// Conflicts
    #[deb822(field = "Conflicts")]
    pub conflicts: Option<Relations>,

    /// Provides
    #[deb822(field = "Provides")]
    pub provides: Option<Relations>,

    /// Replaces
    #[deb822(field = "Replaces")]
    pub replaces: Option<Relations>,

    /// Built-Using
    #[deb822(field = "Built-Using")]
    pub built_using: Option<Relations>,

    /// Static-Built-Using
    #[deb822(field = "Static-Built-Using")]
    pub static_built_using: Option<Relations>,

    /// Description
    #[deb822(field = "Description")]
    pub description: Option<String>,

    /// Homepage
    #[deb822(field = "Homepage")]
    pub homepage: Option<String>,

    /// Origin
    #[deb822(field = "Origin")]
    pub origin: Option<String>,

    /// Priority
    #[deb822(field = "Priority")]
    pub priority: Option<crate::fields::Priority>,

    /// Section
    #[deb822(field = "Section")]
    pub section: Option<String>,

    /// Essential
    #[deb822(field = "Essential", deserialize_with = deserialize_yesno, serialize_with = serialize_yesno)]
    pub essential: Option<bool>,

    /// Multi-Arch
    #[deb822(field = "Multi-Arch")]
    pub multi_arch: Option<crate::fields::MultiArch>,

    /// Tag
    #[deb822(field = "Tag")]
    pub tag: Option<String>,

    /// Task
    #[deb822(field = "Task")]
    pub task: Option<String>,

    /// Size
    #[deb822(field = "Size")]
    pub size: Option<usize>,

    /// Filename (path within the repository)
    #[deb822(field = "Filename")]
    pub filename: Option<String>,

    /// MD5sum
    #[deb822(field = "MD5sum")]
    pub md5sum: Option<String>,

    /// SHA1
    #[deb822(field = "SHA1")]
    pub sha1: Option<String>,

    /// SHA256
    #[deb822(field = "SHA256")]
    pub sha256: Option<String>,

    /// SHA512
    #[deb822(field = "SHA512")]
    pub sha512: Option<String>,

    /// Description (MD5)
    #[deb822(field = "Description-MD5")]
    pub description_md5: Option<String>,
}

impl std::str::FromStr for Package {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let para = s
            .parse::<deb822_fast::Paragraph>()
            .map_err(|e| e.to_string())?;

        FromDeb822Paragraph::from_paragraph(&para)
    }
}

impl std::fmt::Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let para: deb822_fast::Paragraph = self.to_paragraph();
        write!(f, "{}", para)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deb822_fast::Paragraph;
    use deb822_fast::ToDeb822Paragraph;

    #[test]
    fn test_release() {
        let release = Release {
            codename: Some("focal".to_string()),
            components: vec!["main".to_string(), "restricted".to_string()],
            architectures: vec!["amd64".to_string(), "arm64".to_string()],
            description: Some("Ubuntu 20.04 LTS".to_string()),
            origin: Some("Ubuntu".to_string()),
            label: Some("Ubuntu".to_string()),
            suite: Some("focal".to_string()),
            version: Some("20.04".to_string()),
            date: Some("Thu, 23 Apr 2020 17:19:19 UTC".to_string()),
            not_automatic: Some(false),
            but_automatic_upgrades: Some(true),
            acquire_by_hash: Some(true),
            checksums_md5: None,
            checksums_sha1: None,
            checksums_sha256: None,
            checksums_sha512: None,
        };

        let deb822 = r#"Codename: focal
Components: main restricted
Architectures: amd64 arm64
Description: Ubuntu 20.04 LTS
Origin: Ubuntu
Label: Ubuntu
Suite: focal
Version: 20.04
Date: Thu, 23 Apr 2020 17:19:19 UTC
NotAutomatic: no
ButAutomaticUpgrades: yes
Acquire-By-Hash: yes
"#;

        let para = deb822.parse::<Paragraph>().unwrap();

        let release: deb822_fast::Paragraph = release.to_paragraph();

        assert_eq!(release, para);
    }

    #[test]
    fn test_package() {
        let package = r#"Package: apt
Version: 2.1.10
Architecture: amd64
Maintainer: APT Development Team <apt@lists.debian.org>
Installed-Size: 3524
Depends: libc6 (>= 2.14), libgcc1
Pre-Depends: dpkg (>= 1.15.6)
Recommends: gnupg
Suggests: apt-doc, aptitude | synaptic | wajig
"#;

        let package: Package = package.parse().unwrap();

        assert_eq!(package.name, "apt");
        assert_eq!(package.version, "2.1.10".parse().unwrap());
        assert_eq!(package.architecture, "amd64");
    }

    #[test]
    fn test_package_essential() {
        let package = r#"Package: base-files
Version: 11.1
Architecture: amd64
Essential: yes
"#;

        let package: Package = package.parse().unwrap();

        assert_eq!(package.name, "base-files");
        assert_eq!(package.essential, Some(true));
    }

    #[test]
    fn test_package_essential_no() {
        let package = r#"Package: apt
Version: 2.1.10
Architecture: amd64
Essential: no
"#;

        let package: Package = package.parse().unwrap();

        assert_eq!(package.name, "apt");
        assert_eq!(package.essential, Some(false));
    }

    #[test]
    fn test_package_with_different_source() {
        let package = r#"Package: apt
Source: not-apt (1.1.5)
Version: 2.1.10
Architecture: amd64
Maintainer: APT Development Team <apt@lists.debian.org>
Installed-Size: 3524
Depends: libc6 (>= 2.14), libgcc1
Pre-Depends: dpkg (>= 1.15.6)
Recommends: gnupg
Suggests: apt-doc, aptitude | synaptic | wajig
"#;

        let package: Package = package.parse().unwrap();

        assert_eq!(package.name, "apt");
        assert_eq!(package.version, "2.1.10".parse().unwrap());
        assert_eq!(package.architecture, "amd64");
        assert_eq!(
            package.source,
            Some(SourceRelation {
                name: "not-apt".to_string(),
                version: Some("1.1.5".parse().unwrap())
            })
        );
    }

    #[test]
    fn test_source() {
        let source = r#"Package: abinit
Binary: abinit, abinit-doc, abinit-data
Version: 9.10.4-3
Maintainer: Debichem Team <debichem-devel@lists.alioth.debian.org>
Uploaders: Andreas Tille <tille@debian.org>, Michael Banck <mbanck@debian.org>
Build-Depends: debhelper (>= 11), gfortran, liblapack-dev, python3, graphviz, markdown, ghostscript, help2man, libfftw3-dev, libhdf5-dev, libnetcdff-dev, libssl-dev, libxc-dev, mpi-default-dev, python3-dev, python3-numpy, python3-pandas, python3-yaml, texlive-latex-extra, texlive-fonts-recommended, texlive-extra-utils, texlive-pstricks, texlive-publishers, texlive-luatex
Architecture: any all
Standards-Version: 3.9.8
Format: 3.0 (quilt)
Files:
 843550cbd14395c0b9408158a91a239c 2464 abinit_9.10.4-3.dsc
 a323f11fbd4a7d0f461d99c931903b5c 130747285 abinit_9.10.4.orig.tar.gz
 27c12d3dac5cd105cebaa2af4247e807 15068 abinit_9.10.4-3.debian.tar.xz
Vcs-Browser: https://salsa.debian.org/debichem-team/abinit
Vcs-Git: https://salsa.debian.org/debichem-team/abinit.git
Checksums-Sha256:
 c3c217b14bc5705a1d8930a2e7fcef58e64beaa22abc213e2eacc7d5537ef840 2464 abinit_9.10.4-3.dsc
 6bf3c276c333956f722761f189f2b4324e150c8a50470ecb72ee07cc1c457d48 130747285 abinit_9.10.4.orig.tar.gz
 80c4fb7575d67f3167d7c34fd59477baf839810d0b863e19f1dd9fea1bc0b3b5 15068 abinit_9.10.4-3.debian.tar.xz
Homepage: http://www.abinit.org/
Package-List: 
 abinit deb science optional arch=any
 abinit-data deb science optional arch=all
 abinit-doc deb doc optional arch=all
Testsuite: autopkgtest
Testsuite-Triggers: python3, python3-numpy, python3-pandas, python3-yaml
Directory: pool/main/a/abinit
Priority: optional
Section: science
"#;

        let source: Source = source.parse().unwrap();

        assert_eq!(source.package, "abinit");
        assert_eq!(source.version, "9.10.4-3".parse().unwrap());
        assert_eq!(
            source.binaries,
            Some(vec![
                "abinit".to_string(),
                "abinit-doc".to_string(),
                "abinit-data".to_string()
            ])
        );

        let build_depends = source.build_depends.as_ref();
        let build_depends: Vec<_> = build_depends.iter().collect();
        let build_depends = build_depends[0];

        let expected_build_depends = &[
            "debhelper",
            "gfortran",
            "liblapack-dev",
            "python3",
            "graphviz",
            "markdown",
            "ghostscript",
            "help2man",
            "libfftw3-dev",
            "libhdf5-dev",
            "libnetcdff-dev",
            "libssl-dev",
            "libxc-dev",
            "mpi-default-dev",
            "python3-dev",
            "python3-numpy",
            "python3-pandas",
            "python3-yaml",
            "texlive-latex-extra",
            "texlive-fonts-recommended",
            "texlive-extra-utils",
            "texlive-pstricks",
            "texlive-publishers",
            "texlive-luatex",
        ];

        assert_eq!(build_depends.len(), expected_build_depends.len());
        assert_eq!(build_depends[0][0].name, expected_build_depends[0]);
        assert_eq!(
            build_depends[build_depends.len() - 1][0].name,
            expected_build_depends[build_depends.len() - 1]
        );

        assert_eq!(
            source.testsuite_triggers,
            Some(
                ["python3", "python3-numpy", "python3-pandas", "python3-yaml"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            )
        );

        // Test Uploaders field
        assert_eq!(
            source.uploaders,
            Some("Andreas Tille <tille@debian.org>, Michael Banck <mbanck@debian.org>".to_string())
        );

        // Test Files checksum list
        let files = source.files.as_ref().unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].md5sum, "843550cbd14395c0b9408158a91a239c");
        assert_eq!(files[0].size, 2464);
        assert_eq!(files[0].filename, "abinit_9.10.4-3.dsc");

        // Test Checksums-Sha256 list
        let checksums_sha256 = source.checksums_sha256.as_ref().unwrap();
        assert_eq!(checksums_sha256.len(), 3);
        assert_eq!(
            checksums_sha256[0].sha256,
            "c3c217b14bc5705a1d8930a2e7fcef58e64beaa22abc213e2eacc7d5537ef840"
        );
        assert_eq!(checksums_sha256[0].size, 2464);
        assert_eq!(checksums_sha256[0].filename, "abinit_9.10.4-3.dsc");

        // Test Package-List
        let package_list = source.package_list.as_ref().unwrap();
        assert_eq!(package_list.len(), 3);
        assert!(package_list[0].starts_with("abinit "));
    }

    #[test]
    fn test_source_with_extra_source_only() {
        let source = r#"Package: test-pkg
Version: 1.0-1
Maintainer: Test Maintainer <test@example.com>
Extra-Source-Only: yes
Directory: pool/main/t/test-pkg
"#;

        let source: Source = source.parse().unwrap();
        assert_eq!(source.package, "test-pkg");
        assert_eq!(source.extra_source_only, Some(true));
    }

    #[test]
    fn test_source_with_checksums_sha1() {
        let source = r#"Package: test-pkg
Version: 1.0-1
Maintainer: Test Maintainer <test@example.com>
Checksums-Sha1:
 da39a3ee5e6b4b0d3255bfef95601890afd80709 0 test_1.0-1.dsc
Directory: pool/main/t/test-pkg
"#;

        let source: Source = source.parse().unwrap();
        let checksums = source.checksums_sha1.as_ref().unwrap();
        assert_eq!(checksums.len(), 1);
        assert_eq!(
            checksums[0].sha1,
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(checksums[0].size, 0);
        assert_eq!(checksums[0].filename, "test_1.0-1.dsc");
    }

    #[test]
    fn test_source_with_checksums_sha512() {
        let source = r#"Package: test-pkg
Version: 1.0-1
Maintainer: Test Maintainer <test@example.com>
Checksums-Sha512:
 cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e 0 test_1.0-1.dsc
Directory: pool/main/t/test-pkg
"#;

        let source: Source = source.parse().unwrap();
        let checksums = source.checksums_sha512.as_ref().unwrap();
        assert_eq!(checksums.len(), 1);
        assert_eq!(checksums[0].sha512, "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e");
        assert_eq!(checksums[0].size, 0);
        assert_eq!(checksums[0].filename, "test_1.0-1.dsc");
    }

    #[test]
    fn test_package_with_multi_arch() {
        let package = r#"Package: test-pkg
Version: 1.0-1
Architecture: amd64
Multi-Arch: same
"#;

        let package: Package = package.parse().unwrap();
        assert_eq!(package.name, "test-pkg");
        assert_eq!(package.multi_arch, Some(crate::fields::MultiArch::Same));
    }

    #[test]
    fn test_package_with_all_checksums() {
        let package = r#"Package: test-pkg
Version: 1.0-1
Architecture: amd64
MD5sum: d41d8cd98f00b204e9800998ecf8427e
SHA1: da39a3ee5e6b4b0d3255bfef95601890afd80709
SHA256: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
SHA512: cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e
"#;

        let package: Package = package.parse().unwrap();
        assert_eq!(
            package.md5sum,
            Some("d41d8cd98f00b204e9800998ecf8427e".to_string())
        );
        assert_eq!(
            package.sha1,
            Some("da39a3ee5e6b4b0d3255bfef95601890afd80709".to_string())
        );
        assert_eq!(
            package.sha256,
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string())
        );
        assert_eq!(package.sha512, Some("cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e".to_string()));
    }

    #[test]
    fn test_package_with_filename() {
        let package = r#"Package: test-pkg
Version: 1.0-1
Architecture: amd64
Filename: pool/main/t/test-pkg/test-pkg_1.0-1_amd64.deb
"#;

        let package: Package = package.parse().unwrap();
        assert_eq!(
            package.filename,
            Some("pool/main/t/test-pkg/test-pkg_1.0-1_amd64.deb".to_string())
        );
    }

    #[test]
    fn test_package_with_task() {
        let package = r#"Package: ubuntu-desktop
Version: 1.0
Architecture: amd64
Task: ubuntu-desktop
"#;

        let package: Package = package.parse().unwrap();
        assert_eq!(package.task, Some("ubuntu-desktop".to_string()));
    }

    #[test]
    fn test_package_with_origin() {
        let package = r#"Package: test-pkg
Version: 1.0-1
Architecture: amd64
Origin: Debian
"#;

        let package: Package = package.parse().unwrap();
        assert_eq!(package.origin, Some("Debian".to_string()));
    }

    #[test]
    fn test_package_full_with_all_new_fields() {
        let package = r#"Package: test-pkg
Version: 1.0-1
Architecture: amd64
Multi-Arch: foreign
Origin: Ubuntu
Task: minimal
Filename: pool/main/t/test-pkg/test-pkg_1.0-1_amd64.deb
Size: 1234
MD5sum: d41d8cd98f00b204e9800998ecf8427e
SHA1: da39a3ee5e6b4b0d3255bfef95601890afd80709
SHA256: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
SHA512: cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e
"#;

        let package: Package = package.parse().unwrap();
        assert_eq!(package.name, "test-pkg");
        assert_eq!(package.version, "1.0-1".parse().unwrap());
        assert_eq!(package.architecture, "amd64");
        assert_eq!(package.multi_arch, Some(crate::fields::MultiArch::Foreign));
        assert_eq!(package.origin, Some("Ubuntu".to_string()));
        assert_eq!(package.task, Some("minimal".to_string()));
        assert_eq!(
            package.filename,
            Some("pool/main/t/test-pkg/test-pkg_1.0-1_amd64.deb".to_string())
        );
        assert_eq!(package.size, Some(1234));
        assert!(package.md5sum.is_some());
        assert!(package.sha1.is_some());
        assert!(package.sha256.is_some());
        assert!(package.sha512.is_some());
    }

    #[test]
    fn test_source_with_dgit() {
        let source = r#"Package: test-pkg
Version: 1.0-1
Maintainer: Test Maintainer <test@example.com>
Dgit: c1370424e2404d3c22bd09c828d4b28d81d897ad debian archive/debian/1.1.0 https://git.dgit.debian.org/test-pkg
Directory: pool/main/t/test-pkg
Package-List: 
 abinit deb science optional arch=any
 abinit-data deb science optional arch=all
 abinit-doc deb doc optional arch=all
"#;

        let source: Source = source.parse().unwrap();
        assert_eq!(source.package, "test-pkg");
        let dgit = source.dgit.as_ref().unwrap();
        assert_eq!(dgit.commit, "c1370424e2404d3c22bd09c828d4b28d81d897ad");
        assert_eq!(dgit.suite, "debian");
        assert_eq!(dgit.git_ref, "archive/debian/1.1.0");
        assert_eq!(dgit.url, "https://git.dgit.debian.org/test-pkg");
    }

    #[test]
    fn test_source_checksums() {
        let source = r#"Package: hello
Version: 1.0-1
Directory: pool/main/h/hello
Package-List: 
 hello deb misc optional arch=any
Files:
 acbd18db4cc2f85cedef654fccc4a4d8 1234 hello_1.0.orig.tar.gz
Checksums-Sha256:
 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824 1234 hello_1.0.orig.tar.gz
"#;

        let source: Source = source.parse().unwrap();
        assert_eq!(source.package, "hello");
        let files = source.files.as_ref().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].md5sum, "acbd18db4cc2f85cedef654fccc4a4d8");
        assert_eq!(files[0].size, 1234);
        assert_eq!(files[0].filename, "hello_1.0.orig.tar.gz");
        let sha256 = source.checksums_sha256.as_ref().unwrap();
        assert_eq!(sha256.len(), 1);
        assert_eq!(
            sha256[0].sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_release_checksums() {
        let release_str = r#"Suite: stable
Codename: stable
Architectures: amd64
Components: main
MD5Sum:
 acbd18db4cc2f85cedef654fccc4a4d8 1234 main/binary-amd64/Packages
SHA256:
 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824 1234 main/binary-amd64/Packages
"#;

        let release: Release = release_str.parse().unwrap();
        assert_eq!(release.suite, Some("stable".to_string()));
        let md5 = release.checksums_md5.as_ref().unwrap();
        assert_eq!(md5.len(), 1);
        assert_eq!(md5[0].filename, "main/binary-amd64/Packages");
        assert_eq!(md5[0].size, 1234);
        let sha256 = release.checksums_sha256.as_ref().unwrap();
        assert_eq!(sha256.len(), 1);
        assert_eq!(sha256[0].filename, "main/binary-amd64/Packages");
    }
}
