#![no_main]

use libfuzzer_sys::fuzz_target;
use lintian_overrides::LintianOverrides;

fuzz_target!(|s: &str| {
    // Parsing is resilient and always produces a tree.
    let parsed = LintianOverrides::parse(s);
    let overrides = parsed.tree();
    for line in overrides.lines() {
        let _ = line.tag();
        let _ = line.package();
        let _ = line.package_type();
        let _ = line.info();
    }
    // The lossless tree must reproduce the input exactly.
    assert_eq!(overrides.text(), s);
});
