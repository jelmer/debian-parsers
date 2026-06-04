use deb822_edit::TextRange;
use debian_control::lossless::Control;

fn main() {
    // Example control file content
    let control_text = r#"Source: example-package
Maintainer: John Doe <john@example.com>
Build-Depends: debhelper (>= 12)

Package: example-binary
Architecture: any
Depends: ${shlibs:Depends}, ${misc:Depends}
Description: Example package
 This is an example package
 with a multi-line description.
"#;

    // Parse the control file
    let control: Control = control_text.parse().unwrap();

    // Simulate a change in the range where "Architecture: any" is located
    // In a real LSP, this would come from the editor
    let change_start = control_text.find("Architecture:").unwrap();
    let change_end = change_start + "Architecture: any".len();
    let change_range = TextRange::new((change_start as u32).into(), (change_end as u32).into());

    println!("Checking fields in range {}..{}", change_start, change_end);
    println!("Fields affected by the change:");

    // Use the new range-based API to find affected fields
    for entry in control.fields_in_range(change_range) {
        if let Some(key) = entry.key() {
            println!("  - Field: {}", key);
            let value = entry.value();
            println!("    Value: {}", value.trim());
        }
    }

    // Also demonstrate checking if specific paragraphs overlap
    if let Some(source) = control.source() {
        println!(
            "\nSource paragraph overlaps with change: {}",
            source.overlaps_range(change_range)
        );
    }

    for binary in control.binaries() {
        if let Some(name) = binary.name() {
            println!(
                "Binary '{}' overlaps with change: {}",
                name,
                binary.overlaps_range(change_range)
            );
        }
    }
}
