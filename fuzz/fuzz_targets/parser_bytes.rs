#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;

// A `---` frontmatter block, optionally preceded by a single shebang line,
// triggers the pinned rust-analyzer frontmatter lexer panic.
fn triggers_frontmatter(text: &str) -> bool {
    let after_shebang = match text.strip_prefix("#!") {
        Some(rest) => rest.split_once('\n').map_or(rest, |(_, body)| body),
        None => text,
    };
    after_shebang.trim_start().starts_with("---")
}

fuzz_target!(|data: &[u8]| {
    let limits = rustscript_core::ParseLimits {
        max_source_bytes: 64 * 1024,
        max_tokens: 10_000,
        max_syntax_elements: 20_000,
        ..rustscript_core::ParseLimits::default()
    };

    // Known dependency crash (pinned ra_ap_parser 0.0.342, documented in
    // docs/regression-corpus.md): frontmatter — a `---` block at the start,
    // optionally after a shebang line — drives the Edition 2024 frontmatter
    // probe to panic inside `LexedStr::new`. The production path contains
    // that unwind as a `Lex` diagnostic (see the containment test in
    // rustscript-core), but this fuzz binary builds with `panic = abort`, so
    // `catch_unwind` cannot save it here — the whole iteration is skipped.
    // Remove when the rust-analyzer pin advances and re-run the regression
    // input `fuzz/regressions/parser_bytes/frontmatter_lexer_panic.txt`.
    if std::str::from_utf8(data).is_ok_and(triggers_frontmatter) {
        return;
    }

    // Exercise rust-analyzer's own tree invariants on every bounded, valid
    // UTF-8 input — including non-ASCII text that rustscript itself rejects.
    // A panic here is a dependency fuzz finding. `{#}` (attribute recovery)
    // is a second known 0.0.342 check_parser crash the product path rejects
    // at the token policy; inputs containing `#` skip only this call.
    if let Ok(text) = std::str::from_utf8(data)
        && text.len() <= limits.max_source_bytes
        && !data.contains(&b'#')
    {
        ra_ap_syntax::fuzz::check_parser(text);
    }
    if data.len() > limits.max_source_bytes
        || std::str::from_utf8(data).is_err()
        || data.iter().any(|byte| !byte.is_ascii())
    {
        assert!(rustscript_core::parse_bytes(data, limits).is_err());
        return;
    }
    let Ok(parsed) = rustscript_core::parse_bytes(data, limits) else {
        return;
    };
    let parsed_canonical = rustscript_core::format_program(&parsed);
    let parsed_round_trip =
        rustscript_core::parse(&parsed_canonical, rustscript_core::ParseLimits::default())
            .expect("canonical parsed program must reparse");
    assert_eq!(
        parsed_canonical,
        rustscript_core::format_program(&parsed_round_trip)
    );
    let Ok(checked) = rustscript_core::check(&parsed) else {
        return;
    };
    let checked_round_trip =
        rustscript_core::check(&parsed_round_trip).expect("canonical parsed program must check");
    assert!(checked.structurally_eq(&checked_round_trip));
    let canonical = rustscript_core::format(&checked);
    let reparsed = rustscript_core::check_source(&canonical, limits)
        .expect("canonical checked IR must reparse");
    assert!(checked.structurally_eq(&reparsed));
    assert_eq!(canonical, rustscript_core::format(&reparsed));
    let runtime = rustscript_core::Limits {
        fuel: 10_000,
        maximum_call_depth: 32,
        maximum_output_bytes: 4096,
    };
    let _ = rustscript_core::run(&reparsed, runtime);
});
