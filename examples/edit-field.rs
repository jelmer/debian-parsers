fn main() {
    let d: deb822_edit::Deb822 = r#"Source: golang-github-blah-blah
Section: devel
Priority: optional
Standards-Version: 4.2.0
Maintainer: Some Maintainer <example@example.com>
Build-Depends: debhelper (>= 11~),  # comment
               dh-golang,
               golang-any
Homepage: https://github.com/j-keck/arping
"#
    .parse()
    .unwrap();

    let mut ps = d.paragraphs();
    let mut p = ps.next().unwrap();
    assert_eq!(
        "Some Maintainer <example@example.com>",
        p.get("Maintainer").unwrap()
    );
    p.insert("Maintainer", "Some Other Maintainer <blah@example.com>");
    assert_eq!(
        "Some Other Maintainer <blah@example.com>",
        p.get("Maintainer").unwrap()
    );

    assert_eq!(
        d.to_string(),
        r#"Source: golang-github-blah-blah
Section: devel
Priority: optional
Standards-Version: 4.2.0
Maintainer: Some Other Maintainer <blah@example.com>
Build-Depends: debhelper (>= 11~),  # comment
               dh-golang,
               golang-any
Homepage: https://github.com/j-keck/arping
"#
    );
}
