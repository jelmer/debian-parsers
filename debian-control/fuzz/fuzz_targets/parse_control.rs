#![no_main]

use debian_control::lossy::apt::{Package, Source};
use debian_control::lossy::Control;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    // Fuzz main control file parser
    let _ = Control::from_str(s);

    // Fuzz apt package parser
    let _ = Package::from_str(s);

    // Fuzz apt source parser
    let _ = Source::from_str(s);
});
