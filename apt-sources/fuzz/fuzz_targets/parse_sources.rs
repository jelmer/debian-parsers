#![no_main]

use apt_sources::{Repositories, RepositoryType};
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    // Fuzz repositories parser
    let _ = Repositories::from_str(s);

    // Fuzz repository type parser
    let _ = RepositoryType::from_str(s);
});
