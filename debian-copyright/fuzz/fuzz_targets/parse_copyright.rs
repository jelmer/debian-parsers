#![no_main]

use debian_copyright::lossless::Copyright as LosslessCopyright;
use debian_copyright::lossy::Copyright as LossyCopyright;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    // Fuzz lossy copyright parser
    let _ = LossyCopyright::from_str(s);

    // Fuzz lossless copyright parser
    let _ = LosslessCopyright::from_str(s);
});
