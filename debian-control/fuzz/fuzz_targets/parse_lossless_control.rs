#![no_main]

use libfuzzer_sys::fuzz_target;

#[cfg(feature = "lossless")]
use debian_control::lossless::apt::{Package, Release, Source as AptSource};
#[cfg(feature = "lossless")]
use debian_control::lossless::control::Control;
#[cfg(feature = "lossless")]
use std::str::FromStr;

fuzz_target!(|s: &str| {
    #[cfg(feature = "lossless")]
    {
        // Walk Control accessors and assert lossless round-trip.
        if let Ok(control) = Control::from_str(s) {
            if let Some(source) = control.source() {
                let _ = source.name();
                let _ = source.maintainer();
                let _ = source.section();
                let _ = source.priority();
                let _ = source.standards_version();
                let _ = source.build_depends();
            }
            for binary in control.binaries() {
                let _ = binary.name();
            }
            assert_eq!(control.to_string(), s);
        }

        // Other lossless parsers; just exercise from_str.
        let _ = Package::from_str(s);
        let _ = AptSource::from_str(s);
        let _ = Release::from_str(s);
    }
    let _ = s;
});
