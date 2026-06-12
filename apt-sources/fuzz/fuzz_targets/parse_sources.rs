#![no_main]

use apt_sources::legacy::LegacyRepositories;
use apt_sources::{Repositories, RepositoryType};
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|s: &str| {
    // Deb822-format sources.
    if let Ok(repos) = Repositories::from_str(s) {
        for repo in repos.iter() {
            let _ = &repo.uris;
            let _ = &repo.suites;
            let _ = &repo.components;
            let _ = &repo.architectures;
            let _ = &repo.types;
        }
    }

    // Legacy one-line sources format; not previously fuzzed.
    if let Ok(legacy) = LegacyRepositories::from_str(s) {
        for repo in legacy.iter() {
            let _ = &repo.uri;
            let _ = &repo.suite;
            let _ = &repo.components;
        }
    }

    let _ = RepositoryType::from_str(s);
});
