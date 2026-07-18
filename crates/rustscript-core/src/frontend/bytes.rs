use crate::{Diagnostic, Limits, Phase, Span};

pub(super) fn validate(bytes: &[u8], limits: Limits) -> Result<&str, Diagnostic> {
    if bytes.len() > limits.max_source_bytes {
        return Err(Diagnostic::new(
            Phase::Lex,
            "source byte limit exceeded",
            Some(Span {
                start: limits.max_source_bytes,
                end: bytes.len(),
            }),
        ));
    }
    let source = std::str::from_utf8(bytes).map_err(|error| {
        let start = error.valid_up_to();
        Diagnostic::new(
            Phase::Lex,
            "source is not valid UTF-8",
            Some(Span {
                start,
                end: start
                    .saturating_add(error.error_len().unwrap_or(1))
                    .min(bytes.len()),
            }),
        )
    })?;
    if let Some((start, ch)) = source.char_indices().find(|(_, ch)| !ch.is_ascii()) {
        return Err(Diagnostic::new(
            Phase::Lex,
            "source must contain only ASCII",
            Some(Span {
                start,
                end: start + ch.len_utf8(),
            }),
        ));
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_utf8() {
        let error = validate(&[0xff], Limits::default()).unwrap_err();
        assert_eq!(error.phase, Phase::Lex);
        assert_eq!(error.span, Some(Span { start: 0, end: 1 }));
    }

    #[test]
    fn rejects_non_ascii() {
        let error = validate("é".as_bytes(), Limits::default()).unwrap_err();
        assert_eq!(error.span, Some(Span { start: 0, end: 2 }));
    }
}
