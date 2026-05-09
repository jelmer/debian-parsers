use debian_changelog::ChangeLog;

#[test]
fn test_entry_builder_verify_missing_package() {
    let mut cl = ChangeLog::new();
    let builder = cl
        .new_empty_entry()
        .version("1.0.0".parse().unwrap())
        .distributions(vec!["unstable".to_string()]);

    // Missing package - verify should fail
    let result = builder.verify();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "package is required");
}

#[test]
fn test_entry_builder_verify_missing_version() {
    let mut cl = ChangeLog::new();
    let builder = cl
        .new_empty_entry()
        .package("test".to_string())
        .distributions(vec!["unstable".to_string()]);

    // Missing version - verify should fail
    let result = builder.verify();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "version is required");
}

#[test]
fn test_entry_builder_verify_missing_distributions() {
    let mut cl = ChangeLog::new();
    let builder = cl
        .new_empty_entry()
        .package("test".to_string())
        .version("1.0.0".parse().unwrap());

    // Missing distributions - verify should fail
    let result = builder.verify();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "at least one distribution is required");
}

#[test]
fn test_entry_builder_verify_empty_distributions() {
    let mut cl = ChangeLog::new();
    let builder = cl
        .new_empty_entry()
        .package("test".to_string())
        .version("1.0.0".parse().unwrap())
        .distributions(vec![]);

    // Empty distributions - verify should fail
    let result = builder.verify();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "at least one distribution is required");
}

#[test]
fn test_entry_builder_verify_missing_change_lines() {
    let mut cl = ChangeLog::new();
    let builder = cl
        .new_empty_entry()
        .package("test".to_string())
        .version("1.0.0".parse().unwrap())
        .distributions(vec!["unstable".to_string()]);

    // Missing change lines - verify should fail
    let result = builder.verify();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "at least one change line is required");
}

#[test]
fn test_entry_builder_verify_success() {
    let mut cl = ChangeLog::new();
    let builder = cl
        .new_empty_entry()
        .package("test".to_string())
        .version("1.0.0".parse().unwrap())
        .distributions(vec!["unstable".to_string()])
        .change_line("* Initial release.".to_string());

    // All required fields present - verify should succeed
    let result = builder.verify();
    assert!(result.is_ok());
}
