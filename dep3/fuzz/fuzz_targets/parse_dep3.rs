#![no_main]

use dep3::lossy::PatchHeader as LossyPatchHeader;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

#[cfg(feature = "lossless")]
use dep3::lossless::PatchHeader as LosslessPatchHeader;

fuzz_target!(|s: &str| {
    // Fuzz lossy patch header parser
    let _ = LossyPatchHeader::from_str(s);

    #[cfg(feature = "lossless")]
    {
        // Fuzz lossless patch header parser
        let _ = LosslessPatchHeader::from_str(s);
    }
});
