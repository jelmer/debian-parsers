#![no_main]

use debian_control::lossy::{Relation, Relations};
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    // Fuzz relation parsing
    let _ = Relation::from_str(s);
    let _ = Relations::from_str(s);
});
