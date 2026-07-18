use ra_ap_parser::{Edition, LexedStr, SyntaxKind, T};

use crate::{Diagnostic, Limits, Phase, Span};

pub(super) fn validate(source: &str, limits: Limits) -> Result<(), Diagnostic> {
    let lexed = LexedStr::new(Edition::Edition2024, source);
    if lexed.len() > limits.max_tokens {
        let start = lexed.text_start(limits.max_tokens.min(lexed.len()));
        return Err(error("token limit exceeded", start..start));
    }
    if let Some((index, message)) = lexed.errors().next() {
        return Err(error(message, lexed.text_range(index)));
    }

    let mut delimiters = Vec::new();
    for index in 0..lexed.len() {
        let kind = lexed.kind(index);
        let text = lexed.text(index);
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

fn validate_token(
    kind: SyntaxKind,
    text: &str,
    range: std::ops::Range<usize>,
) -> Result<(), Diagnostic> {
    match kind {
        SyntaxKind::WHITESPACE => Ok(()),
        SyntaxKind::COMMENT
            if text.starts_with("//") && !text.starts_with("///") && !text.starts_with("//!") =>
        {
            Ok(())
        }
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
        | T![->]
        | T![=]
        | T![!]
        | T![-]
        | T![+]
        | T![*]
        | T![/]
        | T![%]
        | T![==]
        | T![!=]
        | T![<]
        | T![<=]
        | T![>]
        | T![>=]
        | T![&&]
        | T![||] => Ok(()),
        _ => Err(error(format!("unsupported token `{text}`"), range)),
    }
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
            Limits::default(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_raw_identifier_and_block_comment() {
        assert!(validate("fn r#main() {}", Limits::default()).is_err());
        assert!(validate("fn main() { /* no */ }", Limits::default()).is_err());
    }

    #[test]
    fn rejects_literal_variants() {
        for source in [
            "fn main(){1;}",
            "fn main(){1_u64;}",
            "fn main(){0x1_i64;}",
            "fn main(){\"x\";}",
        ] {
            assert!(validate(source, Limits::default()).is_err(), "{source}");
        }
    }
}
