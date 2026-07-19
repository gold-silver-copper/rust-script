use ra_ap_parser::{Edition, LexedStr, SyntaxKind, T};

use crate::{Diagnostic, ParseLimits, Phase, Span};

pub(super) fn validate(source: &str, limits: ParseLimits) -> Result<(), Diagnostic> {
    let lexed = LexedStr::new(Edition::Edition2024, source);
    if lexed.len() > limits.max_tokens {
        return Err(error(
            "token limit exceeded",
            lexed.text_range(limits.max_tokens.min(lexed.len() - 1)),
        ));
    }
    if let Some((index, message)) = lexed.errors().next() {
        return Err(error(message, lexed.text_range(index)));
    }

    let mut delimiters = Vec::new();
    let mut prefix_run = 0usize;
    for index in 0..lexed.len() {
        let kind = lexed.kind(index);
        let text = lexed.text(index);
        // `SourceFile::parse` recurses on prefix-position tokens (`-x`, `!x`,
        // `&x`, `*x`, `||`-closures, `return return ...`), which delimiter
        // depth cannot bound. A long homogeneous run would overflow the parser
        // stack (an abort, not an unwind), so cap runs at the same value as
        // the post-parse syntax nesting limit before parsing ever starts.
        if !matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::COMMENT) {
            if matches!(
                kind,
                T![-] | T![!] | T![&] | T![|] | T![*] | T![return] | T![break]
            ) {
                prefix_run += 1;
                if prefix_run > limits.max_syntax_depth {
                    return Err(error(
                        "prefix operator nesting limit exceeded",
                        lexed.text_range(index),
                    ));
                }
            } else {
                prefix_run = 0;
            }
        }
        if matches!(kind, T![&] | T![|]) {
            let paired_before = index > 0
                && lexed.kind(index - 1) == kind
                && lexed.text_range(index - 1).end == lexed.text_range(index).start;
            let paired_after = index + 1 < lexed.len()
                && lexed.kind(index + 1) == kind
                && lexed.text_range(index).end == lexed.text_range(index + 1).start;
            if !paired_before && !paired_after {
                return Err(error(
                    "single `&` and `|` operators are unsupported",
                    lexed.text_range(index),
                ));
            }
        } else {
            validate_token(kind, text, lexed.text_range(index))?;
        }
        match kind {
            T!['('] | T!['{'] => {
                delimiters.push((kind, lexed.text_range(index)));
                if delimiters.len() > limits.max_delimiter_depth {
                    return Err(error(
                        "delimiter nesting limit exceeded",
                        lexed.text_range(index),
                    ));
                }
            }
            T![')'] => close(&mut delimiters, T!['('], lexed.text_range(index))?,
            T!['}'] => close(&mut delimiters, T!['{'], lexed.text_range(index))?,
            _ => {}
        }
    }
    if let Some((_, range)) = delimiters.last() {
        return Err(error("unclosed delimiter", range.clone()));
    }
    Ok(())
}

fn close(
    delimiters: &mut Vec<(SyntaxKind, std::ops::Range<usize>)>,
    expected: SyntaxKind,
    range: std::ops::Range<usize>,
) -> Result<(), Diagnostic> {
    match delimiters.pop() {
        Some((kind, _)) if kind == expected => Ok(()),
        _ => Err(error("mismatched closing delimiter", range)),
    }
}

// `LexedStr` wraps `rustc_lexer`, which emits only single-character
// punctuation; compound operators (`->`, `==`, `<=`, `&&`, `||`, ...) arrive
// as adjacent singles and are glued back together by `SourceFile::parse`.
// The whitelist therefore lists only single-character punctuation kinds.
fn validate_token(
    kind: SyntaxKind,
    text: &str,
    range: std::ops::Range<usize>,
) -> Result<(), Diagnostic> {
    match kind {
        SyntaxKind::WHITESPACE
            if text
                .bytes()
                .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n')) =>
        {
            Ok(())
        }
        SyntaxKind::WHITESPACE => Err(error("unsupported whitespace", range)),
        SyntaxKind::COMMENT if text.starts_with("//") && !is_doc_comment(text) => Ok(()),
        SyntaxKind::COMMENT => Err(error(
            "block and documentation comments are unsupported",
            range,
        )),
        SyntaxKind::IDENT if text.starts_with("r#") => {
            Err(error("raw identifiers are unsupported", range))
        }
        SyntaxKind::IDENT => Ok(()),
        SyntaxKind::INT_NUMBER if valid_integer(text) => Ok(()),
        SyntaxKind::INT_NUMBER => Err(error("integer literals must be decimal DIGITS_i64", range)),
        SyntaxKind::STRING if text == "\"{}\"" => Ok(()),
        SyntaxKind::STRING => Err(error(
            "only the println placeholder string is supported",
            range,
        )),
        T![fn]
        | T![let]
        | T![mut]
        | T![while]
        | T![return]
        | T![break]
        | T![continue]
        | T![if]
        | T![else]
        | T![true]
        | T![false]
        | T!['(']
        | T![')']
        | T!['{']
        | T!['}']
        | T![;]
        | T![,]
        | T![:]
        | T![=]
        | T![!]
        | T![-]
        | T![+]
        | T![*]
        | T![/]
        | T![%]
        | T![<]
        | T![>] => Ok(()),
        _ => Err(error(format!("unsupported token `{text}`"), range)),
    }
}

fn is_doc_comment(text: &str) -> bool {
    text.starts_with("//!") || (text.starts_with("///") && !text.starts_with("////"))
}

fn valid_integer(text: &str) -> bool {
    let Some(digits) = text.strip_suffix("_i64") else {
        return false;
    };
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && digits.parse::<i64>().is_ok()
}

fn error(message: impl Into<String>, range: std::ops::Range<usize>) -> Diagnostic {
    Diagnostic::new(
        Phase::Lex,
        message,
        Some(Span {
            start: range.start,
            end: range.end,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_profile_tokens() {
        validate(
            "fn main() { // hi\n println!(\"{}\", 42_i64); }",
            ParseLimits::default(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_raw_identifier_and_block_comment() {
        assert!(validate("fn r#main() {}", ParseLimits::default()).is_err());
        assert!(validate("fn main() { /* no */ }", ParseLimits::default()).is_err());
    }

    #[test]
    fn distinguishes_doc_and_ordinary_slash_comments() {
        for source in [
            "fn main() { //// ordinary\n}",
            "fn main() { ////! ordinary\n}",
        ] {
            validate(source, ParseLimits::default()).unwrap();
        }
        for source in ["fn main() { /// docs\n}", "fn main() { //! docs\n}"] {
            assert!(validate(source, ParseLimits::default()).is_err());
        }
    }

    #[test]
    fn rejects_literal_variants() {
        for source in [
            "fn main(){1;}",
            "fn main(){1_u64;}",
            "fn main(){0x1_i64;}",
            "fn main(){\"x\";}",
        ] {
            assert!(validate(source, ParseLimits::default()).is_err(), "{source}");
        }
    }

    #[test]
    fn propagates_lexed_str_error_ranges() {
        let source = "fn main() { \"unterminated }";
        let error = validate(source, ParseLimits::default()).unwrap_err();
        let start = source.find('"').expect("string start");
        assert_eq!(
            error.span,
            Some(Span {
                start,
                end: source.len()
            })
        );
    }

    #[test]
    fn bounds_prefix_operator_runs_before_parsing() {
        // Each of these previously reached `SourceFile::parse` and overflowed
        // the parser stack (a process abort `catch_unwind` cannot contain).
        for operator in ["-", "!", "&", "|", "*", "return ", "break "] {
            let source = format!("fn main() {{ let x = {}1_i64; }}", operator.repeat(20_000));
            let error = validate(&source, ParseLimits::default()).unwrap_err();
            assert_eq!(error.message, "prefix operator nesting limit exceeded");
        }
    }

    #[test]
    fn allows_prefix_runs_up_to_the_nesting_limit() {
        let limits = ParseLimits::default();
        let source = format!(
            "fn main() {{ let x = {}1_i64; }}",
            "-".repeat(limits.max_syntax_depth)
        );
        validate(&source, limits).unwrap();
        let source = format!(
            "fn main() {{ let x = {}1_i64; }}",
            "-".repeat(limits.max_syntax_depth + 1)
        );
        assert!(validate(&source, limits).is_err());
    }

    #[test]
    fn rejects_non_profile_ascii_whitespace() {
        for whitespace in ['\u{000b}', '\u{000c}'] {
            let source = format!("fn{whitespace}main() {{}}");
            assert!(validate(&source, ParseLimits::default()).is_err(), "{source:?}");
        }
    }
}
