use crate::SyntaxKind;
use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LineType {
    Header,
    Body,
    Footer,
}

pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
    line_type: Option<LineType>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input: input.chars().peekable(),
            line_type: None,
        }
    }

    fn is_whitespace(c: char) -> bool {
        c == ' ' || c == '\t'
    }

    fn is_newline(c: char) -> bool {
        c == '\n' || c == '\r'
    }

    fn is_valid_identifier_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '-' || c == '.'
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

    fn read_while_n<F>(&mut self, n: usize, predicate: F) -> String
    where
        F: Fn(char) -> bool,
    {
        let mut result = String::new();
        while let Some(&c) = self.input.peek() {
            if predicate(c) {
                result.push(c);
                self.input.next();

                if result.len() >= n {
                    break;
                }
            } else {
                break;
            }
        }
        result
    }

    fn next_token(&mut self) -> Option<(SyntaxKind, String)> {
        if let Some(&c) = self.input.peek() {
            match (c, self.line_type) {
                (c, None) | (c, Some(LineType::Header)) if Self::is_valid_identifier_char(c) => {
                    let identifier = self.read_while(Self::is_valid_identifier_char);
                    self.line_type = Some(LineType::Header);
                    Some((SyntaxKind::IDENTIFIER, identifier))
                }
                (c, None) if Self::is_whitespace(c) => {
                    let mut indent = self.read_while_n(2, |c| c == ' ');
                    if indent.len() == 1 {
                        let dashes = self.read_while(|c| c == '-');
                        indent.push_str(dashes.as_str());
                        // Read exactly one trailing space after the dashes
                        if self.input.peek() == Some(&' ') {
                            indent.push(self.input.next().unwrap());
                        }
                        self.line_type = Some(LineType::Footer);
                    } else {
                        self.line_type = Some(LineType::Body);
                    }
                    Some((SyntaxKind::INDENT, indent))
                }
                ('#', None) => {
                    let comment = self.read_while(|c| !Self::is_newline(c));
                    let n = self.input.next();
                    if let Some(n) = n {
                        Some((SyntaxKind::COMMENT, comment + &n.to_string()))
                    } else {
                        Some((SyntaxKind::COMMENT, comment))
                    }
                }
                (c, _) if Self::is_newline(c) => {
                    self.input.next();
                    self.line_type = None;
                    Some((SyntaxKind::NEWLINE, String::from(c)))
                }
                (';', Some(LineType::Header)) => Some((
                    SyntaxKind::SEMICOLON,
                    String::from(self.input.next().unwrap()),
                )),
                ('(', Some(LineType::Header)) => {
                    let version = self
                        .read_while(|c| c != ')' && c != ';' && c != ' ' && !Self::is_newline(c));
                    let n = self.input.next();
                    if n == Some(')') {
                        Some((SyntaxKind::VERSION, version + &n.unwrap().to_string()))
                    } else if let Some(n) = n {
                        Some((SyntaxKind::ERROR, version + &n.to_string()))
                    } else {
                        Some((SyntaxKind::ERROR, version))
                    }
                }
                ('=', Some(LineType::Header)) => {
                    Some((SyntaxKind::EQUALS, String::from(self.input.next().unwrap())))
                }
                (_, Some(LineType::Body)) => {
                    let detail = self.read_while(|c| !Self::is_newline(c));
                    Some((SyntaxKind::DETAIL, detail))
                }
                (c, _) if Self::is_whitespace(c) => {
                    let ws = self.read_while(Self::is_whitespace);
                    Some((SyntaxKind::WHITESPACE, ws))
                }

                ('<', Some(LineType::Footer)) => {
                    let email = self.read_while(|c| c != '>' && c != ' ' && !Self::is_newline(c));
                    let n = self.input.next();
                    if n == Some('>') {
                        Some((SyntaxKind::EMAIL, email + &n.unwrap().to_string()))
                    } else if let Some(n) = n {
                        Some((SyntaxKind::ERROR, email + &n.to_string()))
                    } else {
                        Some((SyntaxKind::ERROR, email))
                    }
                }
                (c, Some(LineType::Footer)) if !Self::is_whitespace(c) && !Self::is_newline(c) => {
                    let identifier =
                        self.read_while(|c| c != ' ' && c != '<' && !Self::is_newline(c));
                    Some((SyntaxKind::TEXT, identifier))
                }
                (_, _) => {
                    self.input.next();
                    Some((SyntaxKind::ERROR, String::from(c)))
                }
            }
        } else {
            None
        }
    }
}

impl Iterator for Lexer<'_> {
    type Item = (crate::SyntaxKind, String);

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}

pub(crate) fn lex(input: &str) -> Vec<(SyntaxKind, String)> {
    let mut lexer = Lexer::new(input);
    lexer.by_ref().collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use crate::SyntaxKind::*;
    #[test]
    fn test_empty() {
        assert_eq!(super::lex(""), vec![]);
    }

    #[test]
    fn test_simple() {
        assert_eq!(
            super::lex(
                r#"breezy (3.3.4-1) unstable; urgency=low

  * New upstream release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 04 Sep 2023 18:13:45 -0000

# Oh, and here is a comment
"#
            )
            .iter()
            .map(|(kind, text)| (*kind, text.as_str()))
            .collect::<Vec<_>>(),
            vec![
                (IDENTIFIER, "breezy"),
                (WHITESPACE, " "),
                (VERSION, "(3.3.4-1)"),
                (WHITESPACE, " "),
                (IDENTIFIER, "unstable"),
                (SEMICOLON, ";"),
                (WHITESPACE, " "),
                (IDENTIFIER, "urgency"),
                (EQUALS, "="),
                (IDENTIFIER, "low"),
                (NEWLINE, "\n"),
                (NEWLINE, "\n"),
                (INDENT, "  "),
                (DETAIL, "* New upstream release."),
                (NEWLINE, "\n"),
                (NEWLINE, "\n"),
                (INDENT, " -- "),
                (TEXT, "Jelmer"),
                (WHITESPACE, " "),
                (TEXT, "Vernooĳ"),
                (WHITESPACE, " "),
                (EMAIL, "<jelmer@debian.org>"),
                (WHITESPACE, "  "),
                (TEXT, "Mon,"),
                (WHITESPACE, " "),
                (TEXT, "04"),
                (WHITESPACE, " "),
                (TEXT, "Sep"),
                (WHITESPACE, " "),
                (TEXT, "2023"),
                (WHITESPACE, " "),
                (TEXT, "18:13:45"),
                (WHITESPACE, " "),
                (TEXT, "-0000"),
                (NEWLINE, "\n"),
                (NEWLINE, "\n"),
                (COMMENT, "# Oh, and here is a comment\n"),
            ]
        );
    }

    #[test]
    fn test_email_edge_cases() {
        // Test email without closing >
        assert_eq!(
            super::lex(" -- Name <email@example.com")
                .iter()
                .map(|(kind, text)| (*kind, text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (INDENT, " -- "),
                (TEXT, "Name"),
                (WHITESPACE, " "),
                (ERROR, "<email@example.com"),
            ]
        );

        // Test email with extra characters after >
        assert_eq!(
            super::lex(" -- Name <email@example.com>x")
                .iter()
                .map(|(kind, text)| (*kind, text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (INDENT, " -- "),
                (TEXT, "Name"),
                (WHITESPACE, " "),
                (EMAIL, "<email@example.com>"),
                (TEXT, "x"),
            ]
        );
    }

    #[test]
    fn test_comment_without_newline() {
        assert_eq!(
            super::lex("# Comment without newline")
                .iter()
                .map(|(kind, text)| (*kind, text.as_str()))
                .collect::<Vec<_>>(),
            vec![(COMMENT, "# Comment without newline")]
        );
    }

    #[test]
    fn test_footer_text_parsing() {
        // Test footer line with various characters
        assert_eq!(
            super::lex(" -- Name123-test")
                .iter()
                .map(|(kind, text)| (*kind, text.as_str()))
                .collect::<Vec<_>>(),
            vec![(INDENT, " -- "), (TEXT, "Name123-test"),]
        );
    }

    #[test]
    fn test_footer_no_maintainer_name() {
        // Footer with no maintainer name (extra space before email)
        assert_eq!(
            super::lex(" --  <atterer@debian.org>  Wed, 17 Mar 2004 15:12:21 +0100\n")
                .iter()
                .map(|(kind, text)| (*kind, text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (INDENT, " -- "),
                (WHITESPACE, " "),
                (EMAIL, "<atterer@debian.org>"),
                (WHITESPACE, "  "),
                (TEXT, "Wed,"),
                (WHITESPACE, " "),
                (TEXT, "17"),
                (WHITESPACE, " "),
                (TEXT, "Mar"),
                (WHITESPACE, " "),
                (TEXT, "2004"),
                (WHITESPACE, " "),
                (TEXT, "15:12:21"),
                (WHITESPACE, " "),
                (TEXT, "+0100"),
                (NEWLINE, "\n"),
            ]
        );
    }

    #[test]
    fn test_whitespace_handling() {
        // Test various whitespace scenarios - when at start of line with detail context, it becomes INDENT
        assert_eq!(
            super::lex("  \t  ")
                .iter()
                .map(|(kind, text)| (*kind, text.as_str()))
                .collect::<Vec<_>>(),
            vec![(INDENT, "  "), (DETAIL, "\t  ")]
        );
    }

    #[test]
    fn test_lex_empty_bullet() {
        // A line with "  * \n" (bullet with trailing space, no content)
        assert_eq!(
            super::lex("  * \n")
                .iter()
                .map(|(kind, text)| (*kind, text.as_str()))
                .collect::<Vec<_>>(),
            vec![(INDENT, "  "), (DETAIL, "* "), (NEWLINE, "\n"),]
        );
    }

    #[test]
    fn test_lex_trailing_space_line() {
        // A line with just " \n" (single space then newline) should produce
        // INDENT " " followed by NEWLINE
        assert_eq!(
            super::lex("  * detail\n\n \n\nbar (1.0) unstable; urgency=low\n")
                .iter()
                .map(|(kind, text)| (*kind, text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (INDENT, "  "),
                (DETAIL, "* detail"),
                (NEWLINE, "\n"),
                (NEWLINE, "\n"),
                (INDENT, " "),
                (NEWLINE, "\n"),
                (NEWLINE, "\n"),
                (IDENTIFIER, "bar"),
                (WHITESPACE, " "),
                (VERSION, "(1.0)"),
                (WHITESPACE, " "),
                (IDENTIFIER, "unstable"),
                (SEMICOLON, ";"),
                (WHITESPACE, " "),
                (IDENTIFIER, "urgency"),
                (EQUALS, "="),
                (IDENTIFIER, "low"),
                (NEWLINE, "\n"),
            ]
        );
    }
}
