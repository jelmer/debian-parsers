//! A small reimplementation of dpkg-mergechangelogs for testing.

use debian_changelog::merge::{merge_changelogs, MergeOptions};
use debian_changelog::ChangeLog;
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut opts = MergeOptions::new();
    let mut files = Vec::new();
    for arg in &args[1..] {
        match arg.as_str() {
            "-m" | "--merge-prereleases" => opts.merge_prereleases = true,
            "--merge-unreleased" => opts.merge_unreleased = true,
            _ => files.push(arg.clone()),
        }
    }
    if files.len() < 3 {
        eprintln!("usage: mergechangelogs [options] <old> <new-a> <new-b>");
        exit(2);
    }
    let read = |p: &str| ChangeLog::parse_relaxed(&std::fs::read_to_string(p).unwrap());
    let old = read(&files[0]);
    let a = read(&files[1]);
    let b = read(&files[2]);
    let result = merge_changelogs(&old, &a, &b, &opts);
    print!("{}", result.changelog);
    exit(if result.conflicts { 1 } else { 0 });
}
