#![no_main]

use debian_copyright::lossless::Copyright as LosslessCopyright;
use debian_copyright::lossy::Copyright as LossyCopyright;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    // Lossy parser: just make sure it doesn't panic.
    let _ = LossyCopyright::from_str(s);

    // Lossless parser: walk the AST and assert round-trip exactness.
    if let Ok(copyright) = LosslessCopyright::from_str(s) {
        if let Some(header) = copyright.header() {
            let _ = header.format_string();
        }
        for files in copyright.iter_files() {
            let _ = files.files();
            let _ = files.license();
        }
        for license in copyright.iter_licenses() {
            let _ = license.license();
        }
        assert_eq!(copyright.to_string(), s);
    }
});
