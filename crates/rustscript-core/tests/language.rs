#![forbid(unsafe_code)]

use rustscript_core::{
    Diagnostic, Limits, ParseLimits, ParsedProgram, Phase, RunResult, check, check_source, format,
    format_program, parse, parse_bytes, run,
};

fn frontend_error(result: Result<ParsedProgram, Diagnostic>, context: &str) -> Diagnostic {
    match result {
        Ok(_) => panic!("expected frontend failure: {context}"),
        Err(error) => error,
    }
}

fn run_program(source: &str) -> RunResult {
    let program = check_source(source, ParseLimits::default())
        .unwrap_or_else(|error| panic!("source must check: {source}: {error:?}"));
    run(&program, Limits::default())
        .unwrap_or_else(|error| panic!("source must run: {source}: {:?}", error.diagnostic))
}

#[test]
fn supported_profile_constructs_check() {
    let valid = [
        "fn main() {}",
        "fn unit() -> () { () } fn main() { unit(); }",
        "fn choose(value: bool) -> i64 { if value { 1_i64 } else { 2_i64 } } fn main() { println!(\"{}\", choose(true)); }",
        "fn calls() {} fn main() { calls(); let y = { let x = 1_i64; x }; y; }",
        "fn main() { let a = -1_i64; let b = !false; let c = 2_i64 * 3_i64 / 1_i64 % 2_i64 + 4_i64 - 1_i64; let d = 1_i64 < 2_i64; let e = 1_i64 <= 2_i64; let f = 2_i64 > 1_i64; let g = 2_i64 >= 1_i64; let h = 1_i64 == 1_i64; let i = true != false; let j = true && false || true; }",
        "fn main() { let mut x: i64 = 0_i64; while x < 4_i64 { x = x + 1_i64; if x == 2_i64 { continue; } else { () }; if x == 3_i64 { break; } else { () }; } }",
        "fn recursive(x: i64) -> i64 { if x == 0_i64 { 0_i64 } else { recursive(x - 1_i64) } } fn main() { println!(\"{}\", recursive(4_i64)); }",
        "fn exits() -> i64 { return 1_i64; true; } fn main() { println!(\"{}\", exits()); }",
        "fn main() { let x = 1_i64; let x: i64 = x + 1_i64; println!(\"{}\", x); }",
        "fn early(value: bool) -> i64 { if value { return 1_i64; } else { () }; 2_i64 } fn main() { println!(\"{}\", early(true)); }",
    ];
    for source in valid {
        assert!(
            check_source(source, ParseLimits::default()).is_ok(),
            "{source}"
        );
    }
}

#[test]
fn required_rejection_corpus_is_rejected_with_spans() {
    // Each entry pairs a rejected program with a fragment the diagnostic span
    // must cover, so spans point at the offending construct rather than merely
    // existing.
    let invalid = [
        (
            "fn main() { let value: bool = 1_i64; }",
            "let value: bool = 1_i64",
        ),
        (
            "fn main() { let value: i64 = 1_i64; value = 2_i64; }",
            "value",
        ),
        ("fn main() { println!(\"{}\", missing); }", "missing"),
        (
            "fn identity(value: i64) -> i64 { value } fn main() { println!(\"{}\", identity()); }",
            "identity()",
        ),
        (
            "fn main() { let value: i64 = 1_i64; let reference = &value; }",
            "&",
        ),
        (
            "fn identity<T>(value: T) -> T { value } fn main() {}",
            "identity<T>",
        ),
        ("fn main() { let text = String::new(); }", "String::new"),
        ("fn main() { println!(\"{:?}\", 1_i64); }", "{:?}"),
        (
            "fn main() { println!(\"{}\", 1_i64 < 2_i64 < 3_i64); }",
            "1_i64 < 2_i64 < 3_i64",
        ),
        (
            "fn main() { let value: i64 = if true { 1_i64 }; }",
            "if true { 1_i64 }",
        ),
    ];
    for (source, expected) in invalid {
        let error = check_source(source, ParseLimits::default()).expect_err(source);
        let span = error
            .span
            .unwrap_or_else(|| panic!("missing span for {source}: {error:?}"));
        let sliced = &source[span.start..span.end];
        assert!(
            !sliced.is_empty(),
            "empty span slice for {source}: {error:?}"
        );
        assert!(
            sliced.contains(expected),
            "span slice {sliced:?} for {source} must cover {expected:?}"
        );
    }
}

#[test]
fn format_program_canonicalizes_parsed_but_type_invalid_sources() {
    let source = "fn main( ) { // discard me\n let value: bool = 1_i64; }";
    let parsed = parse(source, ParseLimits::default()).expect("source must parse");
    assert!(check(&parsed).is_err());

    let canonical = format_program(&parsed);
    assert_eq!(canonical, "fn main() {\n    let value: bool = 1_i64;\n}\n");
    let reparsed = parse(&canonical, ParseLimits::default()).expect("canonical source must parse");
    assert_eq!(format_program(&reparsed), canonical);
}

#[test]
fn rejects_profile_edge_cases() {
    let invalid = [
        "fn helper(x: i64,) {} fn main() {}",
        "fn helper(x: i64) {} fn main() { helper(1_i64,); }",
        "fn main() { while false {}; }",
        "fn main() { while false { 1_i64 } }",
        "fn main() { if true { () } else { () } let x = 1_i64; }",
        "fn main() { println!(\"{}\", 1_i64,); }",
        "fn main() { println!(\"{}\", 1_i64;); }",
        "fn main() { println!(\"{}\", 1_i64; println!(\"{}\", 2_i64)); }",
        "fn main() { let value = println!(\"{}\", 1_i64); }",
        "fn main() { println!(\"{}\", ()); }",
        "fn exits() -> i64 { return 1_i64; true } fn main() {}",
        "fn main() { let x: i64<i64> = 1_i64; }",
        "fn helper() {} fn main() { let helper = 1_i64; }",
        "fn helper(helper: i64) {} fn main() {}",
        "fn helper() { main(); } fn main() {}",
        "fn main() { let raw = 1_i64; }",
        "fn main() { let r#value = 1_i64; }",
        "fn main() { /// docs\n }",
        "fn main() { /* block */ }",
        "fn g(x: ()) -> i64 { 0_i64 } fn f() -> bool { g({ return true; }) } fn main() {}",
        "fn g(x: ()) -> bool { true } fn f() -> i64 { false && g({ return 1_i64; }) } fn main() { println!(\"{}\", f()); }",
        "fn g(x: ()) -> i64 { 0_i64 } fn main() { while false { g({ return; }) } }",
        "fn f() -> i64 { return 1_i64; while false {} } fn main() {}",
        "fn g(x: ()) -> bool { true } fn f() -> i64 { while g({ return 1_i64; }) {} let x = (); } fn main() {}",
    ];
    for source in invalid {
        assert!(
            check_source(source, ParseLimits::default()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn frontend_limits_and_encoding_are_structured() {
    let invalid_utf8 = frontend_error(
        parse_bytes(&[0xff], ParseLimits::default()),
        "invalid UTF-8",
    );
    assert_eq!(invalid_utf8.phase, Phase::Lex);
    let non_ascii = frontend_error(
        parse("fn main() { // é\n }", ParseLimits::default()),
        "non-ASCII",
    );
    assert_eq!(non_ascii.phase, Phase::Lex);

    let limits = ParseLimits {
        max_source_bytes: 4,
        ..ParseLimits::default()
    };
    assert_eq!(
        frontend_error(parse("fn main() {}", limits), "size limit").phase,
        Phase::Lex
    );

    let limits = ParseLimits {
        max_tokens: 3,
        ..ParseLimits::default()
    };
    assert!(parse("fn main() {}", limits).is_err());

    let limits = ParseLimits {
        max_delimiter_depth: 2,
        ..ParseLimits::default()
    };
    assert!(parse("fn main() { ((1_i64)); }", limits).is_err());

    let chained_addition = (0..96).map(|_| "1_i64").collect::<Vec<_>>().join(" + ");
    let print_source = format!("fn main() {{ println!(\"{{}}\", {chained_addition}); }}");
    let limits = ParseLimits {
        max_syntax_depth: 64,
        ..ParseLimits::default()
    };
    let wrapped_error = frontend_error(
        parse(&print_source, limits),
        "println value expression syntax depth",
    );
    assert_eq!(wrapped_error.phase, Phase::Parse);
    assert_eq!(wrapped_error.message, "syntax nesting limit exceeded");
}

#[test]
fn arithmetic_traps_and_runtime_limits_are_structured() {
    let traps = [
        "fn main() { 9223372036854775807_i64 + 1_i64; }",
        "fn main() { -9223372036854775807_i64 - 2_i64; }",
        "fn main() { 3037000500_i64 * 3037000500_i64; }",
        "fn main() { 1_i64 / 0_i64; }",
        "fn main() { 1_i64 % 0_i64; }",
    ];
    for source in traps {
        let program = check_source(source, ParseLimits::default()).expect("trap source must check");
        let error = run(&program, Limits::default()).expect_err(source);
        assert_eq!(error.diagnostic.phase, Phase::Runtime);
        assert_eq!(
            error.stdout, b"",
            "{source} must print nothing before the trap"
        );
    }

    let recursive = check_source(
        "fn recurse() { recurse(); } fn main() { recurse(); }",
        ParseLimits::default(),
    )
    .expect("recursive source must check");
    let error = run(
        &recursive,
        Limits {
            maximum_call_depth: 4,
            ..Limits::default()
        },
    )
    .expect_err("call-depth limit");
    assert_eq!(error.diagnostic.message, "call depth limit exceeded");
}

#[test]
fn unreachable_locals_do_not_preallocate_every_recursive_frame() {
    let unreachable = (0..5_000)
        .map(|index| format!("let x{index} = 0_i64;"))
        .collect::<String>();
    let source = format!("fn recurse() {{ recurse(); {unreachable} }} fn main() {{ recurse(); }}");
    let program =
        check_source(&source, ParseLimits::default()).expect("recursive source must check");
    let error = run(
        &program,
        Limits {
            fuel: 10_000,
            maximum_call_depth: 64,
            maximum_output_bytes: 0,
        },
    )
    .expect_err("call depth must stop recursion");
    assert_eq!(error.diagnostic.message, "call depth limit exceeded");
}

#[test]
fn println_nested_runtime_spans_map_to_original_source() {
    let source = "fn main() { println!(\"{}\", 1_i64 + (2_i64 / 0_i64)); }";
    let program = check_source(source, ParseLimits::default()).expect("source must check");
    let error = run(&program, Limits::default()).expect_err("division must trap");
    let expected = "2_i64 / 0_i64";
    let start = source.find(expected).expect("nested expression");
    assert_eq!(
        error.diagnostic.span,
        Some(rustscript_core::Span {
            start,
            end: start + expected.len(),
        })
    );
}

#[test]
fn println_nested_admission_spans_map_to_original_source() {
    let source = "fn main() { println!(\"{}\", || true); }";
    let error = frontend_error(
        parse(source, ParseLimits::default()),
        "closure must be rejected",
    );
    let expected = "|| true";
    let start = source.find(expected).expect("closure expression");
    assert_eq!(
        error.span,
        Some(rustscript_core::Span {
            start,
            end: start + expected.len(),
        })
    );
}

#[test]
fn check_reports_a_diagnostic_for_each_broken_function_body() {
    let source = "fn first() { let wrong: bool = 1_i64; } fn second() -> i64 { true } fn main() { first(); second(); }";
    let parsed = parse(source, ParseLimits::default()).expect("source must parse");
    let diagnostics = check(&parsed).expect_err("both bodies are broken");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    for diagnostic in &diagnostics {
        assert_eq!(diagnostic.phase, Phase::Type, "{diagnostic:?}");
    }
    let starts: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.span.expect("body diagnostic span").start)
        .collect();
    assert!(
        starts[0] < starts[1],
        "diagnostics must be in source order: {starts:?}"
    );
    assert!(
        source[starts[0]..].starts_with("let wrong"),
        "{diagnostics:?}"
    );
}

#[test]
fn comparison_chains_are_rejected_at_admission() {
    let source = "fn main() { let value = 1_i64 < 2_i64 < 3_i64; }";
    let error = frontend_error(parse(source, ParseLimits::default()), "comparison chain");
    assert_eq!(error.phase, Phase::Parse);
    assert_eq!(error.message, "comparison chains are unsupported");
    let span = error.span.expect("comparison chain span");
    assert_eq!(&source[span.start..span.end], "1_i64 < 2_i64 < 3_i64");

    // A parenthesized sub-comparison is not a chain; it fails later on
    // operand types instead.
    let parenthesized = "fn main() { let value = (1_i64 < 2_i64) < 3_i64; }";
    assert!(
        parse(parenthesized, ParseLimits::default()).is_ok(),
        "{parenthesized}"
    );
    let error = check_source(parenthesized, ParseLimits::default()).expect_err(parenthesized);
    assert_eq!(error.phase, Phase::Type);
}

#[test]
fn reserved_identifiers_are_rejected_at_admission() {
    for (source, expected) in [
        ("fn main() { let raw = 1_i64; }", "raw"),
        ("fn main() { let bool = true; }", "bool"),
        ("fn println() {} fn main() {}", "println"),
        ("fn helper(main: i64) {} fn main() {}", "main"),
    ] {
        let error = frontend_error(parse(source, ParseLimits::default()), source);
        assert_eq!(error.phase, Phase::Parse, "{source}");
        assert_eq!(error.message, "reserved identifier", "{source}");
        let span = error
            .span
            .unwrap_or_else(|| panic!("missing span for {source}"));
        assert_eq!(&source[span.start..span.end], expected, "{source}");
    }
}

#[test]
fn parsed_and_checked_emitters_agree_and_are_idempotent() {
    let sources = [
        "fn main() {}",
        "fn main() { while false {} }",
        "fn main() { println!(\"{}\", 1_i64 + 2_i64 - 3_i64 * 4_i64 / 5_i64 % 6_i64); }",
        "fn main() { println!(\"{}\", 1_i64 - -2_i64); }",
        "fn main() { let a = 1_i64 < 2_i64; let b = 1_i64 <= 2_i64; let c = 1_i64 > 2_i64; let d = 1_i64 >= 2_i64; let e = 1_i64 == 2_i64; let f = 1_i64 != 2_i64; let g = a == b; let h = c != d; println!(\"{}\", e && f || !(g && h)); }",
        "fn main() { let mut total: i64 = 0_i64; while total < 3_i64 { total = total + 1_i64; } println!(\"{}\", total); }",
        "fn unit() -> () { () } fn add(x: i64, y: i64) -> i64 { x + y } fn main() { unit(); println!(\"{}\", add(1_i64, 2_i64)); }",
        "fn main() { let x = if true { 1_i64 } else { 2_i64 }; println!(\"{}\", x); }",
        "fn main() { println!(\"{}\", ((((1_i64))))); }",
        "fn main() { let value = { let inner = -1_i64; !false; inner }; println!(\"{}\", value); }",
        "fn early() -> i64 { return 1_i64; } fn main() { while false { break; } while false { continue; } println!(\"{}\", early()); }",
    ];
    for source in sources {
        let parsed = parse(source, ParseLimits::default())
            .unwrap_or_else(|error| panic!("source must parse: {source}: {error:?}"));
        let canonical = format_program(&parsed);
        let checked = check_source(source, ParseLimits::default())
            .unwrap_or_else(|error| panic!("source must check: {source}: {error:?}"));
        assert_eq!(
            canonical,
            format(&checked),
            "emitters must agree for {source}"
        );

        let reparsed = parse(&canonical, ParseLimits::default())
            .unwrap_or_else(|error| panic!("canonical must parse: {canonical}: {error:?}"));
        assert_eq!(
            format_program(&reparsed),
            canonical,
            "formatting must be idempotent for {source}"
        );
    }
}

#[test]
fn interpreter_operator_results_print_exact_bytes() {
    let cases: &[(&str, &[u8])] = &[
        ("2_i64 * 3_i64", b"6\n"),
        ("7_i64 / 2_i64", b"3\n"),
        ("7_i64 % 2_i64", b"1\n"),
        ("1_i64 + 2_i64", b"3\n"),
        ("5_i64 - 2_i64", b"3\n"),
        ("-5_i64", b"-5\n"),
        ("-(-5_i64)", b"5\n"),
        ("1_i64 - -2_i64", b"3\n"),
        ("!true", b"false\n"),
        ("!false", b"true\n"),
        ("1_i64 == 1_i64", b"true\n"),
        ("1_i64 == 2_i64", b"false\n"),
        ("1_i64 != 1_i64", b"false\n"),
        ("1_i64 != 2_i64", b"true\n"),
        ("true == true", b"true\n"),
        ("true == false", b"false\n"),
        ("true != false", b"true\n"),
        ("false != false", b"false\n"),
        ("1_i64 < 2_i64", b"true\n"),
        ("2_i64 < 2_i64", b"false\n"),
        ("2_i64 <= 2_i64", b"true\n"),
        ("3_i64 <= 2_i64", b"false\n"),
        ("3_i64 > 2_i64", b"true\n"),
        ("2_i64 > 2_i64", b"false\n"),
        ("2_i64 >= 2_i64", b"true\n"),
        ("1_i64 >= 2_i64", b"false\n"),
        ("true && true", b"true\n"),
        ("true && false", b"false\n"),
        ("false || false", b"false\n"),
        ("false || true", b"true\n"),
    ];
    for (expression, expected) in cases {
        let source = format!("fn main() {{ println!(\"{{}}\", {expression}); }}");
        let result = run_program(&source);
        assert_eq!(result.stdout, *expected, "{expression}");
    }
}

#[test]
fn negative_values_print_with_a_sign() {
    let result = run_program("fn main() { println!(\"{}\", -5_i64); }");
    assert_eq!(result.stdout, b"-5\n");
}

#[test]
fn division_and_remainder_truncate_toward_zero() {
    let cases: &[(&str, &[u8])] = &[
        ("-7_i64 / 2_i64", b"-3\n"),
        ("-7_i64 % 2_i64", b"-1\n"),
        ("7_i64 / -2_i64", b"-3\n"),
        ("7_i64 % -2_i64", b"1\n"),
    ];
    for (expression, expected) in cases {
        let source = format!("fn main() {{ println!(\"{{}}\", {expression}); }}");
        let result = run_program(&source);
        assert_eq!(result.stdout, *expected, "{expression}");
    }
}

#[test]
fn i64_min_negation_division_and_remainder_trap() {
    let traps = [
        "fn main() { let min = -9223372036854775807_i64 - 1_i64; println!(\"{}\", min / -1_i64); }",
        "fn main() { let min = -9223372036854775807_i64 - 1_i64; println!(\"{}\", min % -1_i64); }",
        "fn main() { let min = -9223372036854775807_i64 - 1_i64; println!(\"{}\", -min); }",
    ];
    for source in traps {
        let program = check_source(source, ParseLimits::default())
            .unwrap_or_else(|error| panic!("trap source must check: {source}: {error:?}"));
        let error = run(&program, Limits::default()).expect_err(source);
        assert_eq!(error.diagnostic.phase, Phase::Runtime, "{source}");
        assert_eq!(
            error.stdout, b"",
            "{source} must print nothing before the trap"
        );
    }
}

#[test]
fn empty_output_program_produces_no_bytes() {
    let result = run_program("fn main() {}");
    assert_eq!(result.stdout, b"");
}

#[test]
fn golden_step_count_pins_fuel_accounting() {
    // The exact step count guards fuel accounting: any change to how the
    // evaluator charges steps must consciously update this constant.
    let result = run_program("fn main() { println!(\"{}\", 1_i64 + 2_i64); }");
    assert_eq!(result.stdout, b"3\n");
    assert_eq!(result.steps, 5);
}

#[test]
fn nested_parentheses_round_trip_canonically() {
    let source = "fn main() { let value = ((((1_i64)))); println!(\"{}\", value); }";
    let checked = check_source(source, ParseLimits::default()).expect("source must check");
    let canonical = format(&checked);
    assert!(
        canonical.contains("let value = 1_i64;"),
        "parens must not survive around literals: {canonical}"
    );
    let parsed = parse(source, ParseLimits::default()).expect("source must parse");
    assert_eq!(format_program(&parsed), canonical);

    let rechecked = check_source(&canonical, ParseLimits::default()).expect("canonical must check");
    assert_eq!(
        format(&rechecked),
        canonical,
        "canonical form must be a fixed point"
    );
    let roundtripped =
        check_source(&format(&rechecked), ParseLimits::default()).expect("round trip must check");
    assert_eq!(
        roundtripped, rechecked,
        "reparse must reproduce structurally equal IR"
    );
}
