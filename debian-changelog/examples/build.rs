//! A simple example of generate a Debian changelog file.

use debian_changelog::{ChangeLog, Urgency};

fn main() {
    let mut changelog = ChangeLog::new();

    // Note that most of these are optional and fall back to sensible defaults.
    changelog
        .new_entry()
        .package("example".to_string())
        .version("0.1.0".parse().unwrap())
        .distribution("unstable".to_string())
        .urgency(Urgency::Low)
        .maintainer(("John Doe".to_string(), "john@example.com".to_string()))
        .datetime(chrono::DateTime::parse_from_rfc3339("2018-01-01T00:00:00+00:00").unwrap())
        .change_line("* This is a change".to_string())
        .finish();

    // You can also use changelog.try_auto_add_change(), which behaves similarly to "dch"

    println!("{}", changelog);
}
