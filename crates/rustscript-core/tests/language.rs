#![forbid(unsafe_code)]

use rustscript_core::{
    Diagnostic, Limits, ParsedProgram, Phase, RuntimeLimits, check, check_source, format_program,
    parse, parse_bytes, run,
};

fn frontend_error(result: Result<ParsedProgram, Diagnostic>, context: &str) -> Diagnostic {
    match result {
        Ok(_) => panic!("expected frontend failure: {context}"),
        Err(error) => error,
    }
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
        assert!(check_source(source, Limits::default()).is_ok(), "{source}");
    }
}

#[test]
fn required_rejection_corpus_is_rejected_with_spans() {
    let invalid = [
        "fn main() { let value: bool = 1_i64; }",
        "fn main() { let value: i64 = 1_i64; value = 2_i64; }",
        "fn main() { println!(\"{}\", missing); }",
        "fn identity(value: i64) -> i64 { value } fn main() { println!(\"{}\", identity()); }",
        "fn main() { let value: i64 = 1_i64; let reference = &value; }",
        "fn identity<T>(value: T) -> T { value } fn main() {}",
        "fn main() { let text = String::new(); }",
        "fn main() { println!(\"{:?}\", 1_i64); }",
        "fn main() { println!(\"{}\", 1_i64 < 2_i64 < 3_i64); }",
        "fn main() { let value: i64 = if true { 1_i64 }; }",
    ];
    for source in invalid {
        let error = check_source(source, Limits::default()).expect_err(source);
        assert!(error.span.is_some(), "missing span for {source}: {error:?}");
    }
}

#[test]
fn format_program_canonicalizes_parsed_but_type_invalid_sources() {
    let source = "fn main( ) { // discard me\n let value: bool = 1_i64; }";
    let parsed = parse(source, Limits::default()).expect("source must parse");
    assert!(check(&parsed).is_err());

    let canonical = format_program(&parsed);
    assert_eq!(canonical, "fn main() {\n    let value: bool = 1_i64;\n}\n");
    let reparsed = parse(&canonical, Limits::default()).expect("canonical source must parse");
    assert_eq!(format_program(&reparsed), canonical);
}

#[test]
fn rejects_profile_edge_cases() {
    let invalid = [
        "fn helper(x: i64,) {} fn main() {}",
        "fn helper(x: i64) {} fn main() { helper(1_i64,); }",
        "fn main() { while false {}; }",
        "fn main() { if true { () } else { () } let x = 1_i64; }",
        "fn main() { println!(\"{}\", 1_i64,); }",
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
    ];
    for source in invalid {
        assert!(check_source(source, Limits::default()).is_err(), "{source}");
    }
}

#[test]
fn frontend_limits_and_encoding_are_structured() {
    let invalid_utf8 = frontend_error(parse_bytes(&[0xff], Limits::default()), "invalid UTF-8");
    assert_eq!(invalid_utf8.phase, Phase::Lex);
    let non_ascii = frontend_error(
        parse("fn main() { // é\n }", Limits::default()),
        "non-ASCII",
    );
    assert_eq!(non_ascii.phase, Phase::Lex);

    let limits = Limits {
        max_source_bytes: 4,
        ..Limits::default()
    };
    assert_eq!(
        frontend_error(parse("fn main() {}", limits), "size limit").phase,
        Phase::Lex
    );

    let limits = Limits {
        max_tokens: 3,
        ..Limits::default()
    };
    assert!(parse("fn main() {}", limits).is_err());

    let limits = Limits {
        max_delimiter_depth: 2,
        ..Limits::default()
    };
    assert!(parse("fn main() { ((1_i64)); }", limits).is_err());

    let chained_addition = (0..96).map(|_| "1_i64").collect::<Vec<_>>().join(" + ");
    let print_source = format!("fn main() {{ println!(\"{{}}\", {chained_addition}); }}");
    let limits = Limits {
        max_syntax_depth: 64,
        ..Limits::default()
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
        let program = check_source(source, Limits::default()).expect("trap source must check");
        let error = run(&program, RuntimeLimits::default()).expect_err(source);
        assert_eq!(error.phase, Phase::Runtime);
    }

    let recursive = check_source(
        "fn recurse() { recurse(); } fn main() { recurse(); }",
        Limits::default(),
    )
    .expect("recursive source must check");
    let error = run(
        &recursive,
        RuntimeLimits {
            max_call_depth: 4,
            ..RuntimeLimits::default()
        },
    )
    .expect_err("call-depth limit");
    assert_eq!(error.message, "call depth limit exceeded");
}
