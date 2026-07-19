#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let decisions: Vec<u64> = data
        .chunks(8)
        .take(512)
        .map(|chunk| {
            let mut bytes = [0_u8; 8];
            bytes[..chunk.len()].copy_from_slice(chunk);
            u64::from_le_bytes(bytes)
        })
        .collect();
    let generated = rustscript_core::generate_checked_program(&decisions);
    let canonical = rustscript_core::format(&generated);
    let checked = rustscript_core::check_source(&canonical, rustscript_core::ParseLimits::default())
        .expect("generated source must check");
    assert!(generated.structurally_eq(&checked));
    assert_eq!(canonical, rustscript_core::format(&checked));
    let limits = rustscript_core::Limits {
        fuel: 100_000,
        maximum_call_depth: 32,
        maximum_output_bytes: 4096,
    };
    let generated_result = rustscript_core::run(&generated, limits);
    let checked_result = rustscript_core::run(&checked, limits);
    assert_eq!(generated_result, checked_result);
});
