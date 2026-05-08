use crate::SyntaxKind;
use crate::SyntaxKind::*;

/// Split the input string into a flat list of tokens
pub(crate) fn lex(text: &str) -> Vec<(SyntaxKind, String)> {
    fn tok(t: SyntaxKind) -> m_lexer::TokenKind {
        let sk = rowan::SyntaxKind::from(t);
        m_lexer::TokenKind(sk.0)
    }
    fn kind(t: m_lexer::TokenKind) -> SyntaxKind {
        match t.0 {
            0 => KEY,
            1 => VALUE,
            2 => EQUALS,
            3 => QUOTE,
            4 => COMMA,
            5 => CONTINUATION,
            6 => NEWLINE,
            7 => WHITESPACE,
            8 => COMMENT,
            9 => ERROR,
            _ => unreachable!(),
        }
    }

    let lexer = m_lexer::LexerBuilder::new()
        .error_token(tok(ERROR))
        .tokens(&[
            (tok(KEY), r"[a-z]+"),
            (tok(QUOTE), "\""),
            (tok(VALUE), r#"[^\s=,"]*[^\s=\\,"]"#),
            (tok(CONTINUATION), r"\\\n"),
            (tok(EQUALS), r"="),
            (tok(COMMA), r","),
            (tok(NEWLINE), r"\n"),
            (tok(WHITESPACE), r"[ \t\r]+"),
            (tok(COMMENT), r"#[^\n]*"),
        ])
        .build();

    lexer
        .tokenize(text)
        .into_iter()
        .map(|t| (t.len, kind(t.kind)))
        .scan(0usize, |start_offset, (len, kind)| {
            let s = text[*start_offset..*start_offset + len].to_string();
            *start_offset += len;
            Some((kind, s))
        })
        .collect()
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
                r#"version=4
opts=bare,filenamemangle=s/.+\/v?(\d\S+)\.tar\.gz/syncthing-gtk-$1\.tar\.gz/ \
  https://github.com/syncthing/syncthing-gtk/tags .*/v?(\d\S+)\.tar\.gz
"#
            ),
            vec![
                (KEY, "version".into()),
                (EQUALS, "=".into()),
                (VALUE, "4".into()),
                (NEWLINE, "\n".into()),
                (KEY, "opts".into()),
                (EQUALS, "=".into()),
                (KEY, "bare".into()),
                (COMMA, ",".into()),
                (KEY, "filenamemangle".into()),
                (EQUALS, "=".into()),
                (
                    VALUE,
                    "s/.+\\/v?(\\d\\S+)\\.tar\\.gz/syncthing-gtk-$1\\.tar\\.gz/".into()
                ),
                (WHITESPACE, " ".into()),
                (CONTINUATION, "\\\n".into()),
                (WHITESPACE, "  ".into()),
                (
                    VALUE,
                    "https://github.com/syncthing/syncthing-gtk/tags".into()
                ),
                (WHITESPACE, " ".into()),
                (VALUE, ".*/v?(\\d\\S+)\\.tar\\.gz".into()),
                (NEWLINE, "\n".into()),
            ]
        );
    }

    #[test]
    fn test_quoted() {
        assert_eq!(
            super::lex(
                r#"version=4
opts="bare, filenamemangle=foo" \
  https://github.com/syncthing/syncthing-gtk/tags .*/v?(\d\S+)\.tar\.gz
"#
            ),
            vec![
                (KEY, "version".into()),
                (EQUALS, "=".into()),
                (VALUE, "4".into()),
                (NEWLINE, "\n".into()),
                (KEY, "opts".into()),
                (EQUALS, "=".into()),
                (QUOTE, "\"".into()),
                (KEY, "bare".into()),
                (COMMA, ",".into()),
                (WHITESPACE, " ".into()),
                (KEY, "filenamemangle".into()),
                (EQUALS, "=".into()),
                (KEY, "foo".into()),
                (QUOTE, "\"".into()),
                (WHITESPACE, " ".into()),
                (CONTINUATION, "\\\n".into()),
                (WHITESPACE, "  ".into()),
                (
                    VALUE,
                    "https://github.com/syncthing/syncthing-gtk/tags".into()
                ),
                (WHITESPACE, " ".into()),
                (VALUE, ".*/v?(\\d\\S+)\\.tar\\.gz".into()),
                (NEWLINE, "\n".into()),
            ]
        );
    }
}
