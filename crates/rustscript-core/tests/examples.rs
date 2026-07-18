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
