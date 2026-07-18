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
    if let Ok(text) = std::str::from_utf8(data)
        && text.len() <= limits.max_source_bytes
    {
        ra_ap_syntax::fuzz::check_parser(text);
    }
    let Ok(parsed) = rustscript_core::parse_bytes(data, limits) else {
        return;
    };
    let Ok(checked) = rustscript_core::check(&parsed) else {
        return;
    };
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
