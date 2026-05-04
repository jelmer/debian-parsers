//! APT package manager files
use crate::fields::{
    Md5Checksum, MultiArch, Priority, Sha1Checksum, Sha256Checksum, Sha512Checksum,
};
use crate::lossless::relations::Relations;
use rowan::ast::AstNode;

/// A source package in the APT package manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source(deb822_lossless::Paragraph);

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for Source {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        use pyo3::prelude::*;
        let d = self.0.into_pyobject(py)?;

        let m = py.import("debian.deb822")?;
        let cls = m.getattr("Sources")?;

        cls.call1((d,))
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for &Source {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        use pyo3::prelude::*;
        let d = (&self.0).into_pyobject(py)?;

        let m = py.import("debian.deb822")?;
        let cls = m.getattr("Sources")?;

        cls.call1((d,))
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::FromPyObject<'_, 'py> for Source {
    type Error = pyo3::PyErr;

    fn extract(ob: pyo3::Borrowed<'_, 'py, pyo3::PyAny>) -> Result<Self, Self::Error> {
        Ok(Source(ob.extract()?))
    }
}

impl From<deb822_lossless::Paragraph> for Source {
    fn from(paragraph: deb822_lossless::Paragraph) -> Self {
        Self(paragraph)
    }
}

impl Default for Source {
    fn default() -> Self {
        Self(deb822_lossless::Paragraph::new())
    }
}

impl Source {
    /// Create a new source package
    pub fn new() -> Self {
        Self(deb822_lossless::Paragraph::new())
    }

    /// Parse source package text, returning a Parse result
    ///
    /// Note: This expects a single paragraph, not a full deb822 document
    pub fn parse(text: &str) -> deb822_lossless::Parse<Source> {
        // We need to transform Parse<Deb822> to Parse<Source>
        // Since Source wraps a single Paragraph, we need custom logic
        let deb822_parse = deb822_lossless::Deb822::parse(text);

        // For now, we'll use the same green node but the cast will extract the first paragraph
        let green = deb822_parse.green().clone();
        let mut errors = deb822_parse.errors().to_vec();

        // Check if there's exactly one paragraph
        if errors.is_empty() {
            let deb822 = deb822_parse.tree();
            let paragraph_count = deb822.paragraphs().count();
            if paragraph_count == 0 {
                errors.push("No paragraphs found".to_string());
            } else if paragraph_count > 1 {
                errors.push("Multiple paragraphs found, expected one".to_string());
            }
        }

        deb822_lossless::Parse::new(green, errors)
    }

    /// Get the source name
    pub fn package(&self) -> Option<String> {
        self.0.get("Package").map(|s| s.to_string())
    }

    /// Set the package name
    pub fn set_package(&mut self, package: &str) {
        self.0.set("Package", package);
    }

    /// Get the version of the package
    pub fn version(&self) -> Option<debversion::Version> {
        self.0.get("Version").map(|s| s.parse().unwrap())
    }

    /// Set the version of the package
    pub fn set_version(&mut self, version: debversion::Version) {
        self.0.set("Version", &version.to_string());
    }

    /// Get the maintainer of the package
    pub fn maintainer(&self) -> Option<String> {
        self.0.get("Maintainer").map(|s| s.to_string())
    }

    /// Set the maintainer of the package
    pub fn set_maintainer(&mut self, maintainer: &str) {
        self.0.set("Maintainer", maintainer);
    }

    /// Get the uploaders of the package
    pub fn uploaders(&self) -> Option<Vec<String>> {
        self.0.get("Uploaders").map(|s| {
            s.split(',')
                .map(|s| s.trim().to_string())
                .collect::<Vec<String>>()
        })
    }

    /// Set the uploaders of the package
    pub fn set_uploaders(&mut self, uploaders: Vec<String>) {
        self.0.set("Uploaders", &uploaders.join(", "));
    }

    /// Get the standards version of the package
    pub fn standards_version(&self) -> Option<String> {
        self.0.get("Standards-Version").map(|s| s.to_string())
    }

    /// Set the standards version of the package
    pub fn set_standards_version(&mut self, version: &str) {
        self.0.set("Standards-Version", version);
    }

    /// Get the source format of the package
    pub fn format(&self) -> Option<String> {
        self.0.get("Format").map(|s| s.to_string())
    }

    /// Set the format of the package
    pub fn set_format(&mut self, format: &str) {
        self.0.set("Format", format);
    }

    /// Get the Vcs-Browser field
    pub fn vcs_browser(&self) -> Option<String> {
        self.0.get("Vcs-Browser").map(|s| s.to_string())
    }

    /// Set the Vcs-Browser field
    pub fn set_vcs_browser(&mut self, url: &str) {
        self.0.set("Vcs-Browser", url);
    }

    /// Get the Vcs-Git field
    pub fn vcs_git(&self) -> Option<String> {
        self.0.get("Vcs-Git").map(|s| s.to_string())
    }

    /// Set the Vcs-Git field
    pub fn set_vcs_git(&mut self, url: &str) {
        self.0.set("Vcs-Git", url);
    }

    /// Get the Vcs-Svn field
    pub fn vcs_svn(&self) -> Option<String> {
        self.0.get("Vcs-Svn").map(|s| s.to_string())
    }

    /// Set the Vcs-Svn field
    pub fn set_vcs_svn(&mut self, url: &str) {
        self.0.set("Vcs-Svn", url);
    }

    /// Get the Vcs-Hg field
    pub fn vcs_hg(&self) -> Option<String> {
        self.0.get("Vcs-Hg").map(|s| s.to_string())
    }

    /// Set the Vcs-Hg field
    pub fn set_vcs_hg(&mut self, url: &str) {
        self.0.set("Vcs-Hg", url);
    }

    /// Get the Vcs-Bzr field
    pub fn vcs_bzr(&self) -> Option<String> {
        self.0.get("Vcs-Bzr").map(|s| s.to_string())
    }

    /// Set the Vcs-Bzr field
    pub fn set_vcs_bzr(&mut self, url: &str) {
        self.0.set("Vcs-Bzr", url);
    }

    /// Get the Vcs-Arch field
    pub fn vcs_arch(&self) -> Option<String> {
        self.0.get("Vcs-Arch").map(|s| s.to_string())
    }

    /// Set the Vcs-Arch field
    pub fn set_vcs_arch(&mut self, url: &str) {
        self.0.set("Vcs-Arch", url);
    }

    /// Get the Vcs-Svk field
    pub fn vcs_svk(&self) -> Option<String> {
        self.0.get("Vcs-Svk").map(|s| s.to_string())
    }

    /// Set the Svk VCS
    pub fn set_vcs_svk(&mut self, url: &str) {
        self.0.set("Vcs-Svk", url);
    }

    /// Get the Darcs VCS
    pub fn vcs_darcs(&self) -> Option<String> {
        self.0.get("Vcs-Darcs").map(|s| s.to_string())
    }

    /// Set the Darcs VCS
    pub fn set_vcs_darcs(&mut self, url: &str) {
        self.0.set("Vcs-Darcs", url);
    }

    /// Get the Mtn VCS
    pub fn vcs_mtn(&self) -> Option<String> {
        self.0.get("Vcs-Mtn").map(|s| s.to_string())
    }

    /// Set the Mtn VCS
    pub fn set_vcs_mtn(&mut self, url: &str) {
        self.0.set("Vcs-Mtn", url);
    }

    /// Get the Cvs VCS
    pub fn vcs_cvs(&self) -> Option<String> {
        self.0.get("Vcs-Cvs").map(|s| s.to_string())
    }

    /// Set the Cvs VCS
    pub fn set_vcs_cvs(&mut self, url: &str) {
        self.0.set("Vcs-Cvs", url);
    }

    /// Get the build depends
    pub fn build_depends(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Build-Depends")
            .map(|s| s.parse().unwrap())
    }

    /// Set the build depends
    pub fn set_build_depends(&mut self, relations: Relations) {
        self.0.set("Build-Depends", relations.to_string().as_str());
    }

    /// Get the arch-independent build depends
    pub fn build_depends_indep(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Build-Depends-Indep")
            .map(|s| s.parse().unwrap())
    }

    /// Set the arch-independent build depends
    pub fn set_build_depends_indep(&mut self, relations: Relations) {
        self.0.set("Build-Depends-Indep", &relations.to_string());
    }

    /// Get the arch-dependent build depends
    pub fn build_depends_arch(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Build-Depends-Arch")
            .map(|s| s.parse().unwrap())
    }

    /// Set the arch-dependent build depends
    pub fn set_build_depends_arch(&mut self, relations: Relations) {
        self.0.set("Build-Depends-Arch", &relations.to_string());
    }

    /// Get the build conflicts
    pub fn build_conflicts(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Build-Conflicts")
            .map(|s| s.parse().unwrap())
    }

    /// Set the build conflicts
    pub fn set_build_conflicts(&mut self, relations: Relations) {
        self.0.set("Build-Conflicts", &relations.to_string());
    }

    /// Get the build conflicts indep
    pub fn build_conflicts_indep(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Build-Conflicts-Indep")
            .map(|s| s.parse().unwrap())
    }

    /// Set the build conflicts indep
    pub fn set_build_conflicts_indep(&mut self, relations: Relations) {
        self.0.set("Build-Conflicts-Indep", &relations.to_string());
    }

    /// Get the build conflicts arch
    pub fn build_conflicts_arch(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Build-Conflicts-Arch")
            .map(|s| s.parse().unwrap())
    }

    /// Set the build conflicts arch
    pub fn set_build_conflicts_arch(&mut self, relations: Relations) {
        self.0.set("Build-Conflicts-Arch", &relations.to_string());
    }

    /// Get the binary relations
    pub fn binary(&self) -> Option<Relations> {
        self.0.get("Binary").map(|s| s.parse().unwrap())
    }

    /// Set the binary relations
    pub fn set_binary(&mut self, relations: Relations) {
        self.0.set("Binary", &relations.to_string());
    }

    /// Get the homepage of the package.
    pub fn homepage(&self) -> Option<String> {
        self.0.get("Homepage").map(|s| s.to_string())
    }

    /// Set the homepage of the package.
    pub fn set_homepage(&mut self, url: &str) {
        self.0.set("Homepage", url);
    }

    /// Get the section of the package.
    pub fn section(&self) -> Option<String> {
        self.0.get("Section").map(|s| s.to_string())
    }

    /// Set the section of the package.
    pub fn set_section(&mut self, section: &str) {
        self.0.set("Section", section);
    }

    /// Get the priority of the package.
    pub fn priority(&self) -> Option<Priority> {
        self.0.get("Priority").and_then(|v| v.parse().ok())
    }

    /// Set the priority of the package.
    pub fn set_priority(&mut self, priority: Priority) {
        self.0.set("Priority", priority.to_string().as_str());
    }

    /// The architecture of the package.
    pub fn architecture(&self) -> Option<String> {
        self.0.get("Architecture")
    }

    /// Set the architecture of the package.
    pub fn set_architecture(&mut self, arch: &str) {
        self.0.set("Architecture", arch);
    }

    /// Get the directory
    pub fn directory(&self) -> Option<String> {
        self.0.get("Directory").map(|s| s.to_string())
    }

    /// Set the directory
    pub fn set_directory(&mut self, dir: &str) {
        self.0.set("Directory", dir);
    }

    /// Get the test suite
    pub fn testsuite(&self) -> Option<String> {
        self.0.get("Testsuite").map(|s| s.to_string())
    }

    /// Set the testsuite
    pub fn set_testsuite(&mut self, testsuite: &str) {
        self.0.set("Testsuite", testsuite);
    }

    /// Get the files
    pub fn files(&self) -> Vec<Md5Checksum> {
        self.0
            .get("Files")
            .map(|s| {
                s.lines()
                    .map(|line| line.parse().unwrap())
                    .collect::<Vec<Md5Checksum>>()
            })
            .unwrap_or_default()
    }

    /// Set the files
    pub fn set_files(&mut self, files: Vec<Md5Checksum>) {
        self.0.set(
            "Files",
            &files
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        );
    }

    /// Get the SHA1 checksums
    pub fn checksums_sha1(&self) -> Vec<Sha1Checksum> {
        self.0
            .get("Checksums-Sha1")
            .map(|s| {
                s.lines()
                    .map(|line| line.parse().unwrap())
                    .collect::<Vec<Sha1Checksum>>()
            })
            .unwrap_or_default()
    }

    /// Set the SHA1 checksums
    pub fn set_checksums_sha1(&mut self, checksums: Vec<Sha1Checksum>) {
        self.0.set(
            "Checksums-Sha1",
            &checksums
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        );
    }

    /// Get the SHA256 checksums
    pub fn checksums_sha256(&self) -> Vec<Sha256Checksum> {
        self.0
            .get("Checksums-Sha256")
            .map(|s| {
                s.lines()
                    .map(|line| line.parse().unwrap())
                    .collect::<Vec<Sha256Checksum>>()
            })
            .unwrap_or_default()
    }

    /// Set the SHA256 checksums
    pub fn set_checksums_sha256(&mut self, checksums: Vec<Sha256Checksum>) {
        self.0.set(
            "Checksums-Sha256",
            &checksums
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        );
    }

    /// Get the SHA512 checksums
    pub fn checksums_sha512(&self) -> Vec<Sha512Checksum> {
        self.0
            .get("Checksums-Sha512")
            .map(|s| {
                s.lines()
                    .map(|line| line.parse().unwrap())
                    .collect::<Vec<Sha512Checksum>>()
            })
            .unwrap_or_default()
    }

    /// Set the SHA512 checksums
    pub fn set_checksums_sha512(&mut self, checksums: Vec<Sha512Checksum>) {
        self.0.set(
            "Checksums-Sha512",
            &checksums
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        );
    }
}

impl std::str::FromStr for Source {
    type Err = deb822_lossless::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Source::parse(s).to_result()
    }
}

impl AstNode for Source {
    type Language = deb822_lossless::Lang;

    fn can_cast(kind: <Self::Language as rowan::Language>::Kind) -> bool {
        // Source can be cast from either a Paragraph or a Deb822 (taking first paragraph)
        deb822_lossless::Paragraph::can_cast(kind) || deb822_lossless::Deb822::can_cast(kind)
    }

    fn cast(syntax: rowan::SyntaxNode<Self::Language>) -> Option<Self> {
        // Try to cast as Paragraph first
        if let Some(para) = deb822_lossless::Paragraph::cast(syntax.clone()) {
            Some(Source(para))
        } else if let Some(deb822) = deb822_lossless::Deb822::cast(syntax) {
            // If it's a Deb822, take the first paragraph
            deb822.paragraphs().next().map(Source)
        } else {
            None
        }
    }

    fn syntax(&self) -> &rowan::SyntaxNode<Self::Language> {
        self.0.syntax()
    }
}

/// A package in the APT package manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package(deb822_lossless::Paragraph);

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for Package {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        use pyo3::prelude::*;
        let d = self.0.into_pyobject(py)?;

        let m = py.import("debian.deb822")?;
        let cls = m.getattr("Packages")?;

        cls.call1((d,))
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for &Package {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        use pyo3::prelude::*;
        let d = (&self.0).into_pyobject(py)?;

        let m = py.import("debian.deb822")?;
        let cls = m.getattr("Packages")?;

        cls.call1((d,))
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::FromPyObject<'_, 'py> for Package {
    type Error = pyo3::PyErr;

    fn extract(ob: pyo3::Borrowed<'_, 'py, pyo3::PyAny>) -> Result<Self, Self::Error> {
        Ok(Package(ob.extract()?))
    }
}

impl Package {
    /// Create a new package.
    pub fn new(paragraph: deb822_lossless::Paragraph) -> Self {
        Self(paragraph)
    }

    /// Parse package text, returning a Parse result
    ///
    /// Note: This expects a single paragraph, not a full deb822 document
    pub fn parse(text: &str) -> deb822_lossless::Parse<Package> {
        let deb822_parse = deb822_lossless::Deb822::parse(text);
        let green = deb822_parse.green().clone();
        let mut errors = deb822_parse.errors().to_vec();

        // Check if there's exactly one paragraph
        if errors.is_empty() {
            let deb822 = deb822_parse.tree();
            let paragraph_count = deb822.paragraphs().count();
            if paragraph_count == 0 {
                errors.push("No paragraphs found".to_string());
            } else if paragraph_count > 1 {
                errors.push("Multiple paragraphs found, expected one".to_string());
            }
        }

        deb822_lossless::Parse::new(green, errors)
    }

    /// Get the name of the package.
    pub fn name(&self) -> Option<String> {
        self.0.get("Package").map(|s| s.to_string())
    }

    /// Set the name of the package.
    pub fn set_name(&mut self, name: &str) {
        self.0.set("Package", name);
    }

    /// Get the version of the package.
    pub fn version(&self) -> Option<debversion::Version> {
        self.0.get("Version").map(|s| s.parse().unwrap())
    }

    /// Set the version of the package.
    pub fn set_version(&mut self, version: debversion::Version) {
        self.0.set("Version", &version.to_string());
    }

    /// Get the installed size of the package in bytes.
    pub fn installed_size(&self) -> Option<usize> {
        self.0.get("Installed-Size").map(|s| s.parse().unwrap())
    }

    /// Set the installed size of the package in bytes.
    pub fn set_installed_size(&mut self, size: usize) {
        self.0.set("Installed-Size", &size.to_string());
    }

    /// Get the maintainer of the package.
    pub fn maintainer(&self) -> Option<String> {
        self.0.get("Maintainer").map(|s| s.to_string())
    }

    /// Set the maintainer of the package.
    pub fn set_maintainer(&mut self, maintainer: &str) {
        self.0.set("Maintainer", maintainer);
    }

    /// Get the architecture of the package.
    pub fn architecture(&self) -> Option<String> {
        self.0.get("Architecture").map(|s| s.to_string())
    }

    /// Set the architecture of the package.
    pub fn set_architecture(&mut self, arch: &str) {
        self.0.set("Architecture", arch);
    }

    /// Get the packages that this package depends on.
    pub fn depends(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Depends")
            .map(|s| s.parse().unwrap())
    }

    /// Set the packages that this package depends on.
    pub fn set_depends(&mut self, relations: Relations) {
        self.0.set("Depends", &relations.to_string());
    }

    /// Get the packages that this package suggests.
    pub fn recommends(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Recommends")
            .map(|s| s.parse().unwrap())
    }

    /// Set the packages that this package recommends.
    pub fn set_recommends(&mut self, relations: Relations) {
        self.0.set("Recommends", &relations.to_string());
    }

    /// Get the packages that this package suggests.
    pub fn suggests(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Suggests")
            .map(|s| s.parse().unwrap())
    }

    /// Set the packages that this package suggests.
    pub fn set_suggests(&mut self, relations: Relations) {
        self.0.set("Suggests", &relations.to_string());
    }

    /// Get the packages that this package enhances.
    pub fn enhances(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Enhances")
            .map(|s| s.parse().unwrap())
    }

    /// Set the packages that this package enhances.
    pub fn set_enhances(&mut self, relations: Relations) {
        self.0.set("Enhances", &relations.to_string());
    }

    /// Get the relations that this package pre-depends on.
    pub fn pre_depends(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Pre-Depends")
            .map(|s| s.parse().unwrap())
    }

    /// Set the relations that this package pre-depends on.
    pub fn set_pre_depends(&mut self, relations: Relations) {
        self.0.set("Pre-Depends", &relations.to_string());
    }

    /// Get the relations that this package breaks.
    pub fn breaks(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Breaks")
            .map(|s| s.parse().unwrap())
    }

    /// Set the relations that this package breaks.
    pub fn set_breaks(&mut self, relations: Relations) {
        self.0.set("Breaks", &relations.to_string());
    }

    /// Get the relations that this package conflicts with.
    pub fn conflicts(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Conflicts")
            .map(|s| s.parse().unwrap())
    }

    /// Set the relations that this package conflicts with.
    pub fn set_conflicts(&mut self, relations: Relations) {
        self.0.set("Conflicts", &relations.to_string());
    }

    /// Get the relations that this package replaces.
    pub fn replaces(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Replaces")
            .map(|s| s.parse().unwrap())
    }

    /// Set the relations that this package replaces.
    pub fn set_replaces(&mut self, relations: Relations) {
        self.0.set("Replaces", &relations.to_string());
    }

    /// Get the relations that this package provides.
    pub fn provides(&self) -> Option<Relations> {
        self.0
            .get_with_comments("Provides")
            .map(|s| s.parse().unwrap())
    }

    /// Set the relations that the package provides.
    pub fn set_provides(&mut self, relations: Relations) {
        self.0.set("Provides", &relations.to_string());
    }

    /// Get the section of the package.
    pub fn section(&self) -> Option<String> {
        self.0.get("Section").map(|s| s.to_string())
    }

    /// Set the section of the package.
    pub fn set_section(&mut self, section: &str) {
        self.0.set("Section", section);
    }

    /// Get the priority of the package.
    pub fn priority(&self) -> Option<Priority> {
        self.0.get("Priority").and_then(|v| v.parse().ok())
    }

    /// Set the priority of the package.
    pub fn set_priority(&mut self, priority: Priority) {
        self.0.set("Priority", priority.to_string().as_str());
    }

    /// Get the description of the package.
    pub fn description(&self) -> Option<String> {
        self.0.get_multiline("Description")
    }

    /// Set the description of the package.
    pub fn set_description(&mut self, description: &str) {
        self.0.set("Description", description);
    }

    /// Get the upstream homepage of the package.
    pub fn homepage(&self) -> Option<url::Url> {
        self.0.get("Homepage").map(|s| s.parse().unwrap())
    }

    /// Set the upstream homepage of the package.
    pub fn set_homepage(&mut self, url: &url::Url) {
        self.0.set("Homepage", url.as_ref());
    }

    /// Get the source of the package.
    pub fn source(&self) -> Option<String> {
        self.0.get("Source").map(|s| s.to_string())
    }

    /// Set the source of the package.
    pub fn set_source(&mut self, source: &str) {
        self.0.set("Source", source);
    }

    /// Get the MD5 checksum of the description.
    pub fn description_md5(&self) -> Option<String> {
        self.0.get("Description-md5").map(|s| s.to_string())
    }

    /// Set the MD5 checksum of the description.
    pub fn set_description_md5(&mut self, md5: &str) {
        self.0.set("Description-md5", md5);
    }

    /// Get the tags of the package.
    pub fn tags(&self, tag: &str) -> Option<Vec<String>> {
        self.0
            .get(tag)
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
    }

    /// Set the tags of the package.
    pub fn set_tags(&mut self, tag: &str, tags: Vec<String>) {
        self.0.set(tag, &tags.join(", "));
    }

    /// Get the filename of the package.
    pub fn filename(&self) -> Option<String> {
        self.0.get("Filename").map(|s| s.to_string())
    }

    /// Set the filename of the package.
    pub fn set_filename(&mut self, filename: &str) {
        self.0.set("Filename", filename);
    }

    /// Get the size of the package.
    pub fn size(&self) -> Option<usize> {
        self.0.get("Size").map(|s| s.parse().unwrap())
    }

    /// Set the size of the package.
    pub fn set_size(&mut self, size: usize) {
        self.0.set("Size", &size.to_string());
    }

    /// Get the MD5 checksum.
    pub fn md5sum(&self) -> Option<String> {
        self.0.get("MD5sum").map(|s| s.to_string())
    }

    /// Set the MD5 checksum.
    pub fn set_md5sum(&mut self, md5sum: &str) {
        self.0.set("MD5sum", md5sum);
    }

    /// Get the SHA1 checksum.
    pub fn sha1(&self) -> Option<String> {
        self.0.get("SHA1").map(|s| s.to_string())
    }

    /// Set the SHA1 checksum.
    pub fn set_sha1(&mut self, sha1: &str) {
        self.0.set("SHA1", sha1);
    }

    /// Get the SHA256 checksum.
    pub fn sha256(&self) -> Option<String> {
        self.0.get("SHA256").map(|s| s.to_string())
    }

    /// Set the SHA256 checksum.
    pub fn set_sha256(&mut self, sha256: &str) {
        self.0.set("SHA256", sha256);
    }

    /// Get the SHA512 checksum.
    pub fn sha512(&self) -> Option<String> {
        self.0.get("SHA512").map(|s| s.to_string())
    }

    /// Set the SHA512 checksum.
    pub fn set_sha512(&mut self, sha512: &str) {
        self.0.set("SHA512", sha512);
    }

    /// Get the multi-arch field.
    pub fn multi_arch(&self) -> Option<MultiArch> {
        self.0.get("Multi-Arch").map(|s| s.parse().unwrap())
    }

    /// Set the multi-arch field.
    pub fn set_multi_arch(&mut self, arch: MultiArch) {
        self.0.set("Multi-Arch", arch.to_string().as_str());
    }
}

impl Default for Package {
    fn default() -> Self {
        Self(deb822_lossless::Paragraph::new())
    }
}

impl std::fmt::Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for Package {
    type Err = deb822_lossless::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Package::parse(s).to_result()
    }
}

impl AstNode for Package {
    type Language = deb822_lossless::Lang;

    fn can_cast(kind: <Self::Language as rowan::Language>::Kind) -> bool {
        deb822_lossless::Paragraph::can_cast(kind) || deb822_lossless::Deb822::can_cast(kind)
    }

    fn cast(syntax: rowan::SyntaxNode<Self::Language>) -> Option<Self> {
        if let Some(para) = deb822_lossless::Paragraph::cast(syntax.clone()) {
            Some(Package(para))
        } else if let Some(deb822) = deb822_lossless::Deb822::cast(syntax) {
            deb822.paragraphs().next().map(Package)
        } else {
            None
        }
    }

    fn syntax(&self) -> &rowan::SyntaxNode<Self::Language> {
        self.0.syntax()
    }
}

/// A release in the APT package manager.
#[derive(Clone)]
pub struct Release(deb822_lossless::Paragraph);

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for Release {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        use pyo3::prelude::*;
        let d = self.0.into_pyobject(py)?;

        let m = py.import("debian.deb822")?;
        let cls = m.getattr("Release")?;

        cls.call1((d,))
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::IntoPyObject<'py> for &Release {
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, Self::Target>;
    type Error = pyo3::PyErr;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        use pyo3::prelude::*;
        let d = (&self.0).into_pyobject(py)?;

        let m = py.import("debian.deb822")?;
        let cls = m.getattr("Release")?;

        cls.call1((d,))
    }
}

#[cfg(feature = "python-debian")]
impl<'py> pyo3::FromPyObject<'_, 'py> for Release {
    type Error = pyo3::PyErr;

    fn extract(ob: pyo3::Borrowed<'_, 'py, pyo3::PyAny>) -> Result<Self, Self::Error> {
        Ok(Release(ob.extract()?))
    }
}

impl Release {
    /// Create a new release
    pub fn new(paragraph: deb822_lossless::Paragraph) -> Self {
        Self(paragraph)
    }

    /// Parse release text, returning a Parse result
    ///
    /// Note: This expects a single paragraph, not a full deb822 document
    pub fn parse(text: &str) -> deb822_lossless::Parse<Release> {
        let deb822_parse = deb822_lossless::Deb822::parse(text);
        let green = deb822_parse.green().clone();
        let mut errors = deb822_parse.errors().to_vec();

        // Check if there's exactly one paragraph
        if errors.is_empty() {
            let deb822 = deb822_parse.tree();
            let paragraph_count = deb822.paragraphs().count();
            if paragraph_count == 0 {
                errors.push("No paragraphs found".to_string());
            } else if paragraph_count > 1 {
                errors.push("Multiple paragraphs found, expected one".to_string());
            }
        }

        deb822_lossless::Parse::new(green, errors)
    }

    /// Get the origin of the release
    pub fn origin(&self) -> Option<String> {
        self.0.get("Origin").map(|s| s.to_string())
    }

    /// Set the origin of the release
    pub fn set_origin(&mut self, origin: &str) {
        self.0.set("Origin", origin);
    }

    /// Get the label of the release
    pub fn label(&self) -> Option<String> {
        self.0.get("Label").map(|s| s.to_string())
    }

    /// Set the label of the release
    pub fn set_label(&mut self, label: &str) {
        self.0.set("Label", label);
    }

    /// Get the suite of the release
    pub fn suite(&self) -> Option<String> {
        self.0.get("Suite").map(|s| s.to_string())
    }

    /// Set the suite of the release
    pub fn set_suite(&mut self, suite: &str) {
        self.0.set("Suite", suite);
    }

    /// Get the codename of the release
    pub fn codename(&self) -> Option<String> {
        self.0.get("Codename").map(|s| s.to_string())
    }

    /// Set the codename of the release
    pub fn set_codename(&mut self, codename: &str) {
        self.0.set("Codename", codename);
    }

    /// Get the URLs at which the changelogs can be found
    pub fn changelogs(&self) -> Option<Vec<String>> {
        self.0.get("Changelogs").map(|s| {
            s.split(',')
                .map(|s| s.trim().to_string())
                .collect::<Vec<String>>()
        })
    }

    /// Set the URLs at which the changelogs can be found
    pub fn set_changelogs(&mut self, changelogs: Vec<String>) {
        self.0.set("Changelogs", &changelogs.join(", "));
    }

    #[cfg(feature = "chrono")]
    /// Get the date of the release
    pub fn date(&self) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        self.0
            .get("Date")
            .as_ref()
            .map(|s| chrono::DateTime::parse_from_rfc2822(s).unwrap())
    }

    #[cfg(feature = "chrono")]
    /// Set the date of the release
    pub fn set_date(&mut self, date: chrono::DateTime<chrono::FixedOffset>) {
        self.0.set("Date", date.to_rfc2822().as_str());
    }

    #[cfg(feature = "chrono")]
    /// Get the date until the release is valid
    pub fn valid_until(&self) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        self.0
            .get("Valid-Until")
            .as_ref()
            .map(|s| chrono::DateTime::parse_from_rfc2822(s).unwrap())
    }

    #[cfg(feature = "chrono")]
    /// Set the date until the release is valid
    pub fn set_valid_until(&mut self, date: chrono::DateTime<chrono::FixedOffset>) {
        self.0.set("Valid-Until", date.to_rfc2822().as_str());
    }

    /// Get whether acquire by hash is enabled
    pub fn acquire_by_hash(&self) -> bool {
        self.0
            .get("Acquire-By-Hash")
            .map(|s| s == "yes")
            .unwrap_or(false)
    }

    /// Set whether acquire by hash is enabled
    pub fn set_acquire_by_hash(&mut self, acquire_by_hash: bool) {
        self.0.set(
            "Acquire-By-Hash",
            if acquire_by_hash { "yes" } else { "no" },
        );
    }

    /// Get whether the release has no support for architecture all
    pub fn no_support_for_architecture_all(&self) -> bool {
        self.0
            .get("No-Support-For-Architecture-All")
            .map(|s| s == "yes")
            .unwrap_or(false)
    }

    /// Set whether the release has no support for architecture all
    pub fn set_no_support_for_architecture_all(&mut self, no_support_for_architecture_all: bool) {
        self.0.set(
            "No-Support-For-Architecture-All",
            if no_support_for_architecture_all {
                "yes"
            } else {
                "no"
            },
        );
    }

    /// Get the architectures
    pub fn architectures(&self) -> Option<Vec<String>> {
        self.0.get("Architectures").map(|s| {
            s.split_whitespace()
                .map(|s| s.trim().to_string())
                .collect::<Vec<String>>()
        })
    }

    /// Set the architectures
    pub fn set_architectures(&mut self, architectures: Vec<String>) {
        self.0.set("Architectures", &architectures.join(" "));
    }

    /// Get the components
    pub fn components(&self) -> Option<Vec<String>> {
        self.0.get("Components").map(|s| {
            s.split_whitespace()
                .map(|s| s.trim().to_string())
                .collect::<Vec<String>>()
        })
    }

    /// Set the components
    pub fn set_components(&mut self, components: Vec<String>) {
        self.0.set("Components", &components.join(" "));
    }

    /// Get the description
    pub fn description(&self) -> Option<String> {
        self.0.get_multiline("Description")
    }

    /// Set the description
    pub fn set_description(&mut self, description: &str) {
        self.0.set("Description", description);
    }

    /// Get the MD5 checksums
    pub fn checksums_md5(&self) -> Vec<Md5Checksum> {
        self.0
            .get("MD5Sum")
            .map(|s| {
                s.lines()
                    .map(|line| line.parse().unwrap())
                    .collect::<Vec<Md5Checksum>>()
            })
            .unwrap_or_default()
    }

    /// Set the MD5 checksums
    pub fn set_checksums_md5(&mut self, files: Vec<Md5Checksum>) {
        self.0.set(
            "MD5Sum",
            &files
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        );
    }

    /// Get the SHA1 checksums
    pub fn checksums_sha1(&self) -> Vec<Sha1Checksum> {
        self.0
            .get("SHA1")
            .map(|s| {
                s.lines()
                    .map(|line| line.parse().unwrap())
                    .collect::<Vec<Sha1Checksum>>()
            })
            .unwrap_or_default()
    }

    /// Set the SHA1 checksums
    pub fn set_checksums_sha1(&mut self, checksums: Vec<Sha1Checksum>) {
        self.0.set(
            "SHA1",
            &checksums
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        );
    }

    /// Get the SHA256 checksums
    pub fn checksums_sha256(&self) -> Vec<Sha256Checksum> {
        self.0
            .get("SHA256")
            .map(|s| {
                s.lines()
                    .map(|line| line.parse().unwrap())
                    .collect::<Vec<Sha256Checksum>>()
            })
            .unwrap_or_default()
    }

    /// Set the SHA256 checksums
    pub fn set_checksums_sha256(&mut self, checksums: Vec<Sha256Checksum>) {
        self.0.set(
            "SHA256",
            &checksums
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        );
    }

    /// Get the SHA512 checksums
    pub fn checksums_sha512(&self) -> Vec<Sha512Checksum> {
        self.0
            .get("SHA512")
            .map(|s| {
                s.lines()
                    .map(|line| line.parse().unwrap())
                    .collect::<Vec<Sha512Checksum>>()
            })
            .unwrap_or_default()
    }

    /// Set the SHA512 checksums
    pub fn set_checksums_sha512(&mut self, checksums: Vec<Sha512Checksum>) {
        self.0.set(
            "SHA512",
            &checksums
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        );
    }

    /// Get whether the release is not automatic.
    pub fn not_automatic(&self) -> Option<bool> {
        self.0
            .get("NotAutomatic")
            .map(|s| s.to_lowercase() == "yes")
    }

    /// Set whether the release is not automatic.
    pub fn set_not_automatic(&mut self, v: bool) {
        self.0.set("NotAutomatic", if v { "yes" } else { "no" });
    }

    /// Get whether automatic upgrades are allowed despite not automatic.
    pub fn but_automatic_upgrades(&self) -> Option<bool> {
        self.0
            .get("ButAutomaticUpgrades")
            .map(|s| s.to_lowercase() == "yes")
    }

    /// Set whether automatic upgrades are allowed despite not automatic.
    pub fn set_but_automatic_upgrades(&mut self, v: bool) {
        self.0
            .set("ButAutomaticUpgrades", if v { "yes" } else { "no" });
    }

    /// Get the version of the release.
    pub fn version(&self) -> Option<String> {
        self.0.get("Version").map(|s| s.to_string())
    }

    /// Set the version of the release.
    pub fn set_version(&mut self, version: &str) {
        self.0.set("Version", version);
    }
}

impl Default for Release {
    fn default() -> Self {
        Self(deb822_lossless::Paragraph::new())
    }
}

impl std::fmt::Display for Release {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for Release {
    type Err = deb822_lossless::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Release::parse(s).to_result()
    }
}

impl AstNode for Release {
    type Language = deb822_lossless::Lang;

    fn can_cast(kind: <Self::Language as rowan::Language>::Kind) -> bool {
        deb822_lossless::Paragraph::can_cast(kind) || deb822_lossless::Deb822::can_cast(kind)
    }

    fn cast(syntax: rowan::SyntaxNode<Self::Language>) -> Option<Self> {
        if let Some(para) = deb822_lossless::Paragraph::cast(syntax.clone()) {
            Some(Release(para))
        } else if let Some(deb822) = deb822_lossless::Deb822::cast(syntax) {
            deb822.paragraphs().next().map(Release)
        } else {
            None
        }
    }

    fn syntax(&self) -> &rowan::SyntaxNode<Self::Language> {
        self.0.syntax()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fields::PackageListEntry;

    #[test]
    fn test_parse_package_list() {
        let s = "package1 binary section standard extra1=foo extra2=bar";
        let p: PackageListEntry = s.parse().unwrap();
        assert_eq!(p.package, "package1");
        assert_eq!(p.package_type, "binary");
        assert_eq!(p.section, "section");
        assert_eq!(p.priority, super::Priority::Standard);
        assert_eq!(p.extra.get("extra1"), Some(&"foo".to_string()));
        assert_eq!(p.extra.get("extra2"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_parse_package_list_no_extra() {
        let s = "package1 binary section standard";
        let p: PackageListEntry = s.parse().unwrap();
        assert_eq!(p.package, "package1");
        assert_eq!(p.package_type, "binary");
        assert_eq!(p.section, "section");
        assert_eq!(p.priority, super::Priority::Standard);
        assert!(p.extra.is_empty());
    }

    #[test]
    fn test_files() {
        let s = "md5sum 1234 filename";
        let f: super::Md5Checksum = s.parse().unwrap();
        assert_eq!(f.md5sum, "md5sum");
        assert_eq!(f.size, 1234);
        assert_eq!(f.filename, "filename");
    }

    #[test]
    fn test_sha1_checksum() {
        let s = "sha1 1234 filename";
        let f: super::Sha1Checksum = s.parse().unwrap();
        assert_eq!(f.sha1, "sha1");
        assert_eq!(f.size, 1234);
        assert_eq!(f.filename, "filename");
    }

    #[test]
    fn test_sha256_checksum() {
        let s = "sha256 1234 filename";
        let f: super::Sha256Checksum = s.parse().unwrap();
        assert_eq!(f.sha256, "sha256");
        assert_eq!(f.size, 1234);
        assert_eq!(f.filename, "filename");
    }

    #[test]
    fn test_sha512_checksum() {
        let s = "sha512 1234 filename";
        let f: super::Sha512Checksum = s.parse().unwrap();
        assert_eq!(f.sha512, "sha512");
        assert_eq!(f.size, 1234);
        assert_eq!(f.filename, "filename");
    }

    #[test]
    fn test_source() {
        let s = r#"Package: foo
Version: 1.0
Maintainer: John Doe <john@example.com>
Uploaders: Jane Doe <jane@example.com>
Standards-Version: 3.9.8
Format: 3.0 (quilt)
Vcs-Browser: https://example.com/foo
Vcs-Git: https://example.com/foo.git
Build-Depends: debhelper (>= 9)
Build-Depends-Indep: python
Build-Depends-Arch: gcc
Build-Conflicts: bar
Build-Conflicts-Indep: python
Build-Conflicts-Arch: gcc
Binary: foo, bar
Homepage: https://example.com/foo
Section: devel
Priority: optional
Architecture: any
Directory: pool/main/f/foo
Files:
 25dcf3b4b6b3b3b3b3b3b3b3b3b3b3b3 1234 foo_1.0.tar.gz
Checksums-Sha1:
 b72b5fae3b3b3b3b3b3b3b3b3b3b3b3 1234 foo_1.0.tar.gz
"#;
        let p: super::Source = s.parse().unwrap();
        assert_eq!(p.package(), Some("foo".to_string()));
        assert_eq!(p.version(), Some("1.0".parse().unwrap()));
        assert_eq!(
            p.maintainer(),
            Some("John Doe <john@example.com>".to_string())
        );
        assert_eq!(
            p.uploaders(),
            Some(vec!["Jane Doe <jane@example.com>".to_string()])
        );
        assert_eq!(p.standards_version(), Some("3.9.8".to_string()));
        assert_eq!(p.format(), Some("3.0 (quilt)".to_string()));
        assert_eq!(p.vcs_browser(), Some("https://example.com/foo".to_string()));
        assert_eq!(p.vcs_git(), Some("https://example.com/foo.git".to_string()));
        assert_eq!(
            p.build_depends_indep().map(|x| x.to_string()),
            Some("python".parse().unwrap())
        );
        assert_eq!(p.build_depends(), Some("debhelper (>= 9)".parse().unwrap()));
        assert_eq!(p.build_depends_arch(), Some("gcc".parse().unwrap()));
        assert_eq!(p.build_conflicts(), Some("bar".parse().unwrap()));
        assert_eq!(p.build_conflicts_indep(), Some("python".parse().unwrap()));
        assert_eq!(p.build_conflicts_arch(), Some("gcc".parse().unwrap()));
        assert_eq!(p.binary(), Some("foo, bar".parse().unwrap()));
        assert_eq!(p.homepage(), Some("https://example.com/foo".to_string()));
        assert_eq!(p.section(), Some("devel".to_string()));
        assert_eq!(p.priority(), Some(super::Priority::Optional));
        assert_eq!(p.architecture(), Some("any".to_string()));
        assert_eq!(p.directory(), Some("pool/main/f/foo".to_string()));
        assert_eq!(p.files().len(), 1);
        assert_eq!(
            p.files()[0].md5sum,
            "25dcf3b4b6b3b3b3b3b3b3b3b3b3b3b3".to_string()
        );
        assert_eq!(p.files()[0].size, 1234);
        assert_eq!(p.files()[0].filename, "foo_1.0.tar.gz".to_string());
        assert_eq!(p.checksums_sha1().len(), 1);
        assert_eq!(
            p.checksums_sha1()[0].sha1,
            "b72b5fae3b3b3b3b3b3b3b3b3b3b3b3".to_string()
        );
    }

    #[test]
    fn test_package() {
        let s = r#"Package: foo
Version: 1.0
Source: bar
Maintainer: John Doe <john@example.com>
Architecture: any
Depends: bar
Recommends: baz
Suggests: qux
Enhances: quux
Pre-Depends: quuz
Breaks: corge
Conflicts: grault
Replaces: garply
Provides: waldo
Section: devel
Priority: optional
Description: Foo is a bar
Homepage: https://example.com/foo
Description-md5: 1234
Tags: foo, bar
Filename: pool/main/f/foo/foo_1.0.deb
Size: 1234
Installed-Size: 1234
MD5sum: 1234
SHA256: 1234
Multi-Arch: same
"#;
        let p: super::Package = s.parse().unwrap();
        assert_eq!(p.name(), Some("foo".to_string()));
        assert_eq!(p.version(), Some("1.0".parse().unwrap()));
        assert_eq!(p.source(), Some("bar".to_string()));
        assert_eq!(
            p.maintainer(),
            Some("John Doe <john@example.com>".to_string())
        );
        assert_eq!(p.architecture(), Some("any".to_string()));
        assert_eq!(p.depends(), Some("bar".parse().unwrap()));
        assert_eq!(p.recommends(), Some("baz".parse().unwrap()));
        assert_eq!(p.suggests(), Some("qux".parse().unwrap()));
        assert_eq!(p.enhances(), Some("quux".parse().unwrap()));
        assert_eq!(p.pre_depends(), Some("quuz".parse().unwrap()));
        assert_eq!(p.breaks(), Some("corge".parse().unwrap()));
        assert_eq!(p.conflicts(), Some("grault".parse().unwrap()));
        assert_eq!(p.replaces(), Some("garply".parse().unwrap()));
        assert_eq!(p.provides(), Some("waldo".parse().unwrap()));
        assert_eq!(p.section(), Some("devel".to_string()));
        assert_eq!(p.priority(), Some(super::Priority::Optional));
        assert_eq!(p.description(), Some("Foo is a bar".to_string()));
        assert_eq!(
            p.homepage(),
            Some(url::Url::parse("https://example.com/foo").unwrap())
        );
        assert_eq!(p.description_md5(), Some("1234".to_string()));
        assert_eq!(
            p.tags("Tags"),
            Some(vec!["foo".to_string(), "bar".to_string()])
        );
        assert_eq!(
            p.filename(),
            Some("pool/main/f/foo/foo_1.0.deb".to_string())
        );
        assert_eq!(p.size(), Some(1234));
        assert_eq!(p.installed_size(), Some(1234));
        assert_eq!(p.md5sum(), Some("1234".to_string()));
        assert_eq!(p.sha256(), Some("1234".to_string()));
        assert_eq!(p.multi_arch(), Some(MultiArch::Same));
    }

    #[test]
    fn test_release() {
        let s = include_str!("../testdata/Release");
        let release: super::Release = s.parse().unwrap();

        assert_eq!(release.origin(), Some("Debian".to_string()));
        assert_eq!(release.label(), Some("Debian".to_string()));
        assert_eq!(release.suite(), Some("testing".to_string()));
        assert_eq!(
            release.architectures(),
            vec![
                "all".to_string(),
                "amd64".to_string(),
                "arm64".to_string(),
                "armel".to_string(),
                "armhf".to_string()
            ]
            .into()
        );
        assert_eq!(
            release.components(),
            vec![
                "main".to_string(),
                "contrib".to_string(),
                "non-free-firmware".to_string(),
                "non-free".to_string()
            ]
            .into()
        );
        assert_eq!(
            release.description(),
            Some("Debian x.y Testing distribution - Not Released".to_string())
        );
        assert_eq!(318, release.checksums_md5().len());
    }

    #[test]
    fn test_source_vcs_arch() {
        let s = r#"Package: foo
Version: 1.0
Vcs-Arch: https://example.com/arch/repo
"#;
        let p: super::Source = s.parse().unwrap();
        assert_eq!(
            p.vcs_arch(),
            Some("https://example.com/arch/repo".to_string())
        );
    }

    #[test]
    fn test_source_vcs_cvs() {
        let s = r#"Package: foo
Version: 1.0
Vcs-Cvs: :pserver:anoncvs@example.com:/cvs/repo
"#;
        let p: super::Source = s.parse().unwrap();
        assert_eq!(
            p.vcs_cvs(),
            Some(":pserver:anoncvs@example.com:/cvs/repo".to_string())
        );
    }

    #[test]
    fn test_source_set_priority() {
        let s = r#"Package: foo
Version: 1.0
Priority: optional
"#;
        let mut p: super::Source = s.parse().unwrap();
        p.set_priority(super::Priority::Required);
        assert_eq!(p.priority(), Some(super::Priority::Required));
    }

    #[test]
    fn test_release_set_suite() {
        let s = r#"Origin: Debian
Label: Debian
Suite: testing
"#;
        let mut release: super::Release = s.parse().unwrap();
        release.set_suite("unstable");
        assert_eq!(release.suite(), Some("unstable".to_string()));
    }

    #[test]
    fn test_release_codename() {
        let s = r#"Origin: Debian
Label: Debian
Suite: testing
Codename: trixie
"#;
        let release: super::Release = s.parse().unwrap();
        assert_eq!(release.codename(), Some("trixie".to_string()));
    }

    #[test]
    fn test_release_set_checksums_sha512() {
        let s = r#"Origin: Debian
Label: Debian
Suite: testing
"#;
        let mut release: super::Release = s.parse().unwrap();
        let checksums = vec![super::Sha512Checksum {
            sha512: "abc123".to_string(),
            size: 1234,
            filename: "test.deb".to_string(),
        }];
        release.set_checksums_sha512(checksums);
        assert_eq!(release.checksums_sha512().len(), 1);
    }

    #[test]
    fn test_package_set_replaces() {
        let s = r#"Package: foo
Version: 1.0
"#;
        let mut p: super::Package = s.parse().unwrap();
        let relations: Relations = "bar (>= 1.0.0)".parse().unwrap();
        p.set_replaces(relations);
        assert_eq!(p.replaces(), Some("bar (>= 1.0.0)".parse().unwrap()));
    }
}
