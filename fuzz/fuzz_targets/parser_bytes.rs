#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = rustscript_core::ParseLimits {
        max_source_bytes: 64 * 1024,
        max_tokens: 10_000,
        max_syntax_elements: 20_000,
        ..rustscript_core::ParseLimits::default()
    };
    // Exercise rust-analyzer's own tree invariants on every bounded, valid
    // UTF-8 input — including non-ASCII text that rustscript itself rejects.
    // A panic here is a dependency fuzz finding.
    //
    // Known findings for the pinned 0.0.342 release, both documented in
    // docs/regression-corpus.md: `{#}` (attribute recovery) and a leading
    // `---` (Edition 2024 frontmatter probe) each trip a check_parser
    // invariant panic. Inputs containing `#`, or beginning with `---`, are
    // skipped here so long campaigns are not permanently blocked on these
    // known crashes; rustscript's own path rejects or contains both.
    // Remove these guards when the rust-analyzer pin advances and re-run the
    // regression inputs to revalidate.
    if let Ok(text) = std::str::from_utf8(data)
        && text.len() <= limits.max_source_bytes
        && !data.contains(&b'#')
        && !text.trim_start().starts_with("---")
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
