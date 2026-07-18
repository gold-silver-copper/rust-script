#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = rustscript_core::Limits {
        max_source_bytes: 64 * 1024,
        max_tokens: 10_000,
        max_syntax_elements: 20_000,
        ..rustscript_core::Limits::default()
    };
    if data.len() > limits.max_source_bytes
        || std::str::from_utf8(data).is_err()
        || data.iter().any(|byte| !byte.is_ascii())
    {
        assert!(rustscript_core::parse_bytes(data, limits).is_err());
        return;
    }
    if let Ok(text) = std::str::from_utf8(data)
        && text.len() <= limits.max_source_bytes
    {
        ra_ap_syntax::fuzz::check_parser(text);
    }
    let Ok(parsed) = rustscript_core::parse_bytes(data, limits) else {
        return;
    };
    let parsed_canonical = rustscript_core::format_program(&parsed);
    let parsed_round_trip =
        rustscript_core::parse(&parsed_canonical, rustscript_core::Limits::default())
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
    let runtime = rustscript_core::RuntimeLimits {
        fuel: 10_000,
        max_call_depth: 32,
        max_output_bytes: 4096,
    };
    let _ = rustscript_core::run(&reparsed, runtime);
});
