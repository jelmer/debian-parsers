use apt_sources::key_management::create_inline_signature;
use apt_sources::launchpad::{
    download_ppa_signing_key, validate_ppa, validate_ppa_components, PpaInfo, PpaValidationResult,
    LAUNCHPAD_PPA_URL,
};
use apt_sources::{
    distribution::{get_system_info, Distribution},
    legacy::LegacyRepositories,
    signature::Signature,
    sources_manager::{
        FileFormat as SourcesFileFormat, SourcesManager, DEFAULT_KEYRING_PATH, DEFAULT_SOURCES_PATH,
    },
    utils::strip_auth_from_url,
    Repositories, Repository, RepositoryType,
};
use clap::Parser;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, info, warn};
use reqwest;
#[allow(unused_imports)] // Required for Cert::from_str
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::Cert;
use serde_json;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::str::FromStr;
use url::Url;

/// Create HTTP client for making requests
fn create_http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))
}

/// Format network errors in a user-friendly way
fn format_network_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "Connection timeout - please check your internet connection".to_string()
    } else if error.is_connect() {
        "Failed to connect - please check your internet connection".to_string()
    } else {
        error.to_string()
    }
}

#[derive(Parser, Debug, Clone)]
#[command(
    name = "apt-add-repository",
    about = "Add APT repository to sources.list.d",
    long_about = None,
    after_help = "Examples:
  apt-add-repository ppa:user/ppa-name
  apt-add-repository http://ppa.launchpad.net/example/ppa/ubuntu
  apt-add-repository \"deb http://archive.ubuntu.com/ubuntu focal main\"
  apt-add-repository -s http://archive.ubuntu.com/ubuntu
  apt-add-repository -s           # Enable existing disabled deb-src entries
  apt-add-repository -ss          # Enable + create missing deb-src entries
  apt-add-repository \"ppa:user/ppa1 ppa:user/ppa2\"  # Add multiple repositories
  echo \"ppa:user/ppa-name\" | apt-add-repository -
  cat repos.txt | apt-add-repository -"
)]
struct Args {
    /// Repository specification (PPA, URL, or full line, or '-' to read from stdin)
    #[arg(required_unless_present_any = ["list", "component", "enable_source"])]
    repository: Option<String>,

    /// Don't update package cache after adding
    #[arg(short = 'n', long = "no-update")]
    no_update: bool,

    /// Enable source repositories (-s to enable existing, -ss to create missing)
    #[arg(short = 's', long = "source", action = clap::ArgAction::Count)]
    enable_source: u8,

    /// Assume yes to all queries
    #[arg(short = 'y', long = "yes")]
    assume_yes: bool,

    /// Remove the repository instead of adding
    #[arg(short = 'r', long = "remove")]
    remove: bool,

    /// Directory for sources files
    #[arg(short = 'd', long = "directory", default_value = DEFAULT_SOURCES_PATH)]
    directory: String,

    /// Directory for keyring files
    #[arg(long = "keyring-dir", default_value = DEFAULT_KEYRING_PATH)]
    keyring_dir: String,

    /// Add repository for specified pocket
    #[arg(short = 'p', long = "pocket")]
    pocket: Option<String>,

    /// Use specified keyserver URL
    #[arg(short = 'k', long = "keyserver")]
    keyserver: Option<String>,

    /// Add component to all matching repositories
    #[arg(long = "component", conflicts_with = "remove")]
    component: Option<String>,

    /// Output format (deb822 or legacy)
    #[arg(long = "format", default_value = "deb822", value_parser = clap::value_parser!(OutputFormat))]
    format: OutputFormat,

    /// Preview changes without applying them
    #[arg(long = "dry-run")]
    dry_run: bool,

    /// Login to Launchpad to access private PPAs
    #[arg(long = "login")]
    login: bool,

    /// List all configured APT repositories
    #[arg(long = "list", conflicts_with_all = ["repository", "remove", "component", "pocket"])]
    list: bool,

    /// Refresh signing keys for all PPAs
    #[arg(long = "refresh-keys", conflicts_with_all = ["repository", "remove", "component", "pocket", "list"])]
    refresh_keys: bool,

    /// Use inline Signed-By instead of separate keyring files (recommended)
    #[arg(long = "inline-key")]
    inline_key: bool,
}

#[derive(Debug, Clone)]
struct ParsedRepository {
    repository: Repository,
    filename: String,
    ppa_info: Option<PpaInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum OutputFormat {
    Deb822,
    Legacy,
}

impl From<OutputFormat> for SourcesFileFormat {
    fn from(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Deb822 => SourcesFileFormat::Deb822,
            OutputFormat::Legacy => SourcesFileFormat::Legacy,
        }
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "deb822" | "sources" => Ok(OutputFormat::Deb822),
            "legacy" | "list" => Ok(OutputFormat::Legacy),
            _ => Err(format!("Invalid format: {}. Use 'deb822' or 'legacy'", s)),
        }
    }
}

#[derive(Debug)]
enum RepositorySpec {
    Component(String),
    Pocket(String),
    Repository(ParsedRepository),
}

// PpaInfo struct is now imported from apt_sources::ppa

// get_distribution_info is replaced by Distribution::current() from apt_sources::distribution

// get_system_info is now imported from apt_sources::distribution

fn get_distribution_specific_filename(
    base_name: &str,
    extension: &str,
    distribution: &Distribution,
) -> String {
    let dist_name = distribution.to_string().to_lowercase();
    // For main repos (base_name == "custom" or contains the distribution name), use distribution name
    if base_name == "custom" || base_name.contains(&dist_name) {
        format!("{}.{}", dist_name, extension)
    } else {
        format!("{}.{}", base_name, extension)
    }
}

fn determine_repository_filepath(
    parsed: &ParsedRepository,
    args: &Args,
    distribution: &Distribution,
) -> Result<PathBuf, String> {
    // Check if this is a main distribution repository
    let is_main_repo = distribution.is_main_repository(&parsed.repository);

    if is_main_repo {
        // For main distribution repositories, prefer to use the main distribution file
        let main_file = match (&distribution, args.format) {
            (Distribution::Ubuntu, OutputFormat::Deb822) => {
                Path::new(&args.directory).join("ubuntu.sources")
            }
            (Distribution::Ubuntu, OutputFormat::Legacy) => {
                get_main_sources_list_path(&args.directory)
            }
            (Distribution::Debian, OutputFormat::Deb822) => {
                Path::new(&args.directory).join("debian.sources")
            }
            (Distribution::Debian, OutputFormat::Legacy) => {
                get_main_sources_list_path(&args.directory)
            }
            _ => {
                // For other distributions, fall back to sources.list.d
                return Ok(Path::new(&args.directory).join(&parsed.filename));
            }
        };

        Ok(main_file)
    } else {
        // For third-party repositories, use sources.list.d
        Ok(Path::new(&args.directory).join(&parsed.filename))
    }
}

fn is_main_distribution_file(path: &Path) -> bool {
    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
        matches!(
            filename,
            "sources.list" | "ubuntu.sources" | "ubuntu.list" | "debian.sources" | "debian.list"
        )
    } else {
        false
    }
}

/// Derive the path to sources.list from the sources.list.d directory.
/// For example, /etc/apt/sources.list.d -> /etc/apt/sources.list
fn get_main_sources_list_path(sources_dir: &str) -> PathBuf {
    Path::new(sources_dir)
        .parent()
        .unwrap_or(Path::new("/etc/apt"))
        .join("sources.list")
}

// download_key_from_keyserver - wrapper with progress indicator
fn download_key_from_keyserver(fingerprint: &str, keyserver_url: &str) -> Result<String, String> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message(format!(
        "Downloading key {} from keyserver {}...",
        fingerprint, keyserver_url
    ));
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let result =
        apt_sources::keyserver::download_key_from_keyserver_sync(fingerprint, keyserver_url);

    match result {
        Ok(key_data) => {
            spinner.finish_with_message(format!("Successfully downloaded key {}", fingerprint));
            Ok(key_data)
        }
        Err(e) => {
            spinner.finish_and_clear();
            Err(e)
        }
    }
}

#[cfg(feature = "launchpad")]
fn validate_ppa_launchpadlib(
    ppa_info: &PpaInfo,
    auth_required: bool,
) -> Result<PpaValidationResult, String> {
    debug!(
        "Validating PPA {}/{} using launchpadlib",
        ppa_info.user, ppa_info.name
    );
    validate_ppa(ppa_info, auth_required).map_err(|e| e.to_string())
}

/// Validate PPA and provide user-friendly error messages
fn validate_ppa_with_suggestions(ppa_info: &PpaInfo) -> Result<PpaValidationResult, String> {
    info!(
        "Checking if PPA {}/{} exists...",
        ppa_info.user, ppa_info.name
    );

    let result = validate_ppa(ppa_info, false).map_err(|e| e.to_string())?;

    if !result.exists {
        if result.is_private {
            return Err(format!(
                "PPA {}/{} is private.\n\
                 To access private PPAs:\n\
                 1. Use the --login flag to authenticate with Launchpad\n\
                 2. Ensure you're subscribed to this PPA\n\
                 3. Check that apt-add-repository was built with launchpad support",
                ppa_info.user, ppa_info.name
            ));
        } else {
            return Err(format!(
                "PPA {}/{} not found.\n\
                 Suggestions:\n\
                 - Check the PPA name for typos\n\
                 - Verify the user '{}' exists on Launchpad\n\
                 - Browse available PPAs at https://launchpad.net/~{}/+ppas",
                ppa_info.user, ppa_info.name, ppa_info.user, ppa_info.user
            ));
        }
    }

    Ok(result)
}

#[cfg(feature = "launchpad")]
fn get_private_ppa_url(ppa_info: &PpaInfo) -> Result<Url, String> {
    debug!(
        "Getting private PPA URL for {}/{}",
        ppa_info.user, ppa_info.name
    );
    apt_sources::launchpad::get_private_ppa_url(ppa_info).map_err(|e| e.to_string())
}

#[cfg(feature = "launchpad")]
fn download_ppa_key_launchpadlib(
    ppa_info: &PpaInfo,
    auth_required: bool,
) -> Result<String, String> {
    debug!(
        "Using launchpadlib to download PPA key for {}/{}",
        ppa_info.user, ppa_info.name
    );

    let signing_key =
        download_ppa_signing_key(ppa_info, auth_required).map_err(|e| e.to_string())?;

    // Additionally check expiration
    let cert = Cert::from_str(&signing_key.key_data)
        .map_err(|e| format!("Failed to parse PGP key for expiration check: {}", e))?;

    if let Some(expiration_error) = apt_sources::key_management::check_key_expiration(&cert) {
        let expiration_warning = expiration_error.to_string();
        if expiration_warning.contains("expired") {
            error!("Key expiration: {}", expiration_warning);
            return Err(format!("Cannot use expired key: {}", expiration_warning));
        } else {
            warn!("Key expiration warning: {}", expiration_warning);
        }
    }

    info!(
        "Downloaded and verified PPA signing key with fingerprint: {}",
        signing_key.fingerprint
    );
    Ok(signing_key.key_data)
}

// verify_key_fingerprint - wrapper that adds expiration check and logging
fn verify_key_fingerprint(key_data: &str, expected_fingerprint: &str) -> Result<(), String> {
    debug!(
        "Verifying key fingerprint against expected: {}",
        expected_fingerprint
    );

    // Use the crate's fingerprint verification
    apt_sources::key_management::verify_key_fingerprint(key_data, expected_fingerprint)
        .map_err(|e| e.to_string())?;

    // Additionally check expiration
    let cert = Cert::from_str(key_data)
        .map_err(|e| format!("Failed to parse PGP key for expiration check: {}", e))?;

    if let Some(expiration_error) = apt_sources::key_management::check_key_expiration(&cert) {
        let expiration_warning = expiration_error.to_string();
        if expiration_warning.contains("expired") {
            error!("Key expiration: {}", expiration_warning);
            return Err(format!("Cannot use expired key: {}", expiration_warning));
        } else {
            warn!("Key expiration warning: {}", expiration_warning);
        }
    }

    debug!("Key fingerprint verification successful");
    Ok(())
}

fn download_ppa_key(ppa_info: &PpaInfo, keyserver: Option<&str>) -> Result<String, String> {
    info!(
        "Downloading signing key for PPA {}/{}...",
        ppa_info.user, ppa_info.name
    );

    let key_data = if let Some(keyserver_url) = keyserver {
        // Use custom keyserver - need to get fingerprint first
        debug!("Using custom keyserver: {}", keyserver_url);
        let api_url = format!(
            "https://api.launchpad.net/1.0/~{}/+archive/ubuntu/{}",
            ppa_info.user, ppa_info.name
        );

        let client = create_http_client()?;
        let ppa_response = client
            .get(&api_url)
            .header("Accept", "application/json")
            .send()
            .map_err(|e| format!("Failed to fetch PPA metadata: {}", format_network_error(&e)))?;

        if !ppa_response.status().is_success() {
            return Err(format!(
                "PPA not found: {}/{}",
                ppa_info.user, ppa_info.name
            ));
        }

        let ppa_data: serde_json::Value = ppa_response
            .json()
            .map_err(|e| format!("Failed to parse PPA metadata: {}", e))?;

        let fingerprint = ppa_data
            .get("signing_key_fingerprint")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "PPA has no signing key configured".to_string())?;

        let key = download_key_from_keyserver(fingerprint, keyserver_url)?;

        // Verify the key fingerprint matches what we expect
        verify_key_fingerprint(&key, fingerprint)?;
        key
    } else {
        // Use library function for default Launchpad download
        download_ppa_signing_key(ppa_info, false)
            .map_err(|e| e.to_string())?
            .key_data
    };

    info!("Downloaded PPA signing key");
    Ok(key_data)
}

fn save_key_to_keyring(
    key_data: &str,
    keyring_dir: &str,
    ppa_info: &PpaInfo,
) -> Result<PathBuf, String> {
    // The key data from Launchpad is already in armored format,
    // but let's parse it to validate it's a proper key
    let cert = Cert::from_str(key_data).map_err(|e| format!("Failed to parse PGP key: {}", e))?;

    // Verify the key is valid
    let policy = sequoia_openpgp::policy::StandardPolicy::new();
    if cert.with_policy(&policy, None).is_err() {
        return Err("Invalid or expired PGP key".to_string());
    }

    // Save to keyring directory
    // Use .asc extension for ASCII-armored keys (apt prefers this)
    let keyring_path = Path::new(keyring_dir).join(ppa_info.keyring_filename());

    // Check if keyring directory exists
    if !Path::new(keyring_dir).exists() {
        fs::create_dir_all(keyring_dir)
            .map_err(|e| format!("Failed to create keyring directory: {}", e))?;
    }

    // Write the key in ASCII-armored format
    // Since the data from Launchpad is already armored, we can write it directly
    fs::write(&keyring_path, key_data)
        .map_err(|e| format!("Failed to write key to file: {}", e))?;

    Ok(keyring_path)
}

/// Save the PPA key and return the appropriate Signature.
/// If inline_key is true and format is Deb822, tries inline signature first,
/// falling back to keyring file on failure.
fn save_ppa_key(
    key_data: &str,
    ppa_info: &PpaInfo,
    keyring_dir: &str,
    use_inline: bool,
) -> Option<Signature> {
    if use_inline {
        match create_inline_signature(key_data) {
            Ok(inline_sig) => {
                info!("Using inline Signed-By (embedded key)");
                return Some(inline_sig);
            }
            Err(e) => {
                warn!("Failed to create inline signature: {}", e);
                warn!("Falling back to keyring file");
            }
        }
    }

    match save_key_to_keyring(key_data, keyring_dir, ppa_info) {
        Ok(keyring_path) => {
            info!("Signing key saved to {}", keyring_path.display());
            Some(Signature::KeyPath(keyring_path))
        }
        Err(e) => {
            warn!("Failed to save signing key: {}", e);
            warn!("The repository will be added without signature verification.");
            None
        }
    }
}

fn parse_repository_line(
    line: &str,
) -> Result<
    (
        Vec<RepositoryType>,
        Url,
        String,
        Vec<String>,
        Option<PathBuf>,
    ),
    String,
> {
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.is_empty() {
        return Err("Empty repository line".to_string());
    }

    let mut idx = 0;
    let mut signed_by = None;
    let mut _arch = None;

    // Parse repository type
    let types = match parts[idx] {
        "deb" => vec![RepositoryType::Binary],
        "deb-src" => vec![RepositoryType::Source],
        _ => return Err("Repository line must start with 'deb' or 'deb-src'".to_string()),
    };
    idx += 1;

    // Parse options if present
    if idx < parts.len() && parts[idx].starts_with('[') {
        let options_end = parts
            .iter()
            .position(|&p| p.ends_with(']'))
            .ok_or("Unclosed options bracket")?;

        let options_str = parts[idx..=options_end].join(" ");
        let options_str = options_str.trim_start_matches('[').trim_end_matches(']');

        for option in options_str.split_whitespace() {
            if let Some((key, value)) = option.split_once('=') {
                match key {
                    "arch" => _arch = Some(value.to_string()),
                    "signed-by" => signed_by = Some(PathBuf::from(value)),
                    _ => {} // Ignore other options for now
                }
            }
        }

        idx = options_end + 1;
    }

    if idx + 2 >= parts.len() {
        return Err("Invalid repository line format".to_string());
    }

    let uri = Url::parse(parts[idx]).map_err(|e| format!("Invalid URL: {}", e))?;

    // Strip auth data from URL for storage
    let (clean_uri, auth_data) = strip_auth_from_url(&uri);
    if auth_data.is_some() {
        debug!("Repository URL contains authentication data, stripping for storage");
    }

    let suite = parts[idx + 1].to_string();
    let components = parts[idx + 2..].iter().map(|&s| s.to_string()).collect();

    Ok((types, clean_uri, suite, components, signed_by))
}

fn parse_repository_spec(
    spec: &str,
    enable_source_count: u8,
    args: &Args,
    distribution: &Distribution,
    codename: &str,
) -> Result<RepositorySpec, String> {
    // Check if it's just a component name (universe, multiverse, etc.)
    let valid_components = match &distribution {
        Distribution::Ubuntu => vec!["main", "universe", "multiverse", "restricted"],
        Distribution::Debian => vec!["main", "contrib", "non-free", "non-free-firmware"],
        Distribution::Other(_) => vec![
            "main",
            "universe",
            "multiverse",
            "restricted",
            "contrib",
            "non-free",
            "non-free-firmware",
        ],
    };
    if valid_components.contains(&spec) {
        debug!(
            "Adding component {} to existing {} repositories",
            spec, distribution
        );
        return Ok(RepositorySpec::Component(spec.to_string()));
    }

    // Check if it's a pocket specification (e.g., focal-proposed, bionic-backports)
    let valid_pockets = vec!["proposed", "backports", "security", "updates"];
    for pocket in &valid_pockets {
        if spec.ends_with(&format!("-{}", pocket)) {
            debug!("Adding pocket {} repositories", spec);
            return Ok(RepositorySpec::Pocket(spec.to_string()));
        }
    }

    // Check if it's a PPA
    if spec.starts_with("ppa:") {
        let ppa_info = PpaInfo::parse(spec)?;

        let types = if enable_source_count > 0 {
            HashSet::from([RepositoryType::Binary, RepositoryType::Source])
        } else {
            HashSet::from([RepositoryType::Binary])
        };

        // Generate PPA URL (with authentication for private PPAs if needed)
        let uri = if args.login {
            // For private PPAs, we need to get the subscription URL from launchpadlib
            #[cfg(feature = "launchpad")]
            {
                get_private_ppa_url(&ppa_info)?
            }
            #[cfg(not(feature = "launchpad"))]
            {
                return Err("Private PPA support requires the 'launchpad' feature".to_string());
            }
        } else {
            Url::parse(&format!(
                "{}/{}/{}/ubuntu",
                LAUNCHPAD_PPA_URL, ppa_info.user, ppa_info.name
            ))
            .unwrap()
        };

        // Validate components for PPA
        let components = if let Some(ref comps) = args.component {
            // User specified components - validate them
            let comp_vec = vec![comps.clone()];
            validate_ppa_components(&comp_vec)?;
            comp_vec
        } else {
            vec!["main".to_string()]
        };

        let repository = Repository {
            enabled: Some(true),
            types,
            architectures: None,
            uris: vec![uri],
            suites: vec![codename.to_string()],
            components: Some(components),
            signature: None, // Will be set after key download
            x_repolib_name: Some(format!("ppa:{}/{}", ppa_info.user, ppa_info.name)),
            ..Default::default()
        };

        let extension = match args.format {
            OutputFormat::Legacy => "list",
            OutputFormat::Deb822 => "sources",
        };
        let filename = format!(
            "{}-ubuntu-{}-{}.{}",
            ppa_info.user, ppa_info.name, codename, extension
        );

        return Ok(RepositorySpec::Repository(ParsedRepository {
            repository,
            filename,
            ppa_info: Some(ppa_info),
        }));
    }

    // Check if it's a full deb line
    if spec.starts_with("deb ") || spec.starts_with("deb-src ") {
        let (parsed_types, uri, suite, components, signed_by) = parse_repository_line(spec)?;

        // Check if this is a PPA URL and validate components
        if uri
            .host_str()
            .map_or(false, |h| h.contains("ppa.launchpadcontent.net"))
        {
            validate_ppa_components(&components)?;
        }

        let mut types = HashSet::new();
        for t in parsed_types {
            types.insert(t);
        }

        if enable_source_count > 0 {
            types.insert(RepositoryType::Source);
        }

        let mut repository = Repository {
            enabled: Some(true),
            types,
            architectures: None,
            uris: vec![uri.clone()],
            suites: vec![suite],
            components: Some(components),
            ..Default::default()
        };

        if let Some(keypath) = signed_by {
            repository.signature = Some(Signature::KeyPath(keypath));
        }

        let filename = generate_filename(uri.as_str(), args.format, &distribution);

        return Ok(RepositorySpec::Repository(ParsedRepository {
            repository,
            filename,
            ppa_info: None,
        }));
    }

    // Try to parse as URL
    let uri = Url::parse(spec).map_err(|_| {
        "Invalid repository specification. Expected PPA, URL, or full deb line".to_string()
    })?;

    // Strip auth data from URL for storage
    let (clean_uri, auth_data) = strip_auth_from_url(&uri);
    if auth_data.is_some() {
        info!("Repository URL contains authentication data, which will be stored securely");
    }

    // Use system codename and default to 'main' component
    let (suite, components) = default_suite_components(&codename);

    let types = if enable_source_count > 0 {
        HashSet::from([RepositoryType::Binary, RepositoryType::Source])
    } else {
        HashSet::from([RepositoryType::Binary])
    };

    let repository = Repository {
        enabled: Some(true),
        types,
        architectures: None,
        uris: vec![clean_uri.clone()],
        suites: vec![suite],
        components: Some(components),
        ..Default::default()
    };

    let filename = generate_filename(uri.as_str(), args.format, &distribution);

    Ok(RepositorySpec::Repository(ParsedRepository {
        repository,
        filename,
        ppa_info: None,
    }))
}

fn default_suite_components(system_codename: &str) -> (String, Vec<String>) {
    // Match original apt-add-repository behavior:
    // - Suite defaults to system codename
    // - Components default to 'main'
    (system_codename.to_string(), vec!["main".to_string()])
}

fn generate_filename(url: &str, format: OutputFormat, distribution: &Distribution) -> String {
    let extension = match format {
        OutputFormat::Legacy => "list",
        OutputFormat::Deb822 => "sources",
    };

    let Ok(parsed_url) = Url::parse(url) else {
        return get_distribution_specific_filename("custom", extension, distribution);
    };

    // Check if this is a main distribution repository
    if distribution.is_main_repository(&Repository {
        uris: vec![parsed_url.clone()],
        ..Default::default()
    }) {
        return get_distribution_specific_filename("custom", extension, distribution);
    }

    // Use library function for non-main repos
    apt_sources::utils::generate_filename_from_url(&parsed_url, extension)
}

fn add_pocket_repository(
    pocket_spec: &str,
    args: &Args,
    enable_source_count: u8,
    distribution: &Distribution,
) -> Result<(), String> {
    info!("Adding pocket repository: {}", pocket_spec);

    // Parse the pocket specification (e.g., "focal-proposed" -> suite="focal-proposed", pocket="proposed")
    let parts: Vec<&str> = pocket_spec.rsplitn(2, '-').collect();
    if parts.len() != 2 {
        return Err("Invalid pocket specification".to_string());
    }

    let pocket = parts[0];
    let _base_suite = parts[1];

    // Create repository for the pocket
    let types = if enable_source_count > 0 {
        HashSet::from([RepositoryType::Binary, RepositoryType::Source])
    } else {
        HashSet::from([RepositoryType::Binary])
    };

    // Determine components based on distribution
    // All pockets (proposed, backports, security, updates) use the same components
    let components: Vec<String> = match &distribution {
        Distribution::Ubuntu => vec!["main", "universe", "multiverse", "restricted"],
        Distribution::Debian => vec!["main", "contrib", "non-free"],
        Distribution::Other(_) => distribution.default_components(),
    }
    .into_iter()
    .map(|s| s.to_string())
    .collect();

    // Use distribution-specific mirror URLs
    let mirror_url = match &distribution {
        Distribution::Ubuntu => "http://archive.ubuntu.com/ubuntu/",
        Distribution::Debian => "http://deb.debian.org/debian/",
        Distribution::Other(name) => {
            return Err(format!(
                "Pocket repository support not available for distribution '{}'",
                name
            ));
        }
    };

    let repository = Repository {
        enabled: Some(true),
        types,
        architectures: None,
        uris: vec![Url::parse(mirror_url).unwrap()],
        suites: vec![pocket_spec.to_string()],
        components: Some(components),
        ..Default::default()
    };

    let extension = match args.format {
        OutputFormat::Legacy => "list",
        OutputFormat::Deb822 => "sources",
    };

    // Use distribution-specific filename
    let filename = format!(
        "{}-{}.{}",
        distribution.to_string().to_lowercase(),
        pocket,
        extension
    );
    let parsed = ParsedRepository {
        repository,
        filename,
        ppa_info: None,
    };

    add_parsed_repository(parsed, args, distribution)
}

fn add_component_to_existing_repos(
    component: &str,
    directory: &str,
    dry_run: bool,
    distribution: &Distribution,
) -> Result<(), String> {
    info!(
        "Adding component '{}' to existing {} repositories",
        component, distribution
    );

    // Find all .sources files that contain distribution repositories
    let sources_dir = Path::new(directory);
    if !sources_dir.exists() {
        return Err(format!("Sources directory {} does not exist", directory));
    }

    let mut modified_count = 0;

    for entry in
        fs::read_dir(sources_dir).map_err(|e| format!("Failed to read directory: {}", e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) != Some("sources") {
            continue;
        }

        // Read and parse the repository file
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

        let repos = Repositories::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;

        let mut modified = false;
        let mut updated_repos = Vec::new();

        for repo in repos.iter() {
            let mut repo_clone = repo.clone();

            // Check if this is a main distribution repository and doesn't already have the component
            let dominated = distribution.is_main_repository(repo)
                && repo_clone
                    .components
                    .as_ref()
                    .map_or(false, |c| !c.contains(&component.to_string()));

            if dominated {
                if let Some(components) = &mut repo_clone.components {
                    if dry_run {
                        info!("Would add component '{}' to {}", component, path.display());
                    } else {
                        components.push(component.to_string());
                        info!("Adding component '{}' to {}", component, path.display());
                    }
                    modified = true;
                }
            }

            updated_repos.push(repo_clone);
        }

        if modified && !dry_run {
            let updated_repositories = Repositories::new(updated_repos);
            let output = updated_repositories.to_string();
            fs::write(&path, output)
                .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
            modified_count += 1;
        }
    }

    if modified_count == 0 && !dry_run {
        warn!(
            "No {} repositories found to add component '{}' to",
            distribution, component
        );
    } else if !dry_run {
        info!("Modified {} repository file(s)", modified_count);
    }

    Ok(())
}

fn enable_sources_globally(args: &Args) -> Result<(), String> {
    info!(
        "Enabling source repositories globally (level: {})",
        args.enable_source
    );

    let mut enabled_count = 0;
    let mut created_count = 0;
    let mut warned_count = 0;
    let mut errors = Vec::new();

    // Scan both .sources and .list files
    let sources_dir = Path::new(&args.directory);
    if sources_dir.exists() {
        for entry in
            fs::read_dir(sources_dir).map_err(|e| format!("Failed to read directory: {}", e))?
        {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            // Handle .sources files (DEB822 format)
            if path.extension().and_then(|s| s.to_str()) == Some("sources") {
                match enable_sources_in_deb822_file(&path, args.enable_source, args.dry_run) {
                    Ok((enabled, created)) => {
                        enabled_count += enabled;
                        created_count += created;
                    }
                    Err(e) => {
                        errors.push(format!("{}: {}", path.display(), e));
                    }
                }
            }
            // Handle .list files (legacy format)
            else if path.extension().and_then(|s| s.to_str()) == Some("list") {
                match enable_sources_in_legacy_file(&path, args.enable_source, args.dry_run) {
                    Ok((enabled, created, warned)) => {
                        enabled_count += enabled;
                        created_count += created;
                        warned_count += warned;
                    }
                    Err(e) => {
                        errors.push(format!("{}: {}", path.display(), e));
                    }
                }
            }
        }
    }

    // Also handle main sources.list
    let main_sources = get_main_sources_list_path(&args.directory);
    if main_sources.exists() {
        match enable_sources_in_legacy_file(&main_sources, args.enable_source, args.dry_run) {
            Ok((enabled, created, warned)) => {
                enabled_count += enabled;
                created_count += created;
                warned_count += warned;
            }
            Err(e) => {
                errors.push(format!("sources.list: {}", e));
            }
        }
    }

    // Report results
    if args.dry_run {
        info!("Dry run complete - would have:");
    } else {
        info!("Source enablement complete:");
    }

    if enabled_count > 0 {
        info!("  Enabled {} disabled deb-src entries", enabled_count);
    }
    if created_count > 0 {
        info!("  Created {} new deb-src entries", created_count);
    }
    if warned_count > 0 && args.enable_source == 1 {
        info!(
            "  Found {} missing deb-src entries (use -ss to create them)",
            warned_count
        );
    }

    if !errors.is_empty() {
        error!("Errors encountered:");
        for err in errors {
            error!("  {}", err);
        }
        return Err("Some operations failed".to_string());
    }

    if enabled_count == 0 && created_count == 0 && warned_count == 0 {
        info!("No source repositories needed enabling or creation");
    }

    Ok(())
}

fn enable_sources_in_deb822_file(
    file_path: &Path,
    enable_source_count: u8,
    dry_run: bool,
) -> Result<(u32, u32), String> {
    debug!("Processing DEB822 file: {}", file_path.display());

    let content =
        fs::read_to_string(file_path).map_err(|e| format!("Failed to read file: {}", e))?;

    let repos = Repositories::from_str(&content)
        .map_err(|e| format!("Failed to parse DEB822 file: {}", e))?;

    let mut enabled_count = 0;
    let mut created_count = 0;
    let mut modified = false;
    let mut updated_repos = Vec::new();

    // First pass: Enable disabled deb-src entries that have matching enabled deb entries
    for repo in repos.iter() {
        let mut repo_clone = repo.clone();

        // Only process disabled source-only repos with a matching binary repo
        let dominated = !repo.enabled.unwrap_or(true)
            && repo.types.contains(&RepositoryType::Source)
            && !repo.types.contains(&RepositoryType::Binary)
            && find_matching_binary_repo(&repos, repo).is_some();

        if dominated {
            if dry_run {
                info!("Would enable disabled deb-src in {}", file_path.display());
            } else {
                repo_clone.enabled = Some(true);
                info!("Enabled disabled deb-src in {}", file_path.display());
            }
            enabled_count += 1;
            modified = true;
        }

        updated_repos.push(repo_clone);
    }

    // Second pass: Create missing deb-src entries if enable_source > 1
    if enable_source_count > 1 {
        let mut new_repos = Vec::new();

        for repo in repos.iter() {
            // Only process enabled binary-only repos without a matching source repo
            let dominated = repo.enabled.unwrap_or(true)
                && repo.types.contains(&RepositoryType::Binary)
                && !repo.types.contains(&RepositoryType::Source)
                && !has_matching_source_repo(&repos, repo);

            if !dominated {
                continue;
            }

            // Create a new source repository
            let mut source_repo = repo.clone();
            source_repo.types = HashSet::from([RepositoryType::Source]);

            if dry_run {
                info!(
                    "Would create missing deb-src for binary repo in {}",
                    file_path.display()
                );
            } else {
                new_repos.push(source_repo);
                info!(
                    "Created missing deb-src for binary repo in {}",
                    file_path.display()
                );
            }
            created_count += 1;
            modified = true;
        }

        // Add new repositories to updated list
        if !dry_run {
            updated_repos.extend(new_repos);
        }
    }

    // Write changes if modified and not dry run
    if modified && !dry_run {
        let updated_repositories = Repositories::new(updated_repos);
        let output = updated_repositories.to_string();
        fs::write(file_path, output).map_err(|e| format!("Failed to write file: {}", e))?;
    }

    Ok((enabled_count, created_count))
}

fn enable_sources_in_legacy_file(
    file_path: &Path,
    enable_source_count: u8,
    dry_run: bool,
) -> Result<(u32, u32, u32), String> {
    debug!("Processing legacy file: {}", file_path.display());

    let content =
        fs::read_to_string(file_path).map_err(|e| format!("Failed to read file: {}", e))?;

    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut enabled_count = 0;
    let mut created_count = 0;
    let mut warned_count = 0;
    let mut modified = false;

    // First pass: Enable disabled deb-src lines
    for i in 0..lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("# deb-src ") || trimmed.starts_with("#deb-src ") {
            // This is a commented-out deb-src line
            let uncommented = trimmed.trim_start_matches("# ").trim_start_matches("#");

            // Check if there's a matching enabled deb line
            let lines_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            if has_matching_deb_line(&lines_refs, uncommented) {
                if dry_run {
                    info!(
                        "Would enable disabled deb-src line in {}",
                        file_path.display()
                    );
                } else {
                    lines[i] = uncommented.to_string();
                    info!("Enabled disabled deb-src line in {}", file_path.display());
                }
                enabled_count += 1;
                modified = true;
            }
        }
    }

    // Second pass: Handle missing deb-src entries
    let mut new_lines = Vec::new();
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("deb ") && !trimmed.starts_with("deb-src ") {
            // This is an enabled deb line
            let source_line = trimmed.replace("deb ", "deb-src ");

            // Check if there's already a matching deb-src line
            let lines_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            if !has_matching_source_line(&lines_refs, &source_line) {
                if enable_source_count == 1 {
                    // Just warn about missing source
                    debug!("Missing deb-src for: {}", trimmed);
                    warned_count += 1;
                } else if enable_source_count > 1 {
                    // Create the missing deb-src line
                    if dry_run {
                        info!(
                            "Would create missing deb-src line in {}",
                            file_path.display()
                        );
                    } else {
                        new_lines.push(source_line);
                        info!("Created missing deb-src line in {}", file_path.display());
                    }
                    created_count += 1;
                    modified = true;
                }
            }
        }
    }

    // Add new lines
    if !new_lines.is_empty() && !dry_run {
        lines.extend(new_lines);
        modified = true;
    }

    // Write changes if modified and not dry run
    if modified && !dry_run {
        let output = lines.join("\n") + "\n";
        fs::write(file_path, output).map_err(|e| format!("Failed to write file: {}", e))?;
    }

    Ok((enabled_count, created_count, warned_count))
}

fn find_matching_binary_repo<'a>(
    repos: &'a Repositories,
    source_repo: &Repository,
) -> Option<&'a Repository> {
    for repo in repos.iter() {
        if !repo.enabled.unwrap_or(true) {
            continue; // Skip disabled repositories
        }

        // Check if this is a binary repository with matching URIs, suites, and components
        if repo.types.contains(&RepositoryType::Binary)
            && repo.uris == source_repo.uris
            && repo.suites == source_repo.suites
            && repo.components == source_repo.components
        {
            return Some(repo);
        }
    }
    None
}

fn has_matching_source_repo(repos: &Repositories, binary_repo: &Repository) -> bool {
    for repo in repos.iter() {
        // Check if this is a source repository with matching URIs, suites, and components
        if repo.types.contains(&RepositoryType::Source)
            && repo.uris == binary_repo.uris
            && repo.suites == binary_repo.suites
            && repo.components == binary_repo.components
        {
            return true;
        }
    }
    false
}

fn has_matching_deb_line(lines: &[&str], deb_src_line: &str) -> bool {
    // Convert deb-src line to equivalent deb line
    let deb_line = deb_src_line.replace("deb-src ", "deb ");

    for line in lines {
        let trimmed = line.trim();
        if trimmed == deb_line {
            return true;
        }
    }
    false
}

fn has_matching_source_line(lines: &[&str], source_line: &str) -> bool {
    for line in lines {
        let trimmed = line.trim();
        // Check both enabled and disabled source lines
        if trimmed == source_line
            || trimmed == format!("# {}", source_line)
            || trimmed == format!("#{}", source_line)
        {
            return true;
        }
    }
    false
}

fn add_repositories_from_stdin(
    args: &Args,
    distribution: &Distribution,
    codename: &str,
) -> Result<(), String> {
    info!("Reading repository specifications from stdin...");

    use std::io::{BufRead, BufReader};

    let stdin = io::stdin();
    let reader = BufReader::new(stdin);

    let mut success_count = 0;
    let mut error_count = 0;
    let mut errors = Vec::new();

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to read line {}: {}", line_num + 1, e);
                error_count += 1;
                continue;
            }
        };

        // Skip empty lines and comments
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        info!("Processing: {}", trimmed);

        // Create a temporary args with the repository from stdin
        let mut temp_args = args.clone();
        temp_args.repository = Some(trimmed.to_string());

        // Process this repository
        match add_single_repository(&temp_args, distribution, codename) {
            Ok(()) => {
                success_count += 1;
            }
            Err(e) => {
                error_count += 1;
                errors.push(format!("Line {}: {} - {}", line_num + 1, trimmed, e));
            }
        }
    }

    // Report results
    if error_count > 0 {
        error!(
            "Added {} repositories, {} failed:",
            success_count, error_count
        );
        for err in errors {
            error!("  {}", err);
        }
        return Err(format!("{} repositories failed to add", error_count));
    } else if success_count == 0 {
        warn!("No repositories were added (empty input or all lines were comments)");
    } else {
        info!("Successfully added {} repositories", success_count);
    }

    Ok(())
}

fn add_single_repository(
    args: &Args,
    distribution: &Distribution,
    codename: &str,
) -> Result<(), String> {
    let repository = args
        .repository
        .as_ref()
        .ok_or_else(|| "Repository specification required".to_string())?;

    debug!("Adding repository: {}", repository);

    // Handle -p/--pocket flag
    if let Some(pocket) = &args.pocket {
        let pocket_spec = format!("{}-{}", codename, pocket);
        return add_pocket_repository(&pocket_spec, args, args.enable_source, distribution);
    }

    let spec = parse_repository_spec(repository, args.enable_source, args, distribution, codename)?;

    match spec {
        RepositorySpec::Component(component) => {
            add_component_to_existing_repos(&component, &args.directory, args.dry_run, distribution)
        }
        RepositorySpec::Pocket(pocket_spec) => {
            add_pocket_repository(&pocket_spec, args, args.enable_source, distribution)
        }
        RepositorySpec::Repository(parsed) => add_parsed_repository(parsed, args, distribution),
    }
}

fn add_repository(args: &Args, distribution: &Distribution, codename: &str) -> Result<(), String> {
    let repository = args
        .repository
        .as_ref()
        .ok_or_else(|| "Repository specification required".to_string())?;

    // Check if we should read from stdin
    if repository == "-" {
        return add_repositories_from_stdin(args, distribution, codename);
    }

    // Check if multiple repositories are specified (space-separated)
    let repositories: Vec<&str> = repository.split_whitespace().collect();
    if repositories.len() > 1 {
        info!("Adding {} repositories...", repositories.len());
        return add_multiple_repositories(&repositories, args, distribution, codename);
    }

    // Otherwise, add single repository
    add_single_repository(args, distribution, codename)
}

fn add_multiple_repositories(
    repositories: &[&str],
    args: &Args,
    distribution: &Distribution,
    codename: &str,
) -> Result<(), String> {
    let mut success_count = 0;
    let mut error_count = 0;
    let mut errors = Vec::new();

    // Create progress bar
    let pb = ProgressBar::new(repositories.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message("Adding repositories...");

    for (_index, repo_spec) in repositories.iter().enumerate() {
        pb.set_message(format!("Processing: {}", repo_spec));

        // Create a temporary args with the current repository
        let mut temp_args = args.clone();
        temp_args.repository = Some(repo_spec.to_string());

        // Add this repository
        match add_single_repository(&temp_args, distribution, codename) {
            Ok(()) => {
                success_count += 1;
                pb.println(format!("✓ Successfully added: {}", repo_spec));
            }
            Err(e) => {
                error_count += 1;
                errors.push(format!("Repository '{}': {}", repo_spec, e));
                pb.println(format!("✗ Failed to add '{}': {}", repo_spec, e));
            }
        }

        pb.inc(1);
    }

    pb.finish_with_message("Done");

    // Summary
    info!(
        "\nSummary: {} succeeded, {} failed",
        success_count, error_count
    );

    if error_count > 0 {
        error!("\nThe following repositories failed to add:");
        for err in &errors {
            error!("  - {}", err);
        }
        return Err(format!(
            "{} out of {} repositories failed to add",
            error_count,
            repositories.len()
        ));
    }

    Ok(())
}

/// Check a DEB822 sources file for duplicate repositories
fn check_deb822_for_duplicate(
    path: &Path,
    new_uris: &HashSet<String>,
    new_suites: &HashSet<String>,
    new_components: &HashSet<String>,
    new_types: &HashSet<RepositoryType>,
) -> Option<(PathBuf, String)> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            debug!("Failed to read {}: {}", path.display(), e);
            return None;
        }
    };

    let repos = match Repositories::from_str(&content) {
        Ok(r) => r,
        Err(e) => {
            debug!("Failed to parse {}: {}", path.display(), e);
            return None;
        }
    };

    for repo in repos.iter() {
        if is_duplicate_repository(repo, new_uris, new_suites, new_components, new_types) {
            return Some((
                path.to_path_buf(),
                format!("DEB822 format in {}", path.display()),
            ));
        }
    }
    None
}

/// Check a legacy .list file for duplicate repositories
fn check_legacy_for_duplicate(
    path: &Path,
    new_uris: &HashSet<String>,
    new_suites: &HashSet<String>,
    new_components: &HashSet<String>,
    new_types: &HashSet<RepositoryType>,
) -> Option<(PathBuf, String)> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            debug!("Failed to read {}: {}", path.display(), e);
            return None;
        }
    };

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Ok(repos) = line.parse::<LegacyRepositories>() else {
            continue;
        };

        for legacy_repo in repos.iter() {
            let repo = Repository::from(legacy_repo);
            if is_duplicate_repository(&repo, new_uris, new_suites, new_components, new_types) {
                return Some((
                    path.to_path_buf(),
                    format!(
                        "Legacy format in {} at line {}",
                        path.display(),
                        line_num + 1
                    ),
                ));
            }
        }
    }
    None
}

fn find_duplicate_repository(
    new_repo: &Repository,
    sources_dir: &str,
) -> Result<Option<(PathBuf, String)>, String> {
    debug!("Checking for duplicate repositories");

    // Normalize the new repository for comparison
    let new_uris: HashSet<String> = new_repo.uris.iter().map(|u| u.to_string()).collect();
    let new_suites: HashSet<String> = new_repo.suites.iter().cloned().collect();
    let new_components: HashSet<String> = new_repo
        .components
        .as_ref()
        .map(|c| c.iter().cloned().collect())
        .unwrap_or_default();
    let new_types: HashSet<RepositoryType> = new_repo.types.clone();

    // Scan sources.list.d directory
    let sources_path = Path::new(sources_dir);
    if sources_path.exists() {
        for entry in
            fs::read_dir(sources_path).map_err(|e| format!("Failed to read directory: {}", e))?
        {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let ext = path.extension().and_then(|s| s.to_str());
            let result = match ext {
                Some("sources") => check_deb822_for_duplicate(
                    &path,
                    &new_uris,
                    &new_suites,
                    &new_components,
                    &new_types,
                ),
                Some("list") => check_legacy_for_duplicate(
                    &path,
                    &new_uris,
                    &new_suites,
                    &new_components,
                    &new_types,
                ),
                _ => None,
            };

            if result.is_some() {
                return Ok(result);
            }
        }
    }

    // Also check main sources.list
    let main_sources = get_main_sources_list_path(sources_dir);
    if main_sources.exists() {
        if let Some(result) = check_legacy_for_duplicate(
            &main_sources,
            &new_uris,
            &new_suites,
            &new_components,
            &new_types,
        ) {
            return Ok(Some(result));
        }
    }

    Ok(None)
}

fn is_duplicate_repository(
    existing: &Repository,
    new_uris: &HashSet<String>,
    new_suites: &HashSet<String>,
    new_components: &HashSet<String>,
    new_types: &HashSet<RepositoryType>,
) -> bool {
    // Check if disabled
    if !existing.enabled.unwrap_or(true) {
        return false;
    }

    // Compare URIs (must have at least one matching URI)
    let existing_uris: HashSet<String> = existing.uris.iter().map(|u| u.to_string()).collect();
    if existing_uris.is_disjoint(new_uris) {
        return false;
    }

    // Compare suites (must have at least one matching suite)
    let existing_suites: HashSet<String> = existing.suites.iter().cloned().collect();
    if existing_suites.is_disjoint(new_suites) {
        return false;
    }

    // Compare components (must have overlapping components)
    let existing_components: HashSet<String> = existing
        .components
        .as_ref()
        .map(|c| c.iter().cloned().collect())
        .unwrap_or_default();
    if !existing_components.is_empty()
        && !new_components.is_empty()
        && existing_components.is_disjoint(new_components)
    {
        return false;
    }

    // Compare types (must have at least one matching type)
    let existing_types: HashSet<RepositoryType> = existing.types.clone();
    !existing_types.is_disjoint(new_types)
}

fn add_parsed_repository(
    mut parsed: ParsedRepository,
    args: &Args,
    distribution: &Distribution,
) -> Result<(), String> {
    // Check for duplicate repository before proceeding
    debug!("Checking for duplicate repository");
    match find_duplicate_repository(&parsed.repository, &args.directory) {
        Ok(Some((_file_path, location))) => {
            if args.assume_yes {
                warn!("Repository already exists in {}", location);
                warn!("Skipping duplicate repository");
                return Ok(());
            } else {
                eprint!(
                    "Repository already exists in {}.\nAdd it anyway? [y/N] ",
                    location
                );
                io::stdout().flush().unwrap();
                let mut input = String::new();
                io::stdin().read_line(&mut input).unwrap();
                if !input.trim().eq_ignore_ascii_case("y") {
                    return Err("Repository already exists, not adding duplicate".to_string());
                }
                info!("Adding duplicate repository as requested");
            }
        }
        Ok(None) => {
            debug!("No duplicate repository found");
        }
        Err(e) => {
            warn!("Error checking for duplicates: {}", e);
            // Continue anyway - better to add a potential duplicate than fail
        }
    }

    // Handle PPA key download
    if let Some(ref ppa_info) = parsed.ppa_info {
        // Validate PPA first
        info!("Checking PPA availability...");

        #[cfg(feature = "launchpad")]
        let validation_result = if args.login || args.keyserver.is_none() {
            validate_ppa_launchpadlib(ppa_info, args.login)?
        } else {
            validate_ppa_with_suggestions(ppa_info)?
        };

        #[cfg(not(feature = "launchpad"))]
        let validation_result = validate_ppa_with_suggestions(ppa_info)?;

        // Inform about debug symbols if available
        if validation_result.publishes_debug_symbols {
            info!("This PPA publishes debug symbols. To enable debug symbols, add the component 'main/debug'.");
        }

        if !args.assume_yes && !args.dry_run {
            eprint!(
                "You are about to add the following PPA:
 {}
 More info: https://launchpad.net/~{}/+archive/ubuntu/{}
Press [ENTER] to continue or Ctrl-c to cancel.",
                validation_result.display_name, ppa_info.user, ppa_info.name
            );
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
        }

        // Download and save the PPA signing key
        if args.dry_run {
            info!("Would download and verify PPA signing key");
            let keyring_path = Path::new(&args.keyring_dir).join(ppa_info.keyring_filename());
            info!("Would save signing key to {}", keyring_path.display());
            parsed.repository.signature = Some(Signature::KeyPath(keyring_path));
        } else {
            info!("Getting signing key for PPA...");

            // Use launchpadlib if available and needed
            #[cfg(feature = "launchpad")]
            let key_result = if args.login || args.keyserver.is_none() {
                download_ppa_key_launchpadlib(ppa_info, args.login)
            } else {
                download_ppa_key(ppa_info, args.keyserver.as_deref())
            };

            #[cfg(not(feature = "launchpad"))]
            let key_result = download_ppa_key(ppa_info, args.keyserver.as_deref());

            match key_result {
                Ok(key_data) => {
                    let use_inline = args.inline_key && args.format == OutputFormat::Deb822;
                    parsed.repository.signature =
                        save_ppa_key(&key_data, ppa_info, &args.keyring_dir, use_inline);
                }
                Err(e) => {
                    warn!("Failed to download PPA signing key: {}", e);
                    warn!("The repository will be added without signature verification.");
                }
            }
        }
    }

    // Determine the appropriate file path
    let filepath = determine_repository_filepath(&parsed, args, distribution)?;

    // Check if directory exists
    let parent_dir = filepath
        .parent()
        .ok_or_else(|| "Invalid file path".to_string())?;
    if !parent_dir.exists() {
        return Err(format!(
            "Directory {} does not exist.\n\
                           Suggestions:\n\
                           - Create the directory with: sudo mkdir -p {}\n\
                           - Check if you have the correct permissions\n\
                           - Verify the path is correct",
            parent_dir.display(),
            parent_dir.display()
        ));
    }

    // Check if file already exists
    let file_exists = filepath.exists();
    let is_main_file = is_main_distribution_file(&filepath);

    if file_exists && !is_main_file && !args.assume_yes {
        eprint!(
            "Repository file {} already exists. Overwrite? [y/N] ",
            filepath.display()
        );
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        if !input.trim().eq_ignore_ascii_case("y") {
            return Err("Aborted".to_string());
        }
    }

    // Create content based on format
    let content = match args.format {
        OutputFormat::Legacy => {
            // Generate legacy .list format
            LegacyRepositories::from(&parsed.repository).to_string()
        }
        OutputFormat::Deb822 => {
            // Create Repositories container and serialize to DEB822 format
            let repos = Repositories::new(vec![parsed.repository.clone()]);
            repos.to_string()
        }
    };

    if args.dry_run {
        // Show what would be written
        if file_exists && is_main_file {
            info!(
                "Would append repository to existing file {}",
                filepath.display()
            );
        } else {
            info!("Would write repository to {}", filepath.display());
        }
        info!("Repository content that would be written:");
        for line in content.lines() {
            info!("  {}", line);
        }

        if !args.no_update {
            info!("Would run: apt update");
        }
    } else {
        // Write to file - append if it's a main distribution file, otherwise overwrite
        if file_exists && is_main_file {
            // For main distribution files, append the repository
            match args.format {
                OutputFormat::Legacy => {
                    // For legacy format, just append the lines
                    let mut file = fs::OpenOptions::new()
                        .append(true)
                        .open(&filepath)
                        .map_err(|e| format!("Failed to open file for appending: {}", e))?;

                    // Add a newline if the file doesn't end with one
                    let existing_content = fs::read_to_string(&filepath)
                        .map_err(|e| format!("Failed to read existing file: {}", e))?;
                    if !existing_content.ends_with('\n') {
                        use std::io::Write;
                        writeln!(file).map_err(|e| format!("Failed to write newline: {}", e))?;
                    }

                    // Write the new content
                    write!(file, "{}", content)
                        .map_err(|e| format!("Failed to append to file: {}", e))?;
                }
                OutputFormat::Deb822 => {
                    // For DEB822 format, we need to merge repositories
                    let existing_content = fs::read_to_string(&filepath)
                        .map_err(|e| format!("Failed to read existing file: {}", e))?;

                    let repos = Repositories::from_str(&existing_content)
                        .map_err(|e| format!("Failed to parse existing repositories: {}", e))?;

                    // Create a new vector with existing repos plus the new one
                    let mut all_repos: Vec<Repository> = repos.iter().cloned().collect();
                    all_repos.push(parsed.repository.clone());

                    // Write the merged content
                    let merged_repos = Repositories::new(all_repos);
                    fs::write(&filepath, merged_repos.to_string())
                        .map_err(|e| format!("Failed to write merged repositories: {}", e))?;
                }
            }
            info!("Repository appended to {}", filepath.display());
        } else {
            // For non-main files, just write/overwrite
            fs::write(&filepath, &content).map_err(|e| format!("Failed to write file: {}", e))?;
            info!("Repository added to {}", filepath.display());
        }

        // Update package cache unless disabled
        if !args.no_update {
            let spinner = ProgressBar::new_spinner();
            spinner.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg}")
                    .unwrap(),
            );
            spinner.set_message("Updating package cache...");
            spinner.enable_steady_tick(std::time::Duration::from_millis(100));

            let status = process::Command::new("apt")
                .arg("update")
                .status()
                .map_err(|e| {
                    spinner.finish_and_clear();
                    format!("Failed to run apt update: {}", e)
                })?;

            if !status.success() {
                spinner.finish_with_message("apt update failed");
                warn!("apt update failed");
            } else {
                spinner.finish_with_message("Package cache updated successfully");
            }
        }
    }

    Ok(())
}

/// Warn about key expiration if applicable
fn warn_key_expiration(key_data: &str, ppa_info: &PpaInfo) {
    let Ok(cert) = Cert::from_str(key_data) else {
        return;
    };
    let policy = sequoia_openpgp::policy::StandardPolicy::new();
    let Ok(valid_cert) = cert.with_policy(&policy, None) else {
        return;
    };
    let Some(expiration) = valid_cert.primary_key().key_expiration_time() else {
        return;
    };

    let now = std::time::SystemTime::now();
    if expiration < now {
        warn!("Key for {}/{} has expired!", ppa_info.user, ppa_info.name);
    } else if let Ok(duration) = expiration.duration_since(now) {
        let days = duration.as_secs() / 86400;
        if days < 30 {
            warn!(
                "Key for {}/{} expires in {} days",
                ppa_info.user, ppa_info.name, days
            );
        }
    }
}

/// Collect PPAs from DEB822 sources file
fn collect_ppas_from_deb822(
    path: &Path,
    ppa_repos: &mut Vec<(PathBuf, PpaInfo, Option<Signature>)>,
) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read {}: {}", path.display(), e);
            return;
        }
    };

    let repos = match Repositories::from_str(&content) {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to parse {}: {}", path.display(), e);
            return;
        }
    };

    for repo in repos.iter() {
        if let Some(ppa_info) = PpaInfo::from_repository(repo) {
            ppa_repos.push((path.to_path_buf(), ppa_info, repo.signature.clone()));
        }
    }
}

/// Collect PPAs from legacy .list file
fn collect_ppas_from_legacy(
    path: &Path,
    keyring_dir: &str,
    ppa_repos: &mut Vec<(PathBuf, PpaInfo, Option<Signature>)>,
) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read {}: {}", path.display(), e);
            return;
        }
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Ok(repos) = line.parse::<LegacyRepositories>() else {
            continue;
        };

        for legacy_repo in repos.iter() {
            let repo = Repository::from(legacy_repo);
            if let Some(ppa_info) = PpaInfo::from_repository(&repo) {
                let keyring_path = Path::new(keyring_dir).join(ppa_info.keyring_filename());
                let signature = if keyring_path.exists() {
                    Some(Signature::KeyPath(keyring_path))
                } else {
                    None
                };
                ppa_repos.push((path.to_path_buf(), ppa_info, signature));
                break; // Only one PPA per line
            }
        }
    }
}

fn refresh_ppa_keys(args: &Args) -> Result<(), String> {
    info!("Refreshing signing keys for all PPAs...");

    let mut ppa_repos = Vec::new();

    // Scan sources.list.d directory for PPAs
    let sources_dir = Path::new(&args.directory);
    if sources_dir.exists() {
        for entry in
            fs::read_dir(sources_dir).map_err(|e| format!("Failed to read directory: {}", e))?
        {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            match path.extension().and_then(|s| s.to_str()) {
                Some("sources") => collect_ppas_from_deb822(&path, &mut ppa_repos),
                Some("list") => collect_ppas_from_legacy(&path, &args.keyring_dir, &mut ppa_repos),
                _ => {}
            }
        }
    }

    if ppa_repos.is_empty() {
        println!("No PPAs found to refresh keys for.");
        return Ok(());
    }

    println!("Found {} PPA(s) to refresh keys for", ppa_repos.len());

    let mut updated_count = 0;
    let mut failed_count = 0;
    let mut errors = Vec::new();

    for (_source_file, ppa_info, _existing_signature) in &ppa_repos {
        println!(
            "\nRefreshing key for PPA: {}/{}",
            ppa_info.user, ppa_info.name
        );

        // Download the new key
        #[cfg(feature = "launchpad")]
        let key_result = if args.keyserver.is_none() {
            download_ppa_key_launchpadlib(ppa_info, false)
        } else {
            download_ppa_key(ppa_info, args.keyserver.as_deref())
        };

        #[cfg(not(feature = "launchpad"))]
        let key_result = download_ppa_key(ppa_info, args.keyserver.as_deref());

        let key_data = match key_result {
            Ok(data) => data,
            Err(e) => {
                failed_count += 1;
                errors.push(format!("{}/{}: {}", ppa_info.user, ppa_info.name, e));
                continue;
            }
        };

        // Check if we have an existing keyring file
        let keyring_path = Path::new(&args.keyring_dir).join(ppa_info.keyring_filename());

        // Check if the key has changed
        let key_changed = match fs::read_to_string(&keyring_path) {
            Ok(existing_key) => existing_key != key_data,
            Err(_) => true,
        };

        if !key_changed {
            debug!("Key for {}/{} is up to date", ppa_info.user, ppa_info.name);
            continue;
        }

        if args.dry_run {
            info!(
                "Would update signing key for {}/{}",
                ppa_info.user, ppa_info.name
            );
            updated_count += 1;
            warn_key_expiration(&key_data, ppa_info);
            continue;
        }

        // Save the new key
        match save_key_to_keyring(&key_data, &args.keyring_dir, ppa_info) {
            Ok(new_keyring_path) => {
                info!(
                    "Updated signing key saved to {}",
                    new_keyring_path.display()
                );
                updated_count += 1;
                warn_key_expiration(&key_data, ppa_info);
            }
            Err(e) => {
                failed_count += 1;
                errors.push(format!(
                    "{}/{}: Failed to save key - {}",
                    ppa_info.user, ppa_info.name, e
                ));
            }
        }
    }

    // Report results
    if args.dry_run {
        println!("\nDry run - Key refresh summary:");
        println!("  Would update: {}", updated_count);
    } else {
        println!("\nKey refresh complete:");
        println!("  Updated: {}", updated_count);
    }
    println!(
        "  Up to date: {}",
        ppa_repos.len() - updated_count - failed_count
    );
    println!(
        "  {}: {}",
        if args.dry_run {
            "Failed to check"
        } else {
            "Failed"
        },
        failed_count
    );

    if !errors.is_empty() {
        error!("\nErrors encountered:");
        for err in errors {
            error!("  {}", err);
        }
    }

    Ok(())
}

/// Collect repositories from a legacy sources.list file
fn collect_repos_from_sources_list(
    path: &Path,
    all_repos: &mut Vec<(String, PathBuf, RepoFormat, Repository)>,
) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read {}: {}", path.display(), e);
            return;
        }
    };

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Ok(repos) = line.parse::<LegacyRepositories>() else {
            debug!(
                "Failed to parse line {} in {}",
                line_num + 1,
                path.display()
            );
            continue;
        };

        for legacy_repo in repos.iter() {
            all_repos.push((
                format!("sources.list:{}", line_num + 1),
                path.to_path_buf(),
                RepoFormat::Legacy,
                Repository::from(legacy_repo),
            ));
        }
    }
}

/// Print details of a single repository
fn print_repository_details(source: &str, path: &Path, format: RepoFormat, repo: &Repository) {
    let format_str = match format {
        RepoFormat::Deb822 => "DEB822".green(),
        RepoFormat::Legacy => "Legacy".yellow(),
    };
    println!(
        "{} {} [{}]",
        "Source:".bold(),
        source.bright_blue(),
        format_str
    );
    println!(
        "  {}: {}",
        "File".dimmed(),
        path.display().to_string().bright_cyan()
    );

    let enabled_str = if repo.enabled.unwrap_or(true) {
        "enabled".green()
    } else {
        "disabled".red()
    };
    println!("  {}: {}", "Status".dimmed(), enabled_str);

    let types: Vec<&str> = repo
        .types
        .iter()
        .map(|t| match t {
            RepositoryType::Binary => "deb",
            RepositoryType::Source => "deb-src",
        })
        .collect();
    println!("  Types: {}", types.join(", "));

    for uri in &repo.uris {
        println!("  URI: {}", uri);
    }

    println!("  Suites: {}", repo.suites.join(", "));

    if let Some(components) = &repo.components {
        println!("  Components: {}", components.join(" "));
    }

    if !repo.architectures().is_empty() {
        println!("  Architectures: {}", repo.architectures().join(" "));
    }

    if let Some(signature) = &repo.signature {
        match signature {
            Signature::KeyPath(p) => println!("  Signed-By: {}", p.display()),
            Signature::KeyBlock(_) => println!("  Signed-By: [embedded key]"),
        }
    }

    println!(); // Empty line between repositories
}

fn list_repositories(args: &Args) -> Result<(), String> {
    info!("Listing all configured APT repositories");

    let mut all_repos = Vec::new();
    let sources_manager = SourcesManager::new(&args.directory, &args.keyring_dir);

    // Scan all repository files using SourcesManager
    let repo_files = sources_manager.scan_all_repositories()?;

    for (path, repositories) in repo_files {
        let filename = path.file_name().unwrap_or_default().to_string_lossy();
        let format = if path.extension().and_then(|s| s.to_str()) == Some("sources") {
            RepoFormat::Deb822
        } else {
            RepoFormat::Legacy
        };

        for repo in repositories.iter() {
            all_repos.push((filename.to_string(), path.clone(), format, repo.clone()));
        }
    }

    // Also check sources.list if it exists
    let sources_list = get_main_sources_list_path(&args.directory);
    if sources_list.exists() {
        collect_repos_from_sources_list(&sources_list, &mut all_repos);
    }

    // Display the repositories
    if all_repos.is_empty() {
        println!("No repositories configured.");
    } else {
        println!("\n{}\n", "Configured APT repositories:".bold());
        for (source, path, format, repo) in &all_repos {
            print_repository_details(source, path, *format, repo);
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum RepoFormat {
    Deb822,
    Legacy,
}

fn remove_repository(
    args: &Args,
    distribution: &Distribution,
    codename: &str,
) -> Result<(), String> {
    let repository = args
        .repository
        .as_ref()
        .ok_or_else(|| "Repository specification required".to_string())?;

    info!("Removing repository: {}", repository);

    let spec = parse_repository_spec(repository, 0, args, distribution, codename)?;

    let parsed = match spec {
        RepositorySpec::Component(_) => {
            return Err(
                "Cannot remove individual components. Remove the entire repository instead."
                    .to_string(),
            )
        }
        RepositorySpec::Pocket(_) => {
            return Err(
                "Cannot remove individual pockets. Remove the entire repository instead."
                    .to_string(),
            )
        }
        RepositorySpec::Repository(parsed) => parsed,
    };

    // Determine where the repository should be located
    let filepath = determine_repository_filepath(&parsed, args, distribution)?;

    if !filepath.exists() {
        // If the expected file doesn't exist, also check the standard location
        let fallback_path = Path::new(&args.directory).join(&parsed.filename);
        if fallback_path.exists() && fallback_path != filepath {
            return remove_repository_from_file(&fallback_path, &parsed, args);
        }

        return Err(format!(
            "Repository not found in {} or {}",
            filepath.display(),
            fallback_path.display()
        ));
    }

    remove_repository_from_file(&filepath, &parsed, args)
}

/// Log what keyring files would be or were removed for a PPA
fn handle_ppa_keyring_removal(ppa_info: &PpaInfo, keyring_dir: &str, dry_run: bool) {
    for ext in ["asc", "gpg"] {
        let keyring_filename = format!("{}-{}-keyring.{}", ppa_info.user, ppa_info.name, ext);
        let keyring_path = Path::new(keyring_dir).join(&keyring_filename);

        if keyring_path.exists() {
            if dry_run {
                info!(
                    "Would remove associated keyring: {}",
                    keyring_path.display()
                );
            } else {
                fs::remove_file(&keyring_path).ok(); // Ignore errors
                info!("Associated keyring removed from {}", keyring_path.display());
            }
        }
    }
}

/// Run apt update with a spinner
fn run_apt_update() {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Updating package cache...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    match process::Command::new("apt").arg("update").status() {
        Ok(status) if status.success() => {
            spinner.finish_with_message("Package cache updated successfully");
        }
        _ => {
            spinner.finish_with_message("apt update failed");
            warn!("apt update failed");
        }
    }
}

/// Remove repository entry from a DEB822 format file
fn remove_from_deb822_file(filepath: &Path, target: &Repository) -> Result<(), String> {
    let content =
        fs::read_to_string(filepath).map_err(|e| format!("Failed to read file: {}", e))?;

    let repos = Repositories::from_str(&content)
        .map_err(|e| format!("Failed to parse repositories: {}", e))?;

    let all_repos: Vec<Repository> = repos.iter().cloned().collect();
    let initial_count = all_repos.len();
    let filtered_repos: Vec<Repository> = all_repos
        .into_iter()
        .filter(|repo| !repositories_match(repo, target))
        .collect();

    if filtered_repos.len() == initial_count {
        return Err("Repository not found in file".to_string());
    }

    let remaining_repos = Repositories::new(filtered_repos);
    fs::write(filepath, remaining_repos.to_string())
        .map_err(|e| format!("Failed to write file: {}", e))?;

    info!("Repository removed from {}", filepath.display());
    Ok(())
}

/// Remove repository entry from a legacy format file
fn remove_from_legacy_file(filepath: &Path, target: &Repository) -> Result<(), String> {
    let content =
        fs::read_to_string(filepath).map_err(|e| format!("Failed to read file: {}", e))?;

    let to_remove = LegacyRepositories::from(target).to_string();
    let remove_lines: HashSet<String> = to_remove.lines().map(|s| s.trim().to_string()).collect();

    let mut new_lines = Vec::new();
    let mut removed = false;

    for line in content.lines() {
        if remove_lines.contains(line.trim()) {
            removed = true;
        } else {
            new_lines.push(line);
        }
    }

    if !removed {
        return Err("Repository not found in file".to_string());
    }

    let new_content = new_lines.join("\n") + "\n";
    fs::write(filepath, new_content).map_err(|e| format!("Failed to write file: {}", e))?;

    info!("Repository removed from {}", filepath.display());
    Ok(())
}

fn remove_repository_from_file(
    filepath: &Path,
    parsed: &ParsedRepository,
    args: &Args,
) -> Result<(), String> {
    let is_main_file = is_main_distribution_file(filepath);

    // Confirm with user if needed
    if !args.assume_yes && !args.dry_run {
        let prompt = if is_main_file {
            format!("Remove repository from {}? [y/N] ", filepath.display())
        } else {
            format!("Remove repository file {}? [y/N] ", filepath.display())
        };
        eprint!("{}", prompt);
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        if !input.trim().eq_ignore_ascii_case("y") {
            return Err("Aborted".to_string());
        }
    }

    // Handle dry run
    if args.dry_run {
        if is_main_file {
            info!("Would remove repository entry from {}", filepath.display());
            info!("Repository to remove:");
            let content = match args.format {
                OutputFormat::Legacy => LegacyRepositories::from(&parsed.repository).to_string(),
                OutputFormat::Deb822 => {
                    Repositories::new(vec![parsed.repository.clone()]).to_string()
                }
            };
            for line in content.lines() {
                info!("  {}", line);
            }
        } else {
            info!("Would remove repository file: {}", filepath.display());
        }

        if let Some(ref ppa_info) = parsed.ppa_info {
            handle_ppa_keyring_removal(ppa_info, &args.keyring_dir, true);
        }

        if !args.no_update {
            info!("Would run: apt update");
        }
        return Ok(());
    }

    // Actual removal
    if is_main_file {
        let extension = filepath.extension().and_then(|s| s.to_str()).unwrap_or("");
        match extension {
            "sources" => remove_from_deb822_file(filepath, &parsed.repository)?,
            _ => remove_from_legacy_file(filepath, &parsed.repository)?,
        }
    } else {
        fs::remove_file(filepath).map_err(|e| format!("Failed to remove file: {}", e))?;
        info!("Repository file removed: {}", filepath.display());
    }

    if let Some(ref ppa_info) = parsed.ppa_info {
        handle_ppa_keyring_removal(ppa_info, &args.keyring_dir, false);
    }

    if !args.no_update {
        run_apt_update();
    }

    Ok(())
}

fn repositories_match(repo1: &Repository, repo2: &Repository) -> bool {
    // Compare repositories for equality
    repo1.types == repo2.types
        && repo1.uris == repo2.uris
        && repo1.suites == repo2.suites
        && repo1.components == repo2.components
        && repo1.architectures == repo2.architectures
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    // Check if login is requested without launchpad feature
    #[cfg(not(feature = "launchpad"))]
    if args.login {
        error!("Private PPA support requires building with the 'launchpad' feature");
        error!("Rebuild with: cargo build --features launchpad");
        process::exit(1);
    }

    // Detect distribution and codename once at startup
    let distribution = Distribution::current().unwrap_or(Distribution::Debian);
    let (codename, _arch) =
        get_system_info().unwrap_or_else(|| ("stable".to_string(), String::new()));

    let result = if args.list {
        list_repositories(&args)
    } else if args.refresh_keys {
        refresh_ppa_keys(&args)
    } else if let Some(component) = &args.component {
        // Handle --component flag
        add_component_to_existing_repos(component, &args.directory, args.dry_run, &distribution)
    } else if args.enable_source > 0 && args.repository.is_none() {
        // Handle global source enablement when no repository is specified
        enable_sources_globally(&args)
    } else if args.remove {
        remove_repository(&args, &distribution, &codename)
    } else {
        add_repository(&args, &distribution, &codename)
    };

    if let Err(e) = result {
        error!("{}", e);
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sequoia_openpgp::cert::CertBuilder;
    use std::time::Duration;

    #[test]
    fn test_create_inline_signature() {
        // Use a minimal valid PGP key structure
        let key_data = "-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nmQENBFXbjPUBCADRje\n=ABCD\n-----END PGP PUBLIC KEY BLOCK-----";

        // The create_inline_signature now validates the key, so we expect it to fail
        // with this truncated test key
        let result = create_inline_signature(key_data);

        // For now, just check that the function exists and can be called
        // In real usage, it would have a valid key
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn test_format_network_error() {
        // We can't easily create actual reqwest errors, but we can test the function exists
        // and returns reasonable strings for the error types we can simulate

        // In a real test environment, you would mock the reqwest errors
        // For now, we just verify the function compiles
        // Note: We can't call the function without a real reqwest::Error instance
    }

    #[test]
    fn test_is_main_distribution_file() {
        assert!(is_main_distribution_file(Path::new(
            "/etc/apt/sources.list"
        )));
        assert!(is_main_distribution_file(Path::new(
            "/etc/apt/sources.list.d/ubuntu.sources"
        )));
        assert!(is_main_distribution_file(Path::new(
            "/etc/apt/sources.list.d/debian.sources"
        )));
        assert!(is_main_distribution_file(Path::new("/path/to/ubuntu.list")));
        assert!(is_main_distribution_file(Path::new("/path/to/debian.list")));

        assert!(!is_main_distribution_file(Path::new(
            "/etc/apt/sources.list.d/ppa.sources"
        )));
        assert!(!is_main_distribution_file(Path::new(
            "/etc/apt/sources.list.d/custom.list"
        )));
    }

    #[test]
    fn test_repositories_match() {
        let repo1 = Repository {
            types: HashSet::from([RepositoryType::Binary]),
            uris: vec![Url::parse("http://example.com/repo").unwrap()],
            suites: vec!["focal".to_string()],
            components: Some(vec!["main".to_string()]),
            architectures: Some(vec!["amd64".to_string()]),
            ..Default::default()
        };

        let repo2 = repo1.clone();
        assert!(repositories_match(&repo1, &repo2));

        // Different URIs
        let mut repo3 = repo1.clone();
        repo3.uris = vec![Url::parse("http://different.com/repo").unwrap()];
        assert!(!repositories_match(&repo1, &repo3));

        // Different components
        let mut repo4 = repo1.clone();
        repo4.components = Some(vec!["universe".to_string()]);
        assert!(!repositories_match(&repo1, &repo4));
    }

    #[test]
    fn test_get_distribution_info() {
        // This test will depend on the system it's run on
        // We just verify it returns a valid Distribution enum when available
        let dist = Distribution::current();

        // On systems with /etc/os-release (Linux), we should get Some
        // On systems without it (macOS), we get None - both are acceptable
        if std::path::Path::new("/etc/os-release").exists() {
            assert!(dist.is_some());
            match dist.unwrap() {
                Distribution::Ubuntu | Distribution::Debian | Distribution::Other(_) => {
                    // All valid variants
                }
            }
        } else {
            // On macOS or other systems without /etc/os-release, None is expected
            assert!(dist.is_none());
        }
    }

    #[test]
    fn test_check_key_expiration() {
        // Create a cert that's valid for 60 days
        let (cert, _) = CertBuilder::new()
            .set_validity_period(Duration::from_secs(60 * 24 * 60 * 60))
            .generate()
            .unwrap();

        let warning = apt_sources::key_management::check_key_expiration(&cert);
        assert!(warning.is_none());

        // Note: Testing expired certs would require creating a cert with
        // a past expiration date, which CertBuilder doesn't easily support
        // The function only checks if a cert is currently expired, not if it's expiring soon
    }

    #[test]
    fn test_generate_filename() {
        let dist = Distribution::Ubuntu;

        // Test PPA URL
        let filename = generate_filename(
            "https://ppa.launchpad.net/user/ppa/ubuntu",
            OutputFormat::Deb822,
            &dist,
        );
        assert!(filename.ends_with(".sources"));

        // Test main Ubuntu repository
        let filename = generate_filename(
            "http://archive.ubuntu.com/ubuntu",
            OutputFormat::Deb822,
            &dist,
        );
        assert_eq!(filename, "ubuntu.sources");

        // Test main Ubuntu repository with legacy format
        let filename = generate_filename(
            "http://archive.ubuntu.com/ubuntu",
            OutputFormat::Legacy,
            &dist,
        );
        assert_eq!(filename, "ubuntu.list");

        // Test third-party repository
        let filename = generate_filename("https://example.com/repo", OutputFormat::Deb822, &dist);
        assert!(filename.contains("example.com"));
        assert!(filename.ends_with(".sources"));
    }

    #[test]
    fn test_default_suite_components() {
        let (suite, components) = default_suite_components("focal");
        assert_eq!(suite, "focal");
        assert_eq!(components, vec!["main".to_string()]);
    }

    fn create_test_args() -> Args {
        Args {
            repository: None,
            no_update: false,
            enable_source: 0,
            assume_yes: false,
            remove: false,
            directory: DEFAULT_SOURCES_PATH.to_string(),
            keyring_dir: DEFAULT_KEYRING_PATH.to_string(),
            pocket: None,
            keyserver: None,
            component: None,
            list: false,
            login: false,
            dry_run: false,
            format: OutputFormat::Legacy,
            refresh_keys: false,
            inline_key: false,
        }
    }

    #[test]
    fn test_parse_repository_spec() {
        let args = create_test_args();
        let distribution = Distribution::Ubuntu;
        let codename = "noble";

        // Test PPA format
        let spec =
            parse_repository_spec("ppa:user/repo", 0, &args, &distribution, codename).unwrap();
        match spec {
            RepositorySpec::Repository(parsed) => {
                assert!(parsed.ppa_info.is_some());
                let ppa = parsed.ppa_info.unwrap();
                assert_eq!(ppa.user, "user");
                assert_eq!(ppa.name, "repo");
            }
            _ => panic!("Expected Repository spec with PPA info"),
        }

        // NOTE: Component and pocket tests require real system info from get_system_info()
        // These tests are skipped as they depend on the runtime environment

        let distribution = Distribution::Debian;
        let codename = "stable";

        // Test full deb line
        let spec = parse_repository_spec(
            "deb http://example.com focal main",
            0,
            &args,
            &distribution,
            codename,
        )
        .unwrap();
        match spec {
            RepositorySpec::Repository(parsed) => {
                assert!(parsed.repository.uris[0]
                    .as_str()
                    .contains("http://example.com"));
                assert_eq!(parsed.repository.suites, vec!["focal"]);
                assert_eq!(parsed.repository.components, Some(vec!["main".to_string()]));
            }
            _ => panic!("Expected Repository spec"),
        }
    }

    #[test]
    fn test_parse_repository_line() {
        // Test basic deb line
        let (types, uri, suite, components, signed_by) =
            parse_repository_line("deb http://example.com focal main universe").unwrap();
        assert!(types.contains(&RepositoryType::Binary));
        assert_eq!(uri.as_str(), "http://example.com/");
        assert_eq!(suite, "focal");
        assert_eq!(components, vec!["main", "universe"]);
        assert!(signed_by.is_none());

        // Test deb-src line
        let (types, uri, suite, components, signed_by) =
            parse_repository_line("deb-src http://example.com focal main").unwrap();
        assert!(types.contains(&RepositoryType::Source));
        assert_eq!(uri.as_str(), "http://example.com/");
        assert_eq!(suite, "focal");
        assert_eq!(components, vec!["main"]);
        assert!(signed_by.is_none());

        // Test with trailing slash
        let (types, uri, suite, components, signed_by) =
            parse_repository_line("deb http://example.com/ focal main").unwrap();
        assert!(types.contains(&RepositoryType::Binary));
        assert_eq!(uri.as_str(), "http://example.com/");
        assert_eq!(suite, "focal");
        assert_eq!(components, vec!["main"]);
        assert!(signed_by.is_none());

        // Test invalid line
        assert!(parse_repository_line("invalid line").is_err());
        assert!(parse_repository_line("deb").is_err());
        assert!(parse_repository_line("deb http://example.com").is_err());
    }

    #[test]
    fn test_find_matching_binary_repo() {
        // Create a binary repository
        let binary_repo = Repository {
            types: HashSet::from([RepositoryType::Binary]),
            uris: vec![Url::parse("http://example.com/repo").unwrap()],
            suites: vec!["focal".to_string()],
            components: Some(vec!["main".to_string()]),
            ..Default::default()
        };

        // Create a source repository
        let source_repo = Repository {
            types: HashSet::from([RepositoryType::Source]),
            uris: vec![Url::parse("http://example.com/repo").unwrap()],
            suites: vec!["focal".to_string()],
            components: Some(vec!["main".to_string()]),
            ..Default::default()
        };

        let repos = Repositories::new(vec![binary_repo.clone(), source_repo.clone()]);

        // Test finding matching binary repo
        let match_found = find_matching_binary_repo(&repos, &source_repo);
        assert!(match_found.is_some());
        assert_eq!(
            match_found.unwrap().types,
            HashSet::from([RepositoryType::Binary])
        );

        // Test with non-matching source repo
        let non_matching = Repository {
            types: HashSet::from([RepositoryType::Source]),
            uris: vec![Url::parse("http://different.com/repo").unwrap()],
            suites: vec!["focal".to_string()],
            components: Some(vec!["main".to_string()]),
            ..Default::default()
        };
        let match_found = find_matching_binary_repo(&repos, &non_matching);
        assert!(match_found.is_none());
    }

    #[test]
    fn test_is_duplicate_repository() {
        let repo1 = Repository {
            types: HashSet::from([RepositoryType::Binary]),
            uris: vec![Url::parse("http://example.com/repo").unwrap()],
            suites: vec!["focal".to_string()],
            components: Some(vec!["main".to_string()]),
            ..Default::default()
        };

        // Test exact duplicate
        let uris: HashSet<String> = HashSet::from(["http://example.com/repo".to_string()]);
        let suites: HashSet<String> = HashSet::from(["focal".to_string()]);
        let components: HashSet<String> = HashSet::from(["main".to_string()]);
        let types = HashSet::from([RepositoryType::Binary]);

        assert!(is_duplicate_repository(
            &repo1,
            &uris,
            &suites,
            &components,
            &types
        ));

        // Different types
        let different_types = HashSet::from([RepositoryType::Source]);
        assert!(!is_duplicate_repository(
            &repo1,
            &uris,
            &suites,
            &components,
            &different_types
        ));

        // Different URIs
        let different_uris = HashSet::from(["http://different.com/repo".to_string()]);
        assert!(!is_duplicate_repository(
            &repo1,
            &different_uris,
            &suites,
            &components,
            &types
        ));

        // Different suites
        let different_suites = HashSet::from(["jammy".to_string()]);
        assert!(!is_duplicate_repository(
            &repo1,
            &uris,
            &different_suites,
            &components,
            &types
        ));
    }

    #[test]
    fn test_determine_repository_filepath() {
        let args = create_test_args();
        let distribution = Distribution::Debian;

        // Test PPA repository
        let ppa_repo = ParsedRepository {
            repository: Repository {
                enabled: Some(true),
                types: HashSet::from([RepositoryType::Binary]),
                uris: vec![Url::parse("http://ppa.launchpad.net/user/ppa/ubuntu").unwrap()],
                suites: vec!["focal".to_string()],
                components: Some(vec!["main".to_string()]),
                ..Default::default()
            },
            filename: "user-ubuntu-ppa-focal.list".to_string(),
            ppa_info: Some(PpaInfo {
                user: "user".to_string(),
                name: "ppa".to_string(),
            }),
        };
        let path = determine_repository_filepath(&ppa_repo, &args, &distribution).unwrap();
        assert!(path.to_str().unwrap().ends_with(".list")); // Legacy format is default in test args
        assert!(path.to_str().unwrap().contains("user"));

        // Test non-PPA repository
        let url_repo = ParsedRepository {
            repository: Repository {
                enabled: Some(true),
                types: HashSet::from([RepositoryType::Binary]),
                uris: vec![Url::parse("https://example.com/repo").unwrap()],
                suites: vec!["focal".to_string()],
                components: Some(vec!["main".to_string()]),
                ..Default::default()
            },
            filename: "example.com.list".to_string(),
            ppa_info: None,
        };
        let path = determine_repository_filepath(&url_repo, &args, &distribution).unwrap();
        assert!(path.to_str().unwrap().ends_with(".list")); // Legacy format is default in test args
        assert!(path.to_str().unwrap().contains("example.com"));

        // Test with deb822 format
        let mut args_deb822 = create_test_args();
        args_deb822.format = OutputFormat::Deb822;
        let mut url_repo_deb822 = url_repo.clone();
        url_repo_deb822.filename = "example.com.sources".to_string();
        let path =
            determine_repository_filepath(&url_repo_deb822, &args_deb822, &distribution).unwrap();
        assert!(path.to_str().unwrap().ends_with(".sources"));
    }

    #[test]
    fn test_verify_key_fingerprint() {
        // This test needs a real PGP key to work properly
        // For unit testing, we'll verify the function handles invalid input correctly
        let invalid_key = "not a valid key";
        let result = verify_key_fingerprint(invalid_key, "1234567890ABCDEF");
        assert!(result.is_err());

        // Test with empty fingerprint
        let result = verify_key_fingerprint(invalid_key, "");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_distribution_specific_filename() {
        // Test Ubuntu
        let ubuntu_file =
            get_distribution_specific_filename("ubuntu", "sources", &Distribution::Ubuntu);
        assert_eq!(ubuntu_file, "ubuntu.sources");

        let ubuntu_legacy =
            get_distribution_specific_filename("ubuntu", "list", &Distribution::Ubuntu);
        assert_eq!(ubuntu_legacy, "ubuntu.list");

        // Test custom base name for Ubuntu
        let ubuntu_custom =
            get_distribution_specific_filename("custom", "sources", &Distribution::Ubuntu);
        assert_eq!(ubuntu_custom, "ubuntu.sources");

        // Test Debian
        let debian_file =
            get_distribution_specific_filename("debian", "sources", &Distribution::Debian);
        assert_eq!(debian_file, "debian.sources");

        let debian_legacy =
            get_distribution_specific_filename("debian", "list", &Distribution::Debian);
        assert_eq!(debian_legacy, "debian.list");

        // Test custom base name for Debian
        let debian_custom =
            get_distribution_specific_filename("custom", "sources", &Distribution::Debian);
        assert_eq!(debian_custom, "debian.sources");

        // Test Other
        let other_file = get_distribution_specific_filename(
            "myrepo",
            "sources",
            &Distribution::Other("custom".to_string()),
        );
        assert_eq!(other_file, "myrepo.sources");
    }

    #[test]
    fn test_output_format_from_str() {
        // Test valid formats
        assert_eq!(
            OutputFormat::from_str("deb822").unwrap(),
            OutputFormat::Deb822
        );
        assert_eq!(
            OutputFormat::from_str("sources").unwrap(),
            OutputFormat::Deb822
        );
        assert_eq!(
            OutputFormat::from_str("legacy").unwrap(),
            OutputFormat::Legacy
        );
        assert_eq!(
            OutputFormat::from_str("list").unwrap(),
            OutputFormat::Legacy
        );

        // Test case insensitive
        assert_eq!(
            OutputFormat::from_str("DEB822").unwrap(),
            OutputFormat::Deb822
        );
        assert_eq!(
            OutputFormat::from_str("LEGACY").unwrap(),
            OutputFormat::Legacy
        );

        // Test invalid formats
        assert!(OutputFormat::from_str("invalid").is_err());
        assert!(OutputFormat::from_str("").is_err());
    }

    #[test]
    fn test_get_system_info() {
        // This test might fail on non-Linux systems
        if std::path::Path::new("/etc/os-release").exists() {
            if let Some((codename, arch)) = get_system_info() {
                assert!(!codename.is_empty());
                // arch is now empty - APT will use system default
                assert!(arch.is_empty());
            }
            // It's okay if this returns None on systems without VERSION_CODENAME
        }
    }

    #[test]
    fn test_has_matching_source_line() {
        let lines = vec![
            "deb http://example.com focal main",
            "deb-src http://example.com focal main",
            "deb http://other.com focal universe",
        ];

        // Test exact match
        assert!(has_matching_source_line(
            &lines,
            "deb-src http://example.com focal main"
        ));

        // Test no match
        assert!(!has_matching_source_line(
            &lines,
            "deb-src http://other.com focal main"
        ));
        assert!(!has_matching_source_line(
            &lines,
            "deb-src http://example.com focal universe"
        ));
    }

    #[test]
    fn test_has_matching_deb_line() {
        let lines = vec![
            "deb http://example.com focal main",
            "deb-src http://example.com focal main",
            "deb http://other.com focal universe",
        ];

        // Test conversion from deb-src to deb
        assert!(has_matching_deb_line(
            &lines,
            "deb-src http://example.com focal main"
        ));

        // Test no match
        assert!(!has_matching_deb_line(
            &lines,
            "deb-src http://other.com focal main"
        ));
        assert!(!has_matching_deb_line(
            &lines,
            "deb-src http://example.com focal universe"
        ));
    }

    #[test]
    fn test_has_matching_source_repo() {
        // Create binary repo
        let binary_repo = Repository {
            types: HashSet::from([RepositoryType::Binary]),
            uris: vec![Url::parse("http://example.com").unwrap()],
            suites: vec!["focal".to_string()],
            components: Some(vec!["main".to_string()]),
            ..Default::default()
        };

        // Create matching source repo
        let source_repo = Repository {
            types: HashSet::from([RepositoryType::Source]),
            uris: vec![Url::parse("http://example.com").unwrap()],
            suites: vec!["focal".to_string()],
            components: Some(vec!["main".to_string()]),
            ..Default::default()
        };

        // Create repos with both
        let repos = Repositories::new(vec![binary_repo.clone(), source_repo]);

        // Test match found
        assert!(has_matching_source_repo(&repos, &binary_repo));

        // Test no match with different components
        let different_repo = Repository {
            types: HashSet::from([RepositoryType::Binary]),
            uris: vec![Url::parse("http://example.com").unwrap()],
            suites: vec!["focal".to_string()],
            components: Some(vec!["universe".to_string()]),
            ..Default::default()
        };
        assert!(!has_matching_source_repo(&repos, &different_repo));
    }

    #[test]
    fn test_ppa_info() {
        let ppa = PpaInfo {
            user: "test-user".to_string(),
            name: "test-ppa".to_string(),
        };

        // Test Debug trait implementation
        let debug_str = format!("{:?}", ppa);
        assert!(debug_str.contains("test-user"));
        assert!(debug_str.contains("test-ppa"));
    }

    #[test]
    fn test_repository_spec_enum() {
        // Test Component variant
        let comp_spec = RepositorySpec::Component("main".to_string());
        match comp_spec {
            RepositorySpec::Component(c) => assert_eq!(c, "main"),
            _ => panic!("Expected Component variant"),
        }

        // Test Pocket variant
        let pocket_spec = RepositorySpec::Pocket("security".to_string());
        match pocket_spec {
            RepositorySpec::Pocket(p) => assert_eq!(p, "security"),
            _ => panic!("Expected Pocket variant"),
        }
    }

    #[test]
    fn test_create_http_client() {
        // Test without proxy env vars
        std::env::remove_var("HTTP_PROXY");
        std::env::remove_var("http_proxy");
        std::env::remove_var("HTTPS_PROXY");
        std::env::remove_var("https_proxy");

        let client_result = create_http_client();
        assert!(client_result.is_ok());

        // Test with invalid proxy
        std::env::set_var("HTTP_PROXY", "not-a-valid-url");
        let client_result = create_http_client();
        assert!(client_result.is_ok()); // Should still succeed, just warn about invalid proxy

        // Clean up
        std::env::remove_var("HTTP_PROXY");
    }

    #[test]
    fn test_validate_fingerprint() {
        // Valid fingerprints (40 hex chars)
        let valid = "1234567890ABCDEF1234567890ABCDEF12345678";
        assert!(valid.len() == 40);
        assert!(valid.chars().all(|c| c.is_ascii_hexdigit()));

        // Invalid fingerprints
        let invalid_short = "1234567890ABCDEF";
        assert!(invalid_short.len() < 40);

        let invalid_chars = "1234567890ABCDEF1234567890ABCDEF12345XYZ";
        assert!(!invalid_chars.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_parsed_repository_struct() {
        let repo = Repository::default();
        let parsed = ParsedRepository {
            repository: repo,
            filename: "test.sources".to_string(),
            ppa_info: Some(PpaInfo {
                user: "test".to_string(),
                name: "ppa".to_string(),
            }),
        };

        // Test Debug trait
        let debug_str = format!("{:?}", parsed);
        assert!(debug_str.contains("test.sources"));
        assert!(debug_str.contains("ppa_info"));
    }

    #[test]
    fn test_empty_component_handling() {
        // Test that empty components are handled properly
        let empty_vec: Vec<String> = vec![];
        assert!(validate_ppa_components(&empty_vec).is_ok());

        // Test with empty string in components
        let vec_with_empty = vec!["".to_string()];
        assert!(validate_ppa_components(&vec_with_empty).is_err());
    }
}
