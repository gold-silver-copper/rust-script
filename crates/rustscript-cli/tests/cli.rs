#![forbid(unsafe_code)]

use std::process::Command;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_rustscript")
}

fn source_file(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("program.rs");
    std::fs::write(&path, source).expect("source file");
    (directory, path)
}

#[test]
fn commands_follow_stdout_and_exit_contracts() {
    let (_directory, path) = source_file(include_str!("../../../examples/arithmetic.rs"));
    let check = Command::new(binary())
        .args(["check", path.to_str().expect("UTF-8 path")])
        .output()
        .expect("check command");
    assert!(check.status.success());
    assert!(check.stdout.is_empty());
    assert!(check.stderr.is_empty());

    let run = Command::new(binary())
        .args(["run", path.to_str().expect("UTF-8 path")])
        .output()
        .expect("run command");
    assert!(run.status.success());
    assert_eq!(run.stdout, b"42\n");
    assert!(run.stderr.is_empty());

    let ast = Command::new(binary())
        .args(["ast", path.to_str().expect("UTF-8 path")])
        .output()
        .expect("ast command");
    assert!(ast.status.success());
    assert!(String::from_utf8_lossy(&ast.stdout).contains("SOURCE_FILE"));

    let formatted = Command::new(binary())
        .args(["fmt", path.to_str().expect("UTF-8 path")])
        .output()
        .expect("fmt command");
    assert!(formatted.status.success());
    let expected = rustscript_core::format(
        &rustscript_core::check_source(
            include_str!("../../../examples/arithmetic.rs"),
            rustscript_core::Limits::default(),
        )
        .expect("example checks"),
    );
    assert_eq!(
        String::from_utf8(formatted.stdout).expect("formatted UTF-8"),
        expected
    );

    let (_directory, type_invalid) = source_file("fn main(){let value: bool = 1_i64;}");
    let formatted_invalid = Command::new(binary())
        .args(["fmt", type_invalid.to_str().expect("UTF-8 path")])
        .output()
        .expect("format type-invalid source");
    assert!(formatted_invalid.status.success());
    assert_eq!(
        String::from_utf8(formatted_invalid.stdout).expect("formatted UTF-8"),
        "fn main() {\n    let value: bool = 1_i64;\n}\n"
    );
}

#[test]
fn source_errors_exit_one_and_usage_errors_exit_two() {
    let (_directory, path) = source_file("fn main() { missing; }");
    let invalid = Command::new(binary())
        .args(["check", path.to_str().expect("UTF-8 path")])
        .output()
        .expect("invalid command");
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    assert!(!invalid.stderr.is_empty());

    let usage = Command::new(binary()).output().expect("usage command");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    assert!(!usage.stderr.is_empty());
}
