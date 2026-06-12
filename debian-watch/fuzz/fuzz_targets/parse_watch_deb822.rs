#![no_main]

use debian_watch::deb822::WatchFile;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    if let Ok(watch) = WatchFile::from_str(s) {
        let _ = watch.version();
        for entry in watch.entries() {
            let _ = entry.url();
            let _ = entry.matching_pattern();
            let _ = entry.component();
            let _ = entry.version_policy();
        }
    }
});
