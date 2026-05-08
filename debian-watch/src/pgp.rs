//! PGP signature verification support.

use sequoia_openpgp as openpgp;
use std::io::Read;

/// Error type for PGP operations
#[derive(Debug)]
pub enum PgpError {
    /// Failed to parse signature
    SignatureParseError(String),
    /// Failed to verify signature
    VerificationError(String),
    /// IO error
    IoError(std::io::Error),
    /// Sequoia error
    SequoiaError(String),
}

impl std::fmt::Display for PgpError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PgpError::SignatureParseError(s) => write!(f, "signature parse error: {}", s),
            PgpError::VerificationError(s) => write!(f, "verification error: {}", s),
            PgpError::IoError(e) => write!(f, "IO error: {}", e),
            PgpError::SequoiaError(e) => write!(f, "sequoia error: {}", e),
        }
    }
}

impl std::error::Error for PgpError {}

impl From<std::io::Error> for PgpError {
    fn from(e: std::io::Error) -> Self {
        PgpError::IoError(e)
    }
}

impl From<openpgp::Error> for PgpError {
    fn from(e: openpgp::Error) -> Self {
        PgpError::SequoiaError(e.to_string())
    }
}

impl From<anyhow::Error> for PgpError {
    fn from(e: anyhow::Error) -> Self {
        PgpError::SequoiaError(e.to_string())
    }
}

/// Common signature file extensions to probe
pub const SIGNATURE_EXTENSIONS: &[&str] = &[".asc", ".sig", ".sign", ".gpg"];

/// Result of signature verification
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureVerification {
    /// Whether the signature is cryptographically valid
    pub valid: bool,
    /// The fingerprint of the signing key
    pub fingerprint: Option<String>,
    /// Error message if verification failed
    pub error: Option<String>,
}

impl SignatureVerification {
    /// Create a successful verification result
    pub fn valid(fingerprint: String) -> Self {
        Self {
            valid: true,
            fingerprint: Some(fingerprint),
            error: None,
        }
    }

    /// Create a failed verification result
    pub fn invalid(error: String) -> Self {
        Self {
            valid: false,
            fingerprint: None,
            error: Some(error),
        }
    }
}

/// Generate potential signature URLs from a tarball URL
///
/// Returns a list of URLs that might contain the detached signature,
/// based on common naming conventions.
///
/// # Examples
///
/// ```
/// use debian_watch::pgp::probe_signature_urls;
///
/// let tarball_url = "https://example.com/project-1.0.tar.gz";
/// let sig_urls = probe_signature_urls(tarball_url);
/// assert_eq!(sig_urls, vec![
///     "https://example.com/project-1.0.tar.gz.asc",
///     "https://example.com/project-1.0.tar.gz.sig",
///     "https://example.com/project-1.0.tar.gz.sign",
///     "https://example.com/project-1.0.tar.gz.gpg",
/// ]);
/// ```
pub fn probe_signature_urls(url: &str) -> Vec<String> {
    SIGNATURE_EXTENSIONS
        .iter()
        .map(|ext| format!("{}{}", url, ext))
        .collect()
}

/// Verify a detached PGP signature and extract the key fingerprint
///
/// Verifies that the signature correctly signs the data using the provided certificate.
/// This performs cryptographic verification but does NOT verify certificate trust or validity.
/// The caller is responsible for trust decisions.
///
/// # Arguments
///
/// * `signature` - The detached signature data (e.g., .asc file contents)
/// * `data` - The data that was signed
/// * `cert` - The PGP certificate containing the public key
///
/// # Returns
///
/// * `Ok(fingerprint)` with the signing key's fingerprint if the signature is cryptographically valid
/// * `Err(PgpError)` if verification fails or parsing errors occur
///
/// # Examples
///
/// ```ignore
/// use debian_watch::pgp::verify_detached;
///
/// let data = b"Hello, world!";
/// let signature = std::fs::read("data.sig")?;
/// let cert = std::fs::read("pubkey.asc")?;
///
/// match verify_detached(&signature[..], &data[..], &cert[..]) {
///     Ok(fingerprint) => println!("Signature valid, key fingerprint: {}", fingerprint),
///     Err(e) => eprintln!("Signature verification failed: {}", e),
/// }
/// ```
pub fn verify_detached<S, D, C>(signature: S, data: D, cert: C) -> Result<String, PgpError>
where
    S: Read + Send + Sync,
    D: Read + Send + Sync,
    C: Read + Send + Sync,
{
    use openpgp::parse::stream::*;
    use openpgp::parse::Parse;
    use openpgp::policy::StandardPolicy;

    let p = &StandardPolicy::new();

    // Parse the certificate
    let cert = openpgp::Cert::from_reader(cert)
        .map_err(|e| PgpError::SignatureParseError(e.to_string()))?;

    // Create a helper that provides public keys for verification
    struct Helper<'a> {
        cert: &'a openpgp::Cert,
        fingerprint: Option<String>,
    }

    impl<'a> VerificationHelper for Helper<'a> {
        fn get_certs(
            &mut self,
            _ids: &[openpgp::KeyHandle],
        ) -> openpgp::Result<Vec<openpgp::Cert>> {
            Ok(vec![self.cert.clone()])
        }

        fn check(&mut self, structure: MessageStructure) -> openpgp::Result<()> {
            // Check that we have at least one valid signature
            let mut valid_signature = false;

            for layer in structure.iter() {
                match layer {
                    MessageLayer::SignatureGroup { results } => {
                        for result in results {
                            match result {
                                Ok(GoodChecksum { ka, .. }) => {
                                    valid_signature = true;
                                    // Extract the fingerprint from the key amalgamation
                                    self.fingerprint = Some(ka.key().fingerprint().to_hex());
                                }
                                Err(e) => {
                                    eprintln!("Signature verification failed: {}", e);
                                }
                            }
                        }
                    }
                    MessageLayer::Compression { .. } => {}
                    MessageLayer::Encryption { .. } => {}
                }
            }

            if valid_signature {
                Ok(())
            } else {
                Err(anyhow::anyhow!("No valid signature found"))
            }
        }
    }

    let helper = Helper {
        cert: &cert,
        fingerprint: None,
    };

    // Create a verifier and verify the data
    let mut verifier =
        DetachedVerifierBuilder::from_reader(signature)?.with_policy(p, None, helper)?;

    // In sequoia v2, we verify by calling verify_reader with the data
    verifier.verify_reader(data)?;

    // Extract the fingerprint from the helper
    let fingerprint = verifier
        .into_helper()
        .fingerprint
        .ok_or_else(|| PgpError::VerificationError("No fingerprint found".to_string()))?;

    Ok(fingerprint)
}

/// Verify a detached signature from byte slices and extract the key fingerprint
///
/// Convenience wrapper around `verify_detached` for in-memory data.
///
/// # Examples
///
/// ```ignore
/// use debian_watch::pgp::verify_detached_bytes;
///
/// let data = b"Hello, world!";
/// let signature = include_bytes!("test.sig");
/// let cert = include_bytes!("test_key.asc");
///
/// let fingerprint = verify_detached_bytes(signature, data, cert)?;
/// println!("Signature valid, key fingerprint: {}", fingerprint);
/// ```
pub fn verify_detached_bytes(
    signature: &[u8],
    data: &[u8],
    cert: &[u8],
) -> Result<String, PgpError> {
    verify_detached(
        std::io::Cursor::new(signature),
        std::io::Cursor::new(data),
        std::io::Cursor::new(cert),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_signature_urls() {
        let url = "https://example.com/project-1.0.tar.gz";
        let sig_urls = probe_signature_urls(url);

        assert_eq!(
            sig_urls,
            vec![
                "https://example.com/project-1.0.tar.gz.asc",
                "https://example.com/project-1.0.tar.gz.sig",
                "https://example.com/project-1.0.tar.gz.sign",
                "https://example.com/project-1.0.tar.gz.gpg",
            ]
        );
    }

    #[test]
    fn test_probe_signature_urls_tar_xz() {
        let url = "https://example.com/release.tar.xz";
        let sig_urls = probe_signature_urls(url);

        assert_eq!(
            sig_urls,
            vec![
                "https://example.com/release.tar.xz.asc",
                "https://example.com/release.tar.xz.sig",
                "https://example.com/release.tar.xz.sign",
                "https://example.com/release.tar.xz.gpg",
            ]
        );
    }

    #[test]
    fn test_signature_extensions_constant() {
        assert_eq!(SIGNATURE_EXTENSIONS, &[".asc", ".sig", ".sign", ".gpg"]);
    }
}
