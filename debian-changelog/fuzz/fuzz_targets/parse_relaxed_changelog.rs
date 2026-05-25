#![no_main]

use debian_changelog::ChangeLog;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|s: &str| {
    // Relaxed parsing never fails; it collects errors instead.
    let changelog = ChangeLog::parse_relaxed(s);
    for entry in changelog.iter() {
        let _ = entry.package();
        let _ = entry.version();
        let _ = entry.change_lines().count();
    }
    // Even with relaxed parsing the source text is preserved.
    assert_eq!(changelog.to_string(), s);
});
