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
    let mut prefix_nesting = 0usize;
    let mut previous_kind = None;
    for index in 0..lexed.len() {
        let kind = lexed.kind(index);
        let text = lexed.text(index);
        // `SourceFile::parse` recurses on every prefix-position token (`-x`,
        // `!x`, `&x`, `*x`, `||`-closures) and on the value-carrying keywords
        // `return`/`break`, none of which delimiter depth can bound. Such a
        // parser frame can stay live until the enclosing statement ends, so
        // interleaved operands do not release it (`return 1_i64 - return ...`
        // right-nests one frame per `return`). Cap the number of
        // frame-opening tokens per statement at the syntax nesting limit,
        // resetting only at `;`: a fatal parser stack overflow is an abort
        // that `catch_unwind` cannot contain, so it must be prevented before
        // parsing starts. Operators directly after an operand are in binary
        // position, parse iteratively, and are exempt.
        if !matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::COMMENT) {
            if kind == T![;] {
                prefix_nesting = 0;
            } else if matches!(kind, T![return] | T![break])
                || (matches!(kind, T![-] | T![!] | T![&] | T![|] | T![*])
                    && !previous_kind.is_some_and(ends_operand))
            {
                prefix_nesting += 1;
                if prefix_nesting > limits.max_syntax_depth {
                    return Err(error(
                        "prefix operator nesting limit exceeded",
                        lexed.text_range(index),
                    ));
                }
            }
            previous_kind = Some(kind);
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

// A token that completes an operand: an operator directly after one of these
// is in binary position and cannot open a prefix-expression parser frame.
fn ends_operand(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT
            | SyntaxKind::INT_NUMBER
            | SyntaxKind::STRING
            | T![true]
            | T![false]
            | T![')']
            | T!['}']
    )
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
    // Leading zeros are rejected so every admitted literal is already in
    // canonical form and both subset emitters agree byte-for-byte.
    !digits.is_empty()
        && (digits == "0" || !digits.starts_with('0'))
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
            assert!(
                validate(source, ParseLimits::default()).is_err(),
                "{source}"
            );
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
    fn bounds_interleaved_prefix_nesting_within_a_statement() {
        // `return 1_i64 - return 1_i64 - ...` right-nests one parser frame
        // per `return` even though the run of prefix tokens is never longer
        // than two; a consecutive-run bound missed this and the parser
        // aborted with a fatal stack overflow. The per-statement counter
        // catches it because interleaved operands do not release the frames.
        for unit in ["return 1_i64 - ", "break 1_i64 - ", "- return "] {
            let source = format!("fn main() {{ let x = {}1_i64; }}", unit.repeat(9_000));
            let error = validate(&source, ParseLimits::default()).unwrap_err();
            assert_eq!(
                error.message, "prefix operator nesting limit exceeded",
                "{unit}"
            );
        }
    }

    #[test]
    fn binary_position_operators_do_not_count_toward_prefix_nesting() {
        // Left-associative binary chains parse iteratively; operators that
        // directly follow an operand must not accumulate toward the bound,
        // and `;` releases per-statement nesting.
        let long_chain = "1_i64 - ".repeat(2_000);
        let source = format!("fn main() {{ let x = {long_chain}1_i64; }}");
        validate(&source, ParseLimits::default()).unwrap();

        let many_statements = "let x = -1_i64; ".repeat(2_000);
        let source = format!("fn main() {{ {many_statements} }}");
        validate(&source, ParseLimits::default()).unwrap();

        validate(
            "fn negate(value: i64) -> i64 { 0_i64 - value } fn main() { negate(1_i64); }",
            ParseLimits::default(),
        )
        .unwrap();
    }

    #[test]
    fn token_limit_boundary_is_exact() {
        let source = "fn main() { 1_i64; }";
        let count = LexedStr::new(Edition::Edition2024, source).len();
        let at_limit = ParseLimits {
            max_tokens: count,
            ..ParseLimits::default()
        };
        validate(source, at_limit).unwrap();
        let below_limit = ParseLimits {
            max_tokens: count - 1,
            ..ParseLimits::default()
        };
        assert_eq!(
            validate(source, below_limit).unwrap_err().message,
            "token limit exceeded"
        );
    }

    #[test]
    fn delimiter_depth_boundary_is_exact() {
        // The signature parens close before the body brace opens, so the
        // deepest simultaneous nesting is 1 brace + 4 parens = 5.
        let source = "fn main() { ((((1_i64)))); }";
        let at_limit = ParseLimits {
            max_delimiter_depth: 5,
            ..ParseLimits::default()
        };
        validate(source, at_limit).unwrap();
        let below_limit = ParseLimits {
            max_delimiter_depth: 4,
            ..ParseLimits::default()
        };
        assert_eq!(
            validate(source, below_limit).unwrap_err().message,
            "delimiter nesting limit exceeded"
        );
    }

    #[test]
    fn rejects_leading_zero_integer_literals() {
        assert!(validate("fn main() { 01_i64; }", ParseLimits::default()).is_err());
        assert!(validate("fn main() { 007_i64; }", ParseLimits::default()).is_err());
        validate("fn main() { 0_i64; }", ParseLimits::default()).unwrap();
        validate("fn main() { 10_i64; }", ParseLimits::default()).unwrap();
    }

    #[test]
    fn rejects_non_profile_ascii_whitespace() {
        for whitespace in ['\u{000b}', '\u{000c}'] {
            let source = format!("fn{whitespace}main() {{}}");
            assert!(
                validate(&source, ParseLimits::default()).is_err(),
                "{source:?}"
            );
        }
    }
}
