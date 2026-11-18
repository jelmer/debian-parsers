//! Conversion between Deb822-like paragraphs and Rust objects.

/// Abstract trait for accessing and modifying key-value pairs in a paragraph.
pub trait Deb822LikeParagraph: FromIterator<(String, String)> {
    /// Get the value for the given key.
    fn get(&self, key: &str) -> Option<String>;

    /// Insert a key-value pair.
    fn set(&mut self, key: &str, value: &str);

    /// Remove a key-value pair.
    fn remove(&mut self, key: &str);
}

impl Deb822LikeParagraph for crate::Paragraph {
    fn get(&self, key: &str) -> Option<String> {
        crate::Paragraph::get(self, key).map(|v| v.to_string())
    }

    fn set(&mut self, key: &str, value: &str) {
        crate::Paragraph::set(self, key, value);
    }

    fn remove(&mut self, key: &str) {
        crate::Paragraph::remove(self, key);
    }
}

/// Convert a paragraph to this object.
pub trait FromDeb822Paragraph<P: Deb822LikeParagraph> {
    /// Convert a paragraph to this object.
    fn from_paragraph(paragraph: &P) -> Result<Self, String>
    where
        Self: Sized;
}

/// Convert this object to a paragraph.
pub trait ToDeb822Paragraph<P: Deb822LikeParagraph> {
    /// Convert this object to a paragraph.
    fn to_paragraph(&self) -> P;

    /// Update the given paragraph with the values from this object.
    fn update_paragraph(&self, paragraph: &mut P);
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "derive")]
    use super::*;

    #[cfg(feature = "derive")]
    mod derive {
        use super::*;
        use crate as deb822_fast;
        use crate::{FromDeb822, ToDeb822};

        #[test]
        fn test_derive() {
            #[derive(ToDeb822)]
            struct Foo {
                bar: String,
                baz: i32,
                blah: Option<String>,
            }

            let foo = Foo {
                bar: "hello".to_string(),
                baz: 42,
                blah: None,
            };

            let paragraph: crate::Paragraph = foo.to_paragraph();
            assert_eq!(paragraph.get("bar"), Some("hello"));
            assert_eq!(paragraph.get("baz"), Some("42"));
            assert_eq!(paragraph.get("blah"), None);
        }

        #[test]
        fn test_optional_missing() {
            #[derive(ToDeb822)]
            struct Foo {
                bar: String,
                baz: Option<String>,
            }

            let foo = Foo {
                bar: "hello".to_string(),
                baz: None,
            };

            let paragraph: crate::Paragraph = foo.to_paragraph();
            assert_eq!(paragraph.get("bar"), Some("hello"));
            assert_eq!(paragraph.get("baz"), None);

            assert_eq!("bar: hello\n", paragraph.to_string());
        }

        #[test]
        fn test_deserialize_with() {
            let mut para: crate::Paragraph =
                "bar: bar\n# comment\nbaz: blah\n".parse().unwrap();

            fn to_bool(s: &str) -> Result<bool, String> {
                Ok(s == "ja")
            }

            fn from_bool(s: &bool) -> String {
                if *s {
                    "ja".to_string()
                } else {
                    "nee".to_string()
                }
            }

            #[derive(FromDeb822, ToDeb822)]
            struct Foo {
                bar: String,
                #[deb822(deserialize_with = to_bool, serialize_with = from_bool)]
                baz: bool,
            }

            let mut foo: Foo = Foo::from_paragraph(&para).unwrap();
            assert_eq!(foo.bar, "bar");
            assert!(!foo.baz);

            foo.bar = "new".to_string();

            foo.update_paragraph(&mut para);

            assert_eq!(para.get("bar"), Some("new"));
            assert_eq!(para.get("baz"), Some("nee"));
            assert_eq!(para.to_string(), "bar: new\nbaz: nee\n");
        }

        #[test]
        fn test_update_remove() {
            let mut para: crate::Paragraph =
                "bar: bar\n# comment\nbaz: blah\n".parse().unwrap();

            #[derive(FromDeb822, ToDeb822)]
            struct Foo {
                bar: Option<String>,
                baz: String,
            }

            let mut foo: Foo = Foo::from_paragraph(&para).unwrap();
            assert_eq!(foo.bar, Some("bar".to_string()));
            assert_eq!(foo.baz, "blah");

            foo.bar = None;

            foo.update_paragraph(&mut para);

            assert_eq!(para.get("bar"), None);
            assert_eq!(para.get("baz"), Some("blah"));
            assert_eq!(para.to_string(), "baz: blah\n");
        }
    }
}
