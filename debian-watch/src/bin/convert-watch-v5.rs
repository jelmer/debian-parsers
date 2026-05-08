//! Command-line tool to convert Debian watch files from formats 1-4 to format 5

use clap::Parser;
use debian_watch::convert_to_v5;
use debian_watch::linebased::WatchFile;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "convert-watch-v5")]
#[command(version, about = "Convert Debian watch files from formats 1-4 to format 5", long_about = None)]
struct Cli {
    /// Input watch file (use '-' for stdin)
    #[arg(value_name = "INPUT")]
    input: String,

    /// Output file (defaults to stdout if not specified)
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Overwrite output file if it exists
    #[arg(short = 'f', long)]
    force: bool,
}

fn main() {
    let cli = Cli::parse();

    // Read input
    let input_content = if cli.input == "-" {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer).unwrap_or_else(|e| {
            eprintln!("Error reading from stdin: {}", e);
            std::process::exit(1);
        });
        buffer
    } else {
        fs::read_to_string(&cli.input).unwrap_or_else(|e| {
            eprintln!("Error reading file '{}': {}", cli.input, e);
            std::process::exit(1);
        })
    };

    // Parse the watch file
    let watch_file: WatchFile = input_content.parse().unwrap_or_else(|e| {
        eprintln!("Error parsing watch file: {}", e);
        std::process::exit(1);
    });

    // Check version
    let version = watch_file.version();
    if version == 5 {
        eprintln!("Warning: Watch file is already version 5, no conversion needed");
        if cli.output.is_none() {
            // Just output the original content
            print!("{}", input_content);
            return;
        }
    }

    // Convert to v5
    let v5_file = convert_to_v5(&watch_file).unwrap_or_else(|e| {
        eprintln!("Error converting watch file: {}", e);
        std::process::exit(1);
    });

    let output_content = v5_file.to_string();

    // Write output
    if let Some(output_path) = &cli.output {
        // Check if file exists and force flag is not set
        if output_path.exists() && !cli.force {
            eprintln!(
                "Error: Output file '{}' already exists. Use -f/--force to overwrite.",
                output_path.display()
            );
            std::process::exit(1);
        }

        fs::write(output_path, &output_content).unwrap_or_else(|e| {
            eprintln!("Error writing to file '{}': {}", output_path.display(), e);
            std::process::exit(1);
        });
        eprintln!(
            "Successfully converted watch file from version {} to version 5: {}",
            version,
            output_path.display()
        );
    } else {
        print!("{}", output_content);
    }
}
