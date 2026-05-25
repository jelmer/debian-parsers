#![no_main]

use deb822_lossless::Deb822;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    let _ = Deb822::from_str(s);
});
