//! A simple example of making a change to a changelog file

use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open("/usr/share/doc/rustc/changelog.Debian.gz")?;
    let mut gz = flate2::read::GzDecoder::new(file);
    let mut contents = String::new();
    gz.read_to_string(&mut contents)?;
    let mut changelog: debian_changelog::ChangeLog = contents.parse()?;
    changelog.try_auto_add_change(
        &["* Make a change"],
        (
            "Jelmer Vernooĳ".to_string(),
            "jelmer@debian.org".to_string(),
        ),
        None::<String>,
        None,
    )?;
    changelog.write(std::io::stdout())?;
    Ok(())
}
