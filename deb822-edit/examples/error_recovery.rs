//! Example demonstrating error recovery in the deb822-edit parser.
//!
//! This example shows how the parser can handle malformed deb822 files gracefully,
//! continuing to parse valid content even when errors are encountered.

use deb822_edit::Deb822;
use std::collections::HashMap;

fn main() {
    // Example 1: Missing colons
    println!("=== Example 1: Missing Colons ===");
    let malformed_input1 = r#"Source: test-package
Maintainer John Doe <john@example.com>
Section: utils
Version: 1.0.0

Package: test-package
Architecture: all
Description: A test package
 This is a multi-line description
 that spans multiple lines.
"#;

    demonstrate_error_recovery("Missing colon after Maintainer", malformed_input1);

    // Example 2: Orphaned text and missing field names
    println!("\n=== Example 2: Orphaned Text and Missing Field Names ===");
    let malformed_input2 = r#"Package: my-package
some random text without a field name
: orphaned colon without field name
Version: 2.0.0
Description: Valid description
"#;

    demonstrate_error_recovery("Orphaned text and colon", malformed_input2);

    // Example 3: Consecutive field names (missing values)
    println!("\n=== Example 3: Consecutive Field Names ===");
    let malformed_input3 = r#"Package: another-package
Maintainer
Priority
Section: devel
Description: Another package
Version: 3.0.0
"#;

    demonstrate_error_recovery("Missing values for some fields", malformed_input3);

    // Example 4: Mixed errors across paragraphs
    println!("\n=== Example 4: Mixed Errors Across Paragraphs ===");
    let malformed_input4 = r#"Source broken-source
Maintainer: Valid Maintainer <valid@example.com>

completely broken paragraph here
: orphaned stuff
random text everywhere

Package: recovered-package
Architecture: all
Depends: some-dep
Description: This package was recovered after errors
"#;

    demonstrate_error_recovery("Mixed errors across paragraphs", malformed_input4);

    // Example 5: Malformed continuation lines
    println!("\n=== Example 5: Malformed Continuation Lines ===");
    let malformed_input5 = r#"Package: continuation-test
Description: Short description
  Proper continuation line
invalid continuation without indent
 Another proper continuation
  Last proper continuation
Version: 1.0
"#;

    demonstrate_error_recovery("Malformed continuation lines", malformed_input5);
}

fn demonstrate_error_recovery(title: &str, input: &str) {
    println!("Input ({}):", title);
    println!("{}", input);

    // Parse with error recovery
    let (deb822, errors) = Deb822::from_str_relaxed(input);

    // Show errors found
    if !errors.is_empty() {
        println!("Errors found:");
        for (i, error) in errors.iter().enumerate() {
            println!("  {}: {}", i + 1, error);
        }
    } else {
        println!("No errors found.");
    }

    // Show positioned errors if available
    let parsed = Deb822::parse(input);
    let positioned_errors = parsed.positioned_errors();
    if !positioned_errors.is_empty() {
        println!("\nPositioned errors:");
        for (i, error) in positioned_errors.iter().enumerate() {
            println!(
                "  {}: {} (range: {:?}, code: {:?})",
                i + 1,
                error.message,
                error.range,
                error.code
            );

            // Show the problematic text
            let start = error.range.start().into();
            let end = error.range.end().into();
            if end <= input.len() {
                let error_text = &input[start..end];
                if !error_text.is_empty() {
                    println!("     Problematic text: {:?}", error_text);
                }
            }
        }
    }

    // Show recovered structure
    println!("\nRecovered structure:");
    println!("  Number of paragraphs: {}", deb822.paragraphs().count());

    // Collect all valid fields from all paragraphs
    let mut all_fields: HashMap<String, String> = HashMap::new();
    for (i, paragraph) in deb822.paragraphs().enumerate() {
        let fields: Vec<String> = paragraph.keys().collect();
        if !fields.is_empty() {
            println!("  Paragraph {}: {} fields", i + 1, fields.len());
            for field in &fields {
                println!(
                    "    {}: {:?}",
                    field,
                    paragraph.get(field).unwrap_or_default()
                );
                all_fields.insert(field.clone(), paragraph.get(field).unwrap_or_default());
            }
        } else {
            println!(
                "  Paragraph {}: No valid fields (error recovery paragraph)",
                i + 1
            );
        }
    }

    // Show that we can still work with the recovered data
    println!("\nUsable extracted data:");
    for (key, value) in &all_fields {
        println!("  {}: {}", key, value);
    }

    // Demonstrate that the structure is still lossless - we can reconstruct the original
    println!("\nReconstructed output (preserves original formatting):");
    let reconstructed = deb822.to_string();
    println!("{}", reconstructed);

    println!("{}", "=".repeat(60));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_recovery_examples() {
        // Test that all examples in main() actually work
        let examples = [
            r#"Source: test
Maintainer Bad Format
Section: utils"#,
            r#"Package: test
orphaned text
Version: 1.0"#,
            r#"Package: test
Field1
Field2
Valid: field"#,
        ];

        for (i, example) in examples.iter().enumerate() {
            let (deb822, errors) = Deb822::from_str_relaxed(example);

            // Should have errors but still parse something useful
            assert!(!errors.is_empty(), "Example {} should have errors", i);

            // Should still extract some valid fields
            let mut all_fields = HashMap::new();
            for paragraph in deb822.paragraphs() {
                for (key, value) in paragraph.items() {
                    all_fields.insert(key, value);
                }
            }

            assert!(
                !all_fields.is_empty(),
                "Example {} should extract some valid fields",
                i
            );
        }
    }
}
