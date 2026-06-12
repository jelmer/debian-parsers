#![no_main]

use debian_control::lossy::{Relation, Relations};
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    if let Ok(rels) = Relations::from_str(s) {
        // Walk each alternation; touch every relation's public state so
        // accessor methods don't panic on edge-case input.
        for alt in rels.iter() {
            for rel in alt {
                let _ = &rel.name;
                let _ = &rel.version;
                let _ = &rel.profiles;
            }
        }
    }
    let _ = Relation::from_str(s);
});
