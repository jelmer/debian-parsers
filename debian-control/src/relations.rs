//! Parsing of Debian relations strings.
use std::iter::Peekable;
use std::str::Chars;

/// Build profile for a package.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BuildProfile {
    /// A build profile that is enabled.
    Enabled(String),

    /// A build profile that is disabled.
    Disabled(String),
}

impl std::fmt::Display for BuildProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BuildProfile::Enabled(s) => f.write_str(s),
            BuildProfile::Disabled(s) => write!(f, "!{}", s),
        }
    }
}

impl std::str::FromStr for BuildProfile {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(s) = s.strip_prefix('!') {
            Ok(BuildProfile::Disabled(s.to_string()))
        } else {
            Ok(BuildProfile::Enabled(s.to_string()))
        }
    }
}

/// Constraint on a Debian package version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VersionConstraint {
    /// <<
    LessThan, // <<
    /// <=
    LessThanEqual, // <=
    /// =
    Equal, // =
    /// >>
    GreaterThan, // >>
    /// >=
    GreaterThanEqual, // >=
}

impl std::str::FromStr for VersionConstraint {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            ">=" => Ok(VersionConstraint::GreaterThanEqual),
            "<=" => Ok(VersionConstraint::LessThanEqual),
            "=" => Ok(VersionConstraint::Equal),
            ">>" => Ok(VersionConstraint::GreaterThan),
            "<<" => Ok(VersionConstraint::LessThan),
            _ => Err(format!("Invalid version constraint: {}", s)),
        }
    }
}

impl std::fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            VersionConstraint::GreaterThanEqual => f.write_str(">="),
            VersionConstraint::LessThanEqual => f.write_str("<="),
            VersionConstraint::Equal => f.write_str("="),
            VersionConstraint::GreaterThan => f.write_str(">>"),
            VersionConstraint::LessThan => f.write_str("<<"),
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for VersionConstraint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for VersionConstraint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(all(test, feature = "serde"))]
mod version_constraint_serde_tests {
    use super::VersionConstraint;

    #[test]
    fn test_serialize() {
        assert_eq!(
            serde_json::to_string(&VersionConstraint::GreaterThanEqual).unwrap(),
            "\">=\"",
        );
        assert_eq!(
            serde_json::to_string(&VersionConstraint::LessThanEqual).unwrap(),
            "\"<=\"",
        );
        assert_eq!(
            serde_json::to_string(&VersionConstraint::Equal).unwrap(),
            "\"=\"",
        );
        assert_eq!(
            serde_json::to_string(&VersionConstraint::GreaterThan).unwrap(),
            "\">>\"",
        );
        assert_eq!(
            serde_json::to_string(&VersionConstraint::LessThan).unwrap(),
            "\"<<\"",
        );
    }

    #[test]
    fn test_deserialize() {
        assert_eq!(
            serde_json::from_str::<VersionConstraint>("\">=\"").unwrap(),
            VersionConstraint::GreaterThanEqual,
        );
        assert_eq!(
            serde_json::from_str::<VersionConstraint>("\"<=\"").unwrap(),
            VersionConstraint::LessThanEqual,
        );
        assert_eq!(
            serde_json::from_str::<VersionConstraint>("\"=\"").unwrap(),
            VersionConstraint::Equal,
        );
        assert_eq!(
            serde_json::from_str::<VersionConstraint>("\">>\"").unwrap(),
            VersionConstraint::GreaterThan,
        );
        assert_eq!(
            serde_json::from_str::<VersionConstraint>("\"<<\"").unwrap(),
            VersionConstraint::LessThan,
        );
    }

    #[test]
    fn test_deserialize_invalid() {
        assert!(serde_json::from_str::<VersionConstraint>("\"!!\"").is_err());
    }
}

/// Let's start with defining all kinds of tokens and
/// composite nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
#[repr(u16)]
#[allow(missing_docs)]
pub enum SyntaxKind {
    IDENT = 0, // package name
    COLON,     // :
    PIPE,
    COMMA,      // ,
    L_PARENS,   // (
    R_PARENS,   // )
    L_BRACKET,  // [
    R_BRACKET,  // ]
    NOT,        // !
    L_ANGLE,    // <
    R_ANGLE,    // >
    EQUAL,      // =
    WHITESPACE, // whitespace
    NEWLINE,    // newline
    DOLLAR,     // $
    L_CURLY,
    R_CURLY,
    COMMENT, // comment line starting with #
    ERROR,   // as well as errors

    // composite nodes
    ROOT,       // The entire file
    ENTRY,      // A single entry
    RELATION,   // An alternative in a dependency
    ARCHQUAL,   // An architecture qualifier
    VERSION,    // A version constraint
    CONSTRAINT, // (">=", "<=", "=", ">>", "<<")
    ARCHITECTURES,
    PROFILES,
    SUBSTVAR,
}

/// Convert our `SyntaxKind` into the rowan `SyntaxKind`.
#[cfg(feature = "lossless")]
impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

/// A lexer for relations strings.
pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer for the given input.
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input: input.chars().peekable(),
        }
    }

    fn is_whitespace(c: char) -> bool {
        c == ' ' || c == '\t' || c == '\r'
    }

    fn is_valid_ident_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '+' || c == '~'
    }

    fn read_while<F>(&mut self, predicate: F) -> String
    where
        F: Fn(char) -> bool,
    {
        let mut result = String::new();
        while let Some(&c) = self.input.peek() {
            if predicate(c) {
                result.push(c);
                self.input.next();
            } else {
                break;
            }
        }
        result
    }

    fn next_token(&mut self) -> Option<(SyntaxKind, String)> {
        if let Some(&c) = self.input.peek() {
            match c {
                ':' => {
                    self.input.next();
                    Some((SyntaxKind::COLON, c.to_string()))
                }
                '|' => {
                    self.input.next();
                    Some((SyntaxKind::PIPE, c.to_string()))
                }
                ',' => {
                    self.input.next();
                    Some((SyntaxKind::COMMA, c.to_string()))
                }
                '(' => {
                    self.input.next();
                    Some((SyntaxKind::L_PARENS, c.to_string()))
                }
                ')' => {
                    self.input.next();
                    Some((SyntaxKind::R_PARENS, c.to_string()))
                }
                '[' => {
                    self.input.next();
                    Some((SyntaxKind::L_BRACKET, c.to_string()))
                }
                ']' => {
                    self.input.next();
                    Some((SyntaxKind::R_BRACKET, c.to_string()))
                }
                '!' => {
                    self.input.next();
                    Some((SyntaxKind::NOT, c.to_string()))
                }
                '$' => {
                    self.input.next();
                    Some((SyntaxKind::DOLLAR, c.to_string()))
                }
                '{' => {
                    self.input.next();
                    Some((SyntaxKind::L_CURLY, c.to_string()))
                }
                '}' => {
                    self.input.next();
                    Some((SyntaxKind::R_CURLY, c.to_string()))
                }
                '<' => {
                    self.input.next();
                    Some((SyntaxKind::L_ANGLE, c.to_string()))
                }
                '>' => {
                    self.input.next();
                    Some((SyntaxKind::R_ANGLE, c.to_string()))
                }
                '=' => {
                    self.input.next();
                    Some((SyntaxKind::EQUAL, c.to_string()))
                }
                '\n' => {
                    self.input.next();
                    Some((SyntaxKind::NEWLINE, c.to_string()))
                }
                '#' => {
                    let comment = self.read_while(|c| c != '\n');
                    Some((SyntaxKind::COMMENT, comment))
                }
                _ if Self::is_whitespace(c) => {
                    let whitespace = self.read_while(Self::is_whitespace);
                    Some((SyntaxKind::WHITESPACE, whitespace))
                }
                // TODO: separate handling for package names and versions?
                _ if Self::is_valid_ident_char(c) => {
                    let key = self.read_while(Self::is_valid_ident_char);
                    Some((SyntaxKind::IDENT, key))
                }
                _ => {
                    self.input.next();
                    Some((SyntaxKind::ERROR, c.to_string()))
                }
            }
        } else {
            None
        }
    }
}

impl Iterator for Lexer<'_> {
    type Item = (SyntaxKind, String);

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}

pub(crate) fn lex(input: &str) -> Vec<(SyntaxKind, String)> {
    let mut lexer = Lexer::new(input);
    lexer.by_ref().collect::<Vec<_>>()
}
