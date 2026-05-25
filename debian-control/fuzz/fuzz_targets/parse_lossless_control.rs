#![no_main]

use libfuzzer_sys::fuzz_target;

#[cfg(feature = "lossless")]
use debian_control::lossless::apt::{Package, Release, Source};
#[cfg(feature = "lossless")]
use debian_control::lossless::control::Control;
#[cfg(feature = "lossless")]
use std::str::FromStr;

fuzz_target!(|s: &str| {
    #[cfg(feature = "lossless")]
    {
        // Fuzz lossless control file parser
        let _ = Control::from_str(s);

        // Fuzz lossless apt package parser
        let _ = Package::from_str(s);

        // Fuzz lossless apt source parser
        let _ = Source::from_str(s);

        // Fuzz lossless release parser
        let _ = Release::from_str(s);
    }
    // Avoid unused-variable warnings when the lossless feature is off.
    let _ = s;
});
