use debian_changelog::{ChangeLog, Error};
use std::io;

#[test]
fn test_error_display() {
    // Test IO error display
    let io_error = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let error = Error::Io(io_error);
    let display = format!("{}", error);
    assert!(display.contains("IO error"));
    assert!(display.contains("file not found"));

    // Test Parse error display by triggering a parse error
    let result: Result<ChangeLog, _> = "invalid changelog".parse();
    assert!(result.is_err());
    if let Err(parse_error) = result {
        let error = Error::Parse(parse_error);
        let display = format!("{}", error);
        assert!(display.contains("Parse error"));
    }
}

#[test]
fn test_parse_error_from_invalid_input() {
    // Test that parsing invalid input produces errors with proper display
    let result: Result<ChangeLog, _> = "INVALID".parse();
    assert!(result.is_err());

    if let Err(error) = result {
        let display = format!("{}", error);
        // Should contain some error message
        assert!(!display.is_empty());
    }
}
