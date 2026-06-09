//! License expression parsing for DEP-5 copyright files.
//!
//! License expressions combine license names with `or`, `and`, and `with` operators.
//! `and` binds tighter than `or`. A comma before an operator lowers its precedence,
//! e.g. `A or B, and C` means `(A or B) and C`.
//!
//! The `with` keyword attaches an exception to the preceding license name
//! (e.g. `GPL-2+ with OpenSSL-exception`).

/// A parsed license expression from a DEP-5 copyright file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LicenseExpr {
    /// A single license name, e.g. `MIT`.
    Name(String),

    /// A license with an exception, e.g. `GPL-2+ with OpenSSL-exception`.
    WithException(String, String),

    /// All of these licenses apply simultaneously.
    And(Vec<LicenseExpr>),

    /// Any one of these licenses may be chosen.
    Or(Vec<LicenseExpr>),
}

impl LicenseExpr {
    /// Parse a license expression string.
    ///
    /// # Examples
    ///
    /// ```
    /// use debian_copyright::LicenseExpr;
    ///
    /// let expr = LicenseExpr::parse("GPL-2+ or MIT");
    /// assert_eq!(expr, LicenseExpr::Or(vec![
    ///     LicenseExpr::Name("GPL-2+".to_string()),
    ///     LicenseExpr::Name("MIT".to_string()),
    /// ]));
    ///
    /// let expr = LicenseExpr::parse("GPL-2+ with OpenSSL-exception");
    /// assert_eq!(expr, LicenseExpr::WithException(
    ///     "GPL-2+".to_string(),
    ///     "OpenSSL-exception".to_string(),
    /// ));
    /// ```
    pub fn parse(input: &str) -> Self {
        let tokens = tokenize(input);
        if tokens.is_empty() {
            return LicenseExpr::Name(String::new());
        }
        parse_expr(input, &tokens)
    }

    /// Returns the individual license names contained in this expression.
    ///
    /// For `WithException` variants, only the license name is returned,
    /// not the exception name.
    pub fn license_names(&self) -> Vec<&str> {
        let mut names = Vec::new();
        self.collect_names(&mut names);
        names
    }

    /// Locate each license name in `input` along with its byte range.
    ///
    /// Returns each license name paired with the half-open byte range it
    /// occupies in `input`. Exception words after `with` are skipped, matching
    /// [`license_names`](Self::license_names). Unlike `license_names`, no
    /// entry is emitted for expressions that contain no name token, since
    /// there is no meaningful range to report.
    ///
    /// # Examples
    ///
    /// ```
    /// use debian_copyright::LicenseExpr;
    ///
    /// let input = "GPL-2+ or MIT";
    /// assert_eq!(
    ///     LicenseExpr::name_ranges(input),
    ///     vec![("GPL-2+", 0..6), ("MIT", 10..13)],
    /// );
    ///
    /// let input = "GPL-2+ with OpenSSL-exception or MIT";
    /// assert_eq!(
    ///     LicenseExpr::name_ranges(input),
    ///     vec![("GPL-2+", 0..6), ("MIT", 33..36)],
    /// );
    /// ```
    pub fn name_ranges(input: &str) -> Vec<(&str, std::ops::Range<usize>)> {
        let tokens = tokenize(input);
        let mut out = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            match &tokens[i].kind {
                TokenKind::Word => {
                    let range = tokens[i].range.clone();
                    out.push((&input[range.clone()], range));
                    i += 1;
                    if matches!(tokens.get(i).map(|t| &t.kind), Some(TokenKind::With)) {
                        i += 1;
                        while matches!(tokens.get(i).map(|t| &t.kind), Some(TokenKind::Word)) {
                            i += 1;
                        }
                    }
                }
                _ => {
                    i += 1;
                }
            }
        }
        out
    }

    fn collect_names<'a>(&'a self, names: &mut Vec<&'a str>) {
        match self {
            LicenseExpr::Name(n) => names.push(n),
            LicenseExpr::WithException(n, _) => names.push(n),
            LicenseExpr::And(exprs) | LicenseExpr::Or(exprs) => {
                for expr in exprs {
                    expr.collect_names(names);
                }
            }
        }
    }
}

impl std::fmt::Display for LicenseExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LicenseExpr::Name(n) => f.write_str(n),
            LicenseExpr::WithException(n, e) => write!(f, "{} with {}", n, e),
            LicenseExpr::And(exprs) => {
                for (i, expr) in exprs.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" and ")?;
                    }
                    write!(f, "{}", expr)?;
                }
                Ok(())
            }
            LicenseExpr::Or(exprs) => {
                for (i, expr) in exprs.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" or ")?;
                    }
                    write!(f, "{}", expr)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum TokenKind {
    Word,
    Or,
    And,
    With,
    Comma,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    range: std::ops::Range<usize>,
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && !(bytes[i] as char).is_whitespace() {
            i += 1;
        }
        let mut end = i;
        let mut trailing_comma = false;
        if end > start && bytes[end - 1] == b',' {
            trailing_comma = true;
            end -= 1;
        }
        if end > start {
            let word = &input[start..end];
            let kind = if word.eq_ignore_ascii_case("or") {
                TokenKind::Or
            } else if word.eq_ignore_ascii_case("and") {
                TokenKind::And
            } else if word.eq_ignore_ascii_case("with") {
                TokenKind::With
            } else {
                TokenKind::Word
            };
            tokens.push(Token {
                kind,
                range: start..end,
            });
        }
        if trailing_comma {
            tokens.push(Token {
                kind: TokenKind::Comma,
                range: end..end + 1,
            });
        }
    }
    tokens
}

/// Parse a single license term: a name optionally followed by `with <exception>`.
/// The exception after `with` consumes all words until the next `or`, `and`, comma, or end.
fn parse_term(input: &str, tokens: &[Token], pos: &mut usize) -> LicenseExpr {
    let name = match tokens.get(*pos) {
        Some(Token {
            kind: TokenKind::Word,
            range,
        }) => {
            let w = input[range.clone()].to_string();
            *pos += 1;
            w
        }
        _ => return LicenseExpr::Name(String::new()),
    };

    if matches!(tokens.get(*pos).map(|t| &t.kind), Some(TokenKind::With)) {
        *pos += 1;
        let mut exception_parts = Vec::new();
        while let Some(Token {
            kind: TokenKind::Word,
            range,
        }) = tokens.get(*pos)
        {
            exception_parts.push(input[range.clone()].to_string());
            *pos += 1;
        }
        LicenseExpr::WithException(name, exception_parts.join(" "))
    } else {
        LicenseExpr::Name(name)
    }
}

/// Parse a token stream into a `LicenseExpr`.
///
/// Handles comma-lowered precedence by splitting on `, and` / `, or` first,
/// then parsing each segment with normal precedence (`and` > `or`).
fn parse_expr(input: &str, tokens: &[Token]) -> LicenseExpr {
    // Split into segments at comma boundaries (comma + operator = low precedence).
    let mut segments: Vec<(Vec<Token>, Option<TokenKind>)> = Vec::new();
    let mut current: Vec<Token> = Vec::new();

    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].kind == TokenKind::Comma {
            if i + 1 < tokens.len() && matches!(tokens[i + 1].kind, TokenKind::Or | TokenKind::And)
            {
                let op = tokens[i + 1].kind.clone();
                segments.push((std::mem::take(&mut current), Some(op)));
                i += 2;
            } else {
                i += 1;
            }
        } else {
            current.push(tokens[i].clone());
            i += 1;
        }
    }
    if !current.is_empty() {
        segments.push((current, None));
    }

    if segments.len() == 1 {
        return parse_segment(input, &segments[0].0);
    }

    // Group segments by their joining low-precedence operator.
    // Low-precedence `and` binds tighter than low-precedence `or`.
    let mut and_groups: Vec<Vec<LicenseExpr>> = vec![vec![parse_segment(input, &segments[0].0)]];

    for i in 1..segments.len() {
        let preceding_op = segments[i - 1].1.as_ref().unwrap_or(&TokenKind::Or);
        if matches!(preceding_op, TokenKind::And) {
            and_groups
                .last_mut()
                .unwrap()
                .push(parse_segment(input, &segments[i].0));
        } else {
            and_groups.push(vec![parse_segment(input, &segments[i].0)]);
        }
    }

    let flattened: Vec<LicenseExpr> = and_groups
        .into_iter()
        .map(|group| {
            if group.len() == 1 {
                group.into_iter().next().unwrap()
            } else {
                LicenseExpr::And(group)
            }
        })
        .collect();

    if flattened.len() == 1 {
        flattened.into_iter().next().unwrap()
    } else {
        LicenseExpr::Or(flattened)
    }
}

/// Parse a segment (no comma-lowered operators) with normal precedence: `and` > `or`.
fn parse_segment(input: &str, tokens: &[Token]) -> LicenseExpr {
    // Split on `or` (lower precedence), then each part on `and`.
    let mut or_groups: Vec<Vec<Token>> = vec![Vec::new()];
    for tok in tokens {
        if tok.kind == TokenKind::Or {
            or_groups.push(Vec::new());
        } else {
            or_groups.last_mut().unwrap().push(tok.clone());
        }
    }

    let or_exprs: Vec<LicenseExpr> = or_groups
        .into_iter()
        .map(|group| {
            let mut and_groups: Vec<Vec<Token>> = vec![Vec::new()];
            for tok in &group {
                if tok.kind == TokenKind::And {
                    and_groups.push(Vec::new());
                } else {
                    and_groups.last_mut().unwrap().push(tok.clone());
                }
            }

            let and_exprs: Vec<LicenseExpr> = and_groups
                .into_iter()
                .map(|toks| {
                    let mut pos = 0;
                    parse_term(input, &toks, &mut pos)
                })
                .collect();

            if and_exprs.len() == 1 {
                and_exprs.into_iter().next().unwrap()
            } else {
                LicenseExpr::And(and_exprs)
            }
        })
        .collect();

    if or_exprs.len() == 1 {
        or_exprs.into_iter().next().unwrap()
    } else {
        LicenseExpr::Or(or_exprs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_name() {
        assert_eq!(LicenseExpr::parse("MIT"), LicenseExpr::Name("MIT".into()));
    }

    #[test]
    fn test_or() {
        assert_eq!(
            LicenseExpr::parse("GPL-2+ or MIT"),
            LicenseExpr::Or(vec![
                LicenseExpr::Name("GPL-2+".into()),
                LicenseExpr::Name("MIT".into()),
            ])
        );
    }

    #[test]
    fn test_and() {
        assert_eq!(
            LicenseExpr::parse("Apache-2.0 and BSD-3-clause"),
            LicenseExpr::And(vec![
                LicenseExpr::Name("Apache-2.0".into()),
                LicenseExpr::Name("BSD-3-clause".into()),
            ])
        );
    }

    #[test]
    fn test_with_exception() {
        assert_eq!(
            LicenseExpr::parse("GPL-2+ with OpenSSL-exception"),
            LicenseExpr::WithException("GPL-2+".into(), "OpenSSL-exception".into())
        );
    }

    #[test]
    fn test_with_multi_word_exception() {
        assert_eq!(
            LicenseExpr::parse("GPL-2+ with Autoconf exception"),
            LicenseExpr::WithException("GPL-2+".into(), "Autoconf exception".into())
        );
    }

    #[test]
    fn test_with_exception_then_or() {
        assert_eq!(
            LicenseExpr::parse("GPL-2+ with OpenSSL-exception or MIT"),
            LicenseExpr::Or(vec![
                LicenseExpr::WithException("GPL-2+".into(), "OpenSSL-exception".into()),
                LicenseExpr::Name("MIT".into()),
            ])
        );
    }

    #[test]
    fn test_and_binds_tighter_than_or() {
        // A or B and C → A or (B and C)
        assert_eq!(
            LicenseExpr::parse("A or B and C"),
            LicenseExpr::Or(vec![
                LicenseExpr::Name("A".into()),
                LicenseExpr::And(vec![
                    LicenseExpr::Name("B".into()),
                    LicenseExpr::Name("C".into()),
                ]),
            ])
        );
    }

    #[test]
    fn test_comma_lowers_precedence() {
        // A or B, and C → (A or B) and C
        assert_eq!(
            LicenseExpr::parse("A or B, and C"),
            LicenseExpr::And(vec![
                LicenseExpr::Or(vec![
                    LicenseExpr::Name("A".into()),
                    LicenseExpr::Name("B".into()),
                ]),
                LicenseExpr::Name("C".into()),
            ])
        );
    }

    #[test]
    fn test_case_insensitive_operators() {
        assert_eq!(
            LicenseExpr::parse("GPL-2+ OR MIT"),
            LicenseExpr::Or(vec![
                LicenseExpr::Name("GPL-2+".into()),
                LicenseExpr::Name("MIT".into()),
            ])
        );
    }

    #[test]
    fn test_license_names() {
        let expr = LicenseExpr::parse("GPL-2+ or MIT and BSD-3-clause");
        assert_eq!(expr.license_names(), vec!["GPL-2+", "MIT", "BSD-3-clause"]);
    }

    #[test]
    fn test_license_names_with_exception() {
        let expr = LicenseExpr::parse("GPL-2+ with OpenSSL-exception or MIT");
        assert_eq!(expr.license_names(), vec!["GPL-2+", "MIT"]);
    }

    #[test]
    fn test_display_round_trip_simple() {
        let input = "GPL-2+ or MIT";
        let expr = LicenseExpr::parse(input);
        assert_eq!(expr.to_string(), input);
    }

    #[test]
    fn test_display_with_exception() {
        let input = "GPL-2+ with OpenSSL-exception";
        let expr = LicenseExpr::parse(input);
        assert_eq!(expr.to_string(), input);
    }

    #[test]
    fn test_name_ranges_simple() {
        let input = "MIT";
        assert_eq!(LicenseExpr::name_ranges(input), vec![("MIT", 0..3)]);
    }

    #[test]
    fn test_name_ranges_or() {
        let input = "GPL-2+ or MIT";
        assert_eq!(
            LicenseExpr::name_ranges(input),
            vec![("GPL-2+", 0..6), ("MIT", 10..13)],
        );
    }

    #[test]
    fn test_name_ranges_and() {
        let input = "Apache-2.0 and BSD-3-clause";
        assert_eq!(
            LicenseExpr::name_ranges(input),
            vec![("Apache-2.0", 0..10), ("BSD-3-clause", 15..27)],
        );
    }

    #[test]
    fn test_name_ranges_with_exception() {
        let input = "GPL-2+ with OpenSSL-exception or MIT";
        assert_eq!(
            LicenseExpr::name_ranges(input),
            vec![("GPL-2+", 0..6), ("MIT", 33..36)],
        );
    }

    #[test]
    fn test_name_ranges_multi_word_exception() {
        let input = "GPL-2+ with Autoconf exception or MIT";
        assert_eq!(
            LicenseExpr::name_ranges(input),
            vec![("GPL-2+", 0..6), ("MIT", 34..37)],
        );
    }

    #[test]
    fn test_name_ranges_comma_lowered() {
        let input = "A or B, and C";
        assert_eq!(
            LicenseExpr::name_ranges(input),
            vec![("A", 0..1), ("B", 5..6), ("C", 12..13)],
        );
    }

    #[test]
    fn test_name_ranges_empty() {
        assert_eq!(LicenseExpr::name_ranges(""), Vec::<(&str, _)>::new());
        assert_eq!(LicenseExpr::name_ranges("   "), Vec::<(&str, _)>::new());
    }

    #[test]
    fn test_name_ranges_matches_license_names() {
        let cases = [
            "GPL-2+ or MIT",
            "Apache-2.0 and BSD-3-clause",
            "GPL-2+ with OpenSSL-exception or MIT",
            "A or B, and C",
            "GPL-1+ or Artistic or Perl",
        ];
        for input in cases {
            let from_expr: Vec<String> = LicenseExpr::parse(input)
                .license_names()
                .into_iter()
                .map(str::to_owned)
                .collect();
            let from_ranges: Vec<String> = LicenseExpr::name_ranges(input)
                .into_iter()
                .map(|(n, _)| n.to_owned())
                .collect();
            assert_eq!(from_ranges, from_expr, "mismatch for input {input:?}");
        }
    }

    #[test]
    fn test_three_way_or() {
        assert_eq!(
            LicenseExpr::parse("GPL-1+ or Artistic or Perl"),
            LicenseExpr::Or(vec![
                LicenseExpr::Name("GPL-1+".into()),
                LicenseExpr::Name("Artistic".into()),
                LicenseExpr::Name("Perl".into()),
            ])
        );
    }
}
