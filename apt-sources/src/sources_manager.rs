use crate::{Repositories, Repository, RepositoryType};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Default path for APT sources files
pub const DEFAULT_SOURCES_PATH: &str = "/etc/apt/sources.list.d";
/// Default path for APT keyring files
pub const DEFAULT_KEYRING_PATH: &str = "/etc/apt/trusted.gpg.d";

/// Manager for APT sources and keyrings
#[derive(Debug, Clone)]
pub struct SourcesManager {
    sources_dir: PathBuf,
    keyring_dir: PathBuf,
}

impl Default for SourcesManager {
    fn default() -> Self {
        Self {
            sources_dir: PathBuf::from(DEFAULT_SOURCES_PATH),
            keyring_dir: PathBuf::from(DEFAULT_KEYRING_PATH),
        }
    }
}

impl SourcesManager {
    /// Create a new SourcesManager with custom directories
    pub fn new(sources_dir: impl Into<PathBuf>, keyring_dir: impl Into<PathBuf>) -> Self {
        Self {
            sources_dir: sources_dir.into(),
            keyring_dir: keyring_dir.into(),
        }
    }

    /// Get the path to the sources directory
    pub fn sources_dir(&self) -> &Path {
        &self.sources_dir
    }

    /// Get the path to the keyring directory
    pub fn keyring_dir(&self) -> &Path {
        &self.keyring_dir
    }

    /// Generate a filename for a repository
    pub fn generate_filename(&self, name: &str, format: FileFormat) -> String {
        let sanitized = name.replace(['/', ':', ' '], "-").to_lowercase();

        match format {
            FileFormat::Deb822 => format!("{sanitized}.sources"),
            FileFormat::Legacy => format!("{sanitized}.list"),
        }
    }

    /// Get the full path for a repository file
    pub fn get_repository_path(&self, filename: &str) -> PathBuf {
        self.sources_dir.join(filename)
    }

    /// Get the full path for a keyring file
    pub fn get_keyring_path(&self, filename: &str) -> PathBuf {
        self.keyring_dir.join(filename)
    }

    /// Write repositories to a file
    pub fn write_repositories(&self, path: &Path, repositories: &Repositories) -> io::Result<()> {
        let mut file = fs::File::create(path)?;
        write!(file, "{repositories}")
    }

    /// Read repositories from a file
    pub fn read_repositories(&self, path: &Path) -> Result<Repositories, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file {}: {e}", path.display()))?;

        content
            .parse::<Repositories>()
            .map_err(|e| format!("Failed to parse repositories: {e}"))
    }

    /// List all repository files in the sources directory
    pub fn list_repository_files(&self) -> io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        if self.sources_dir.exists() {
            for entry in fs::read_dir(&self.sources_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "sources" || ext == "list" {
                            files.push(path);
                        }
                    }
                }
            }
        }

        Ok(files)
    }

    /// Scan all repository files and return their contents
    pub fn scan_all_repositories(&self) -> Result<Vec<(PathBuf, Repositories)>, String> {
        let mut results = Vec::new();

        let files = self
            .list_repository_files()
            .map_err(|e| format!("Failed to list repository files: {}", e))?;

        for file in files {
            match self.read_repositories(&file) {
                Ok(repos) => results.push((file, repos)),
                Err(e) => {
                    // Log error but continue scanning
                    eprintln!("Warning: Failed to read {}: {}", file.display(), e);
                }
            }
        }

        Ok(results)
    }

    /// Check if a repository already exists in any file
    pub fn repository_exists(&self, repository: &Repository) -> Result<Option<PathBuf>, String> {
        let all_repos = self.scan_all_repositories()?;

        for (path, repos) in all_repos {
            for repo in repos.iter() {
                if repos_match(repo, repository) {
                    return Ok(Some(path));
                }
            }
        }

        Ok(None)
    }

    /// Ensure the sources and keyring directories exist
    pub fn ensure_directories(&self) -> io::Result<()> {
        fs::create_dir_all(&self.sources_dir)?;
        fs::create_dir_all(&self.keyring_dir)?;
        Ok(())
    }

    /// Add a repository to a file, creating the file if it doesn't exist
    pub fn add_repository(&self, repository: &Repository, filename: &str) -> Result<(), String> {
        let path = self.get_repository_path(filename);

        // Check if repository already exists
        if let Some(existing_path) = self.repository_exists(repository)? {
            return Err(format!(
                "Repository already exists in {}",
                existing_path.display()
            ));
        }

        // Read existing repositories if file exists
        let mut repositories = if path.exists() {
            self.read_repositories(&path)?
        } else {
            Repositories::empty()
        };

        // Add the new repository
        repositories.push(repository.clone());

        // Write back to file
        self.write_repositories(&path, &repositories)
            .map_err(|e| format!("Failed to write repository: {e}"))
    }

    /// Remove a repository from all files
    pub fn remove_repository(&self, repository: &Repository) -> Result<bool, String> {
        let mut removed = false;
        let all_files = self.scan_all_repositories()?;

        for (path, mut repos) in all_files {
            let initial_count = repos.len();
            repos.retain(|r| !repos_match(r, repository));

            if repos.len() < initial_count {
                removed = true;
                if repos.is_empty() {
                    // Remove empty file
                    fs::remove_file(&path)
                        .map_err(|e| format!("Failed to remove {}: {e}", path.display()))?;
                } else {
                    // Write updated repositories
                    self.write_repositories(&path, &repos)
                        .map_err(|e| format!("Failed to update {}: {e}", path.display()))?;
                }
            }
        }

        Ok(removed)
    }

    /// Enable or disable a repository
    pub fn set_repository_enabled(
        &self,
        repository: &Repository,
        enabled: bool,
    ) -> Result<bool, String> {
        let mut modified = false;
        let all_files = self.scan_all_repositories()?;

        for (path, mut repos) in all_files {
            let mut changed = false;
            for repo in repos.iter_mut() {
                if repos_match(repo, repository) && repo.enabled != Some(enabled) {
                    repo.enabled = Some(enabled);
                    changed = true;
                    modified = true;
                }
            }

            if changed {
                self.write_repositories(&path, &repos)
                    .map_err(|e| format!("Failed to update {}: {}", path.display(), e))?;
            }
        }

        Ok(modified)
    }

    /// Add a component to all matching repositories
    pub fn add_component_to_repositories(
        &self,
        component: &str,
        filter: impl Fn(&Repository) -> bool,
    ) -> Result<u32, String> {
        let mut modified_count = 0;
        let all_files = self.scan_all_repositories()?;

        for (path, mut repos) in all_files {
            let mut changed = false;

            for repo in repos.iter_mut() {
                if filter(repo) {
                    if let Some(components) = &mut repo.components {
                        if !components.contains(&component.to_string()) {
                            components.push(component.to_string());
                            changed = true;
                            modified_count += 1;
                        }
                    } else {
                        repo.components = Some(vec![component.to_string()]);
                        changed = true;
                        modified_count += 1;
                    }
                }
            }

            if changed {
                self.write_repositories(&path, &repos)
                    .map_err(|e| format!("Failed to update {}: {}", path.display(), e))?;
            }
        }

        Ok(modified_count)
    }

    /// Enable source repositories
    pub fn enable_source_repositories(
        &self,
        create_if_missing: bool,
    ) -> Result<(u32, u32), String> {
        let mut enabled_count = 0;
        let mut created_count = 0;
        let all_files = self.scan_all_repositories()?;

        for (path, mut repos) in all_files {
            let mut changed = false;
            let mut new_repos = Vec::new();

            for repo in repos.iter_mut() {
                // Check if this repo has binary type but not source
                if repo.types.contains(&RepositoryType::Binary)
                    && !repo.types.contains(&RepositoryType::Source)
                {
                    if repo.enabled == Some(false) {
                        // Just enable existing disabled source repo
                        repo.types.insert(RepositoryType::Source);
                        repo.enabled = Some(true);
                        enabled_count += 1;
                        changed = true;
                    } else if create_if_missing {
                        // Create a new source repository entry
                        let mut source_repo = repo.clone();
                        source_repo.types = HashSet::from([RepositoryType::Source]);
                        new_repos.push(source_repo);
                        created_count += 1;
                        changed = true;
                    }
                }
            }

            // Add any new repositories
            repos.extend(new_repos);

            if changed {
                self.write_repositories(&path, &repos)
                    .map_err(|e| format!("Failed to update {}: {}", path.display(), e))?;
            }
        }

        Ok((enabled_count, created_count))
    }

    /// List all repositories with their file paths
    pub fn list_all_repositories(&self) -> Result<Vec<(PathBuf, Repository)>, String> {
        let files = self.scan_all_repositories()?;

        // Pre-calculate capacity to avoid reallocations
        let total_repos: usize = files.iter().map(|(_, repos)| repos.len()).sum();
        let mut all_repos = Vec::with_capacity(total_repos);

        for (path, repos) in files {
            for repo in repos.iter() {
                all_repos.push((path.clone(), repo.clone()));
            }
        }

        Ok(all_repos)
    }

    /// Generate a keyring filename for a repository
    pub fn generate_keyring_filename(&self, repository_name: &str) -> String {
        let sanitized = repository_name.replace(['/', ':', ' '], "-").to_lowercase();
        format!("{sanitized}.gpg")
    }

    /// Save a GPG key to the keyring directory
    pub fn save_key(&self, key_data: &[u8], filename: &str) -> io::Result<PathBuf> {
        let key_path = self.get_keyring_path(filename);
        fs::write(&key_path, key_data)?;
        Ok(key_path)
    }
}

/// File format for APT source list files
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileFormat {
    /// Deb822 format (new style)
    Deb822,
    /// Legacy format (one-line style)
    Legacy,
}

/// Check if two repositories match (have the same URIs, suites, and components)
fn repos_match(repo1: &Repository, repo2: &Repository) -> bool {
    // Compare types
    if repo1.types != repo2.types {
        return false;
    }

    // Compare URIs
    let uris1: HashSet<_> = repo1.uris.iter().collect();
    let uris2: HashSet<_> = repo2.uris.iter().collect();
    if uris1 != uris2 {
        return false;
    }

    // Compare suites
    let suites1: HashSet<_> = repo1.suites.iter().collect();
    let suites2: HashSet<_> = repo2.suites.iter().collect();
    if suites1 != suites2 {
        return false;
    }

    // Compare components
    let components1: HashSet<_> = repo1.components.iter().collect();
    let components2: HashSet<_> = repo2.components.iter().collect();
    if components1 != components2 {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use url::Url;

    fn create_test_manager() -> (SourcesManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let sources_dir = temp_dir.path().join("sources.list.d");
        let keyring_dir = temp_dir.path().join("trusted.gpg.d");

        let manager = SourcesManager::new(&sources_dir, &keyring_dir);
        (manager, temp_dir)
    }

    fn create_test_repository() -> Repository {
        Repository {
            enabled: Some(true),
            types: HashSet::from([RepositoryType::Binary]),
            uris: vec![Url::parse("http://example.com/ubuntu").unwrap()],
            suites: vec!["focal".to_string()],
            components: Some(vec!["main".to_string()]),
            architectures: Some(vec!["amd64".to_string()]),
            ..Default::default()
        }
    }

    #[test]
    fn test_ensure_directories() {
        let (manager, _temp_dir) = create_test_manager();

        assert!(!manager.sources_dir().exists());
        assert!(!manager.keyring_dir().exists());

        manager.ensure_directories().unwrap();

        assert!(manager.sources_dir().exists());
        assert!(manager.keyring_dir().exists());
    }

    #[test]
    fn test_generate_filename() {
        let (manager, _) = create_test_manager();

        assert_eq!(
            manager.generate_filename("test-repo", FileFormat::Deb822),
            "test-repo.sources"
        );

        assert_eq!(
            manager.generate_filename("Test/Repo:Name", FileFormat::Legacy),
            "test-repo-name.list"
        );
    }

    #[test]
    fn test_add_repository() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let repo = create_test_repository();

        // Add repository
        manager.add_repository(&repo, "test.sources").unwrap();

        // Verify it was added
        let path = manager.get_repository_path("test.sources");
        assert!(path.exists());

        let repos = manager.read_repositories(&path).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].uris[0].as_str(), "http://example.com/ubuntu");

        // Try to add duplicate - should fail
        let result = manager.add_repository(&repo, "test2.sources");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn test_remove_repository() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let repo = create_test_repository();

        // Add repository
        manager.add_repository(&repo, "test.sources").unwrap();

        // Remove it
        let removed = manager.remove_repository(&repo).unwrap();
        assert!(removed);

        // Verify file was removed
        let path = manager.get_repository_path("test.sources");
        assert!(!path.exists());

        // Try to remove again - should return false
        let removed = manager.remove_repository(&repo).unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_set_repository_enabled() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let mut repo = create_test_repository();
        repo.enabled = Some(true);

        // Add repository
        manager.add_repository(&repo, "test.sources").unwrap();

        // Disable it
        let modified = manager.set_repository_enabled(&repo, false).unwrap();
        assert!(modified);

        // Verify it was disabled
        let path = manager.get_repository_path("test.sources");
        let repos = manager.read_repositories(&path).unwrap();
        assert_eq!(repos[0].enabled, Some(false));

        // Enable it again
        let modified = manager.set_repository_enabled(&repo, true).unwrap();
        assert!(modified);

        // Verify it was enabled
        let repos = manager.read_repositories(&path).unwrap();
        assert_eq!(repos[0].enabled, Some(true));
    }

    #[test]
    fn test_add_component_to_repositories() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let repo = create_test_repository();

        // Add repository
        manager.add_repository(&repo, "test.sources").unwrap();

        // Add component to repositories from example.com
        let count = manager
            .add_component_to_repositories("universe", |r| {
                r.uris.iter().any(|u| u.host_str() == Some("example.com"))
            })
            .unwrap();
        assert_eq!(count, 1);

        // Verify component was added
        let path = manager.get_repository_path("test.sources");
        let repos = manager.read_repositories(&path).unwrap();
        assert!(repos[0]
            .components
            .as_ref()
            .unwrap()
            .contains(&"universe".to_string()));

        // Try to add same component again - should not modify
        let count = manager
            .add_component_to_repositories("universe", |r| {
                r.uris.iter().any(|u| u.host_str() == Some("example.com"))
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_enable_source_repositories() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let repo = create_test_repository();

        // Add repository
        manager.add_repository(&repo, "test.sources").unwrap();

        // Enable source repositories (create if missing)
        let (enabled, created) = manager.enable_source_repositories(true).unwrap();
        assert_eq!(enabled, 0);
        assert_eq!(created, 1);

        // Verify source repo was created
        let path = manager.get_repository_path("test.sources");
        let repos = manager.read_repositories(&path).unwrap();
        assert_eq!(repos.len(), 2);

        // Find the source repo
        let source_repo = repos
            .iter()
            .find(|r| r.types.contains(&RepositoryType::Source))
            .unwrap();
        assert!(source_repo.types.contains(&RepositoryType::Source));
        assert!(!source_repo.types.contains(&RepositoryType::Binary));
    }

    #[test]
    fn test_list_repository_files() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        // Initially empty
        let files = manager.list_repository_files().unwrap();
        assert_eq!(files.len(), 0);

        // Add some files
        let repo1 = create_test_repository();
        let mut repo2 = create_test_repository();
        repo2.suites = vec!["jammy".to_string()]; // Make it different
        manager.add_repository(&repo1, "test1.sources").unwrap();
        manager.add_repository(&repo2, "test2.list").unwrap();

        // Should find both files
        let files = manager.list_repository_files().unwrap();
        assert_eq!(files.len(), 2);

        // Create a non-repository file
        let non_repo = manager.get_repository_path("test.txt");
        fs::write(&non_repo, "not a repo").unwrap();

        // Should still only find 2 repository files
        let files = manager.list_repository_files().unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_scan_all_repositories() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let repo1 = create_test_repository();
        let mut repo2 = create_test_repository();
        repo2.uris = vec![Url::parse("http://example2.com/ubuntu").unwrap()];

        // Add repositories to different files
        manager.add_repository(&repo1, "test1.sources").unwrap();
        manager.add_repository(&repo2, "test2.sources").unwrap();

        // Scan all
        let all_repos = manager.scan_all_repositories().unwrap();
        assert_eq!(all_repos.len(), 2);

        // Each file should have one repository
        for (_, repos) in all_repos {
            assert_eq!(repos.len(), 1);
        }
    }

    #[test]
    fn test_repository_exists() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let repo = create_test_repository();

        // Should not exist initially
        assert!(manager.repository_exists(&repo).unwrap().is_none());

        // Add repository
        manager.add_repository(&repo, "test.sources").unwrap();

        // Should exist now
        let existing_path = manager.repository_exists(&repo).unwrap();
        assert!(existing_path.is_some());
        assert!(existing_path.unwrap().ends_with("test.sources"));
    }

    #[test]
    fn test_save_key() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let key_data =
            b"-----BEGIN PGP PUBLIC KEY BLOCK-----\ntest key\n-----END PGP PUBLIC KEY BLOCK-----";

        let key_path = manager.save_key(key_data, "test.gpg").unwrap();
        assert!(key_path.exists());
        assert_eq!(key_path.file_name().unwrap(), "test.gpg");

        let saved_data = fs::read(&key_path).unwrap();
        assert_eq!(saved_data, key_data);
    }

    #[test]
    fn test_repos_match() {
        let repo1 = create_test_repository();
        let mut repo2 = repo1.clone();

        // Should match identical repos
        assert!(repos_match(&repo1, &repo2));

        // Different types
        repo2.types.insert(RepositoryType::Source);
        assert!(!repos_match(&repo1, &repo2));
        repo2.types = repo1.types.clone();

        // Different URIs
        repo2.uris.push(Url::parse("http://extra.com").unwrap());
        assert!(!repos_match(&repo1, &repo2));
        repo2.uris = repo1.uris.clone();

        // Different suites
        repo2.suites.push("bionic".to_string());
        assert!(!repos_match(&repo1, &repo2));
        repo2.suites = repo1.suites.clone();

        // Different components
        if let Some(ref mut components) = repo2.components {
            components.push("universe".to_string());
        }
        assert!(!repos_match(&repo1, &repo2));
    }

    #[test]
    fn test_generate_keyring_filename() {
        let (manager, _) = create_test_manager();

        // Test basic filename
        assert_eq!(
            manager.generate_keyring_filename("test-repo"),
            "test-repo.gpg"
        );

        // Test with special characters that should be sanitized
        assert_eq!(
            manager.generate_keyring_filename("Test/Repo:Name With Spaces"),
            "test-repo-name-with-spaces.gpg"
        );

        // Test empty string
        assert_eq!(manager.generate_keyring_filename(""), ".gpg");
    }

    #[test]
    fn test_list_all_repositories() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        // Initially empty
        let all_repos = manager.list_all_repositories().unwrap();
        assert!(all_repos.is_empty());

        // Add some repositories
        let repo1 = create_test_repository();
        let mut repo2 = create_test_repository();
        repo2.suites = vec!["jammy".to_string()];

        manager.add_repository(&repo1, "test1.sources").unwrap();
        manager.add_repository(&repo2, "test2.sources").unwrap();

        // Should list all repositories with their paths
        let all_repos = manager.list_all_repositories().unwrap();
        assert_eq!(all_repos.len(), 2);

        // Check that paths are included
        let paths: Vec<_> = all_repos
            .iter()
            .map(|(p, _)| p.file_name().unwrap())
            .collect();
        assert!(paths.contains(&std::ffi::OsStr::new("test1.sources")));
        assert!(paths.contains(&std::ffi::OsStr::new("test2.sources")));
    }

    #[test]
    fn test_enable_source_repositories_counter_edge_cases() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        // Add a binary repository
        let mut repo = create_test_repository();
        repo.types = HashSet::from([RepositoryType::Binary]);
        manager.add_repository(&repo, "test.sources").unwrap();

        // Enable source repositories with creation
        let (enabled, created) = manager.enable_source_repositories(true).unwrap();
        assert_eq!(enabled, 0);
        assert_eq!(created, 1);

        // Check that source repo was actually created
        let all_repos = manager.list_all_repositories().unwrap();
        let source_repos: Vec<_> = all_repos
            .iter()
            .filter(|(_, r)| {
                r.types.contains(&RepositoryType::Source)
                    && !r.types.contains(&RepositoryType::Binary)
            })
            .collect();
        assert_eq!(source_repos.len(), 1);

        // Add a disabled binary repository
        let mut disabled_repo = create_test_repository();
        disabled_repo.types = HashSet::from([RepositoryType::Binary]);
        disabled_repo.enabled = Some(false);
        disabled_repo.suites = vec!["jammy".to_string()]; // Make it different
        manager
            .add_repository(&disabled_repo, "test2.sources")
            .unwrap();

        // Enable source repositories again
        // The first binary repo will create another source repo
        // The disabled binary repo will have source type added and be enabled
        let (enabled2, created2) = manager.enable_source_repositories(true).unwrap();
        assert_eq!(enabled2, 1); // Should enable the disabled binary repo
        assert_eq!(created2, 1); // Will create a source repo for the new binary repo
    }

    #[test]
    fn test_set_repository_enabled_edge_cases() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let mut repo = create_test_repository();
        repo.enabled = Some(true);

        // Add repository
        manager.add_repository(&repo, "test.sources").unwrap();

        // Try to enable already enabled repo - should return false
        let modified = manager.set_repository_enabled(&repo, true).unwrap();
        assert!(!modified);

        // Disable it
        let modified = manager.set_repository_enabled(&repo, false).unwrap();
        assert!(modified);

        // Try to disable already disabled repo - should return false
        let modified = manager.set_repository_enabled(&repo, false).unwrap();
        assert!(!modified);
    }

    #[test]
    fn test_add_component_edge_cases() {
        let (manager, _) = create_test_manager();
        manager.ensure_directories().unwrap();

        let mut repo = create_test_repository();
        repo.components = Some(vec!["main".to_string()]);

        manager.add_repository(&repo, "test.sources").unwrap();

        // Add component that already exists - should not increment counter
        let count = manager
            .add_component_to_repositories("main", |_| true)
            .unwrap();
        assert_eq!(count, 0);

        // Add new component
        let count = manager
            .add_component_to_repositories("universe", |_| true)
            .unwrap();
        assert_eq!(count, 1);

        // Add to repo with no components initially
        let mut repo2 = create_test_repository();
        repo2.components = None;
        manager.add_repository(&repo2, "test2.sources").unwrap();

        let count = manager
            .add_component_to_repositories("restricted", |r| r.components.is_none())
            .unwrap();
        assert_eq!(count, 1);
    }
}
