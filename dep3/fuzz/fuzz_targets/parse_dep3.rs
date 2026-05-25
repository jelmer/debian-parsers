#![no_main]

use dep3::lossy::PatchHeader as LossyPatchHeader;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

#[cfg(feature = "lossless")]
use dep3::lossless::PatchHeader as LosslessPatchHeader;

fuzz_target!(|s: &str| {
    // Lossy parser: must not panic.
    let _ = LossyPatchHeader::from_str(s);

    #[cfg(feature = "lossless")]
    {
        // Lossless parser: walk accessors and assert round-trip.
        if let Ok(header) = LosslessPatchHeader::from_str(s) {
            let _ = header.description();
            let _ = header.origin();
            let _ = header.forwarded();
            for bug in header.bugs() {
                let _ = bug;
            }
            assert_eq!(header.to_string(), s);
        }
    }
});
