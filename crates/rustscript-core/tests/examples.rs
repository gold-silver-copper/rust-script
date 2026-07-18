#![forbid(unsafe_code)]

use rustscript_core::{Limits, RuntimeLimits, check_source, run};

const EXAMPLES: &[(&str, &str, &[u8])] = &[
    (
        "arithmetic",
        include_str!("../../../examples/arithmetic.rs"),
        b"42\n",
    ),
    (
        "evaluation_order",
        include_str!("../../../examples/evaluation_order.rs"),
        b"1\n2\n30\n",
    ),
    (
        "short_circuit",
        include_str!("../../../examples/short_circuit.rs"),
        b"false\ntrue\n",
    ),
    (
        "sum_loop",
        include_str!("../../../examples/sum_loop.rs"),
        b"55\n",
    ),
    ("gcd", include_str!("../../../examples/gcd.rs"), b"6\n"),
    (
        "shadowing",
        include_str!("../../../examples/shadowing.rs"),
        b"2\n",
    ),
];

#[test]
fn normative_examples_produce_exact_stdout() {
    for (name, source, expected) in EXAMPLES {
        let program = check_source(source, Limits::default())
            .unwrap_or_else(|error| panic!("{name} did not check: {error}"));
        let result = run(&program, RuntimeLimits::default())
            .unwrap_or_else(|error| panic!("{name} did not run: {error}"));
        assert_eq!(&result.stdout, expected, "{name}");
    }
}

#[test]
fn normative_examples_compile_with_rustc() {
    for (name, source, _) in EXAMPLES {
        let directory = tempfile::tempdir().expect("temporary directory must be available");
        let source_path = directory.path().join(format!("{name}.rs"));
        let executable = directory.path().join(name);
        std::fs::write(&source_path, source).expect("example source must be writable");
        let output = std::process::Command::new("rustc")
            .args([
                "--edition=2024",
                "-C",
                "overflow-checks=yes",
                "-A",
                "warnings",
            ])
            .arg(&source_path)
            .arg("-o")
            .arg(&executable)
            .output()
            .expect("rustc must be installed for the conformance tests");
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let native = std::process::Command::new(executable)
            .output()
            .expect("compiled example must execute");
        assert!(native.status.success(), "{name}");
        let expected = EXAMPLES
            .iter()
            .find(|(candidate, _, _)| candidate == name)
            .map(|(_, _, expected)| *expected)
            .expect("example expectation must exist");
        assert_eq!(native.stdout, expected, "{name}");
        assert!(native.stderr.is_empty(), "{name}");
    }
}
