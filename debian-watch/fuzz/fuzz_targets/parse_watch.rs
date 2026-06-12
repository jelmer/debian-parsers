#![no_main]

use debian_watch::linebased::WatchFile;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|s: &str| {
    // Relaxed parsing never fails; it collects errors instead.
    let watch = WatchFile::from_str_relaxed(s);
    let _ = watch.version();
    for entry in watch.entries() {
        let _ = entry.url();
        let _ = entry.matching_pattern();
        let _ = entry.version();
        let _ = entry.component();
        let _ = entry.opts();
    }
    // The lossless tree must reproduce the input exactly.
    assert_eq!(watch.to_string(), s);
});
