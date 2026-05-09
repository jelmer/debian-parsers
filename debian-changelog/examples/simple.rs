//! A simple example of parsing a Debian changelog.

use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open("/usr/share/doc/rustc/changelog.Debian.gz")?;
    let mut gz = flate2::read::GzDecoder::new(file);
    let mut contents = String::new();
    gz.read_to_string(&mut contents)?;
    let changelog: debian_changelog::ChangeLog = contents.parse()?;
    for entry in changelog.iter() {
        println!("{}: {}", entry.package().unwrap(), entry.version().unwrap());
    }
    Ok(())
}
