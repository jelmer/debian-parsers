#![no_main]

use debian_control::lossy::apt::{Package, Source};
use debian_control::lossy::Control;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    if let Ok(control) = Control::from_str(s) {
        // Touch every field so accessor logic gets exercised.
        let _ = &control.source.name;
        let _ = &control.source.maintainer;
        for binary in &control.binaries {
            let _ = &binary.name;
            let _ = &binary.architecture;
        }
        // Display impl must reproduce the input *as accepted by the parser*;
        // we can't claim equality with `s` since lossy parsing may drop
        // whitespace/comments. Just make sure it doesn't panic.
        let _ = control.to_string();
    }

    let _ = Package::from_str(s);
    let _ = Source::from_str(s);
});
