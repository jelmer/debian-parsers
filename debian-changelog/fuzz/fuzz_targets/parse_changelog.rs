#![no_main]

use debian_changelog::ChangeLog;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(changelog) = ChangeLog::from_str(s) {
            for entry in changelog.iter() {
                let _ = entry.package();
                let _ = entry.version();
                let _ = entry.distributions();
                let _ = entry.urgency();
                let _ = entry.maintainer();
                let _ = entry.timestamp();
                let _ = entry.change_lines().count();
            }
            // Round-trip should preserve the input exactly.
            assert_eq!(changelog.to_string(), s);
        }
    }
});
