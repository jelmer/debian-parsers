#![no_main]

use deb822_edit::Deb822;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    if let Ok(deb822) = Deb822::from_str(s) {
        // Walk the AST: each accessor on each paragraph should not panic.
        for paragraph in deb822.paragraphs() {
            for key in paragraph.keys() {
                let _ = paragraph.get(&key);
            }
            let _ = paragraph.to_string();
        }
        // The lossless tree must reproduce the input exactly.
        assert_eq!(deb822.to_string(), s);
    }
});
