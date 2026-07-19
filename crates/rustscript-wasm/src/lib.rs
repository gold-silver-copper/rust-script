#![forbid(unsafe_code)]
#![doc = "WASM adapter for the rustscript semantic engine."]

use rustscript_core::{Diagnostic, ParseLimits, Location, Limits};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[derive(Clone, Copy, Deserialize, Serialize, Default)]
struct Options {
    #[serde(default)]
    frontend: ParseLimits,
    #[serde(default)]
    runtime: Limits,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    steps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<WasmDiagnostic>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct WasmDiagnostic {
    #[serde(flatten)]
    diagnostic: Diagnostic,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<Location>,
}

impl Response {
    fn success(text: Option<String>, output: Option<Vec<u8>>, steps: Option<u64>) -> Self {
        Self {
            ok: true,
            output,
            steps,
            text,
            error: None,
        }
    }

    fn failure(location: Option<Location>, diagnostic: Diagnostic) -> Self {
        Self {
            ok: false,
            output: None,
            steps: None,
            text: None,
            error: Some(WasmDiagnostic {
                diagnostic,
                location,
            }),
        }
    }

    /// A runtime failure keeps the partial stdout and step count produced
    /// before the fault, matching native prefix output before a trap.
    fn runtime_failure(location: Option<Location>, failure: rustscript_core::RuntimeDiagnostic) -> Self {
        Self {
            ok: false,
            output: Some(failure.stdout),
            steps: Some(failure.steps),
            text: None,
            error: Some(WasmDiagnostic {
                diagnostic: failure.diagnostic,
                location,
            }),
        }
    }
}

#[wasm_bindgen]
/// Check source and return a structured JavaScript response.
pub fn check(source: &str, options: JsValue) -> Result<JsValue, JsValue> {
    respond(source, options, Operation::Check)
}

#[wasm_bindgen]
/// Run source and return stdout bytes, steps, or a structured diagnostic.
pub fn run(source: &str, options: JsValue) -> Result<JsValue, JsValue> {
    respond(source, options, Operation::Run)
}

#[wasm_bindgen]
/// Return the pinned rust-analyzer syntax debug tree for admitted source.
pub fn ast(source: &str, options: JsValue) -> Result<JsValue, JsValue> {
    respond(source, options, Operation::Ast)
}

#[wasm_bindgen]
/// Return canonical rustscript formatting for admitted source.
pub fn format(source: &str, options: JsValue) -> Result<JsValue, JsValue> {
    respond(source, options, Operation::Format)
}

enum Operation {
    Check,
    Run,
    Ast,
    Format,
}

fn respond(source: &str, options: JsValue, operation: Operation) -> Result<JsValue, JsValue> {
    let options = match decode_options(options) {
        Ok(options) => options,
        Err(error) => {
            let response = Response::failure(
                None,
                Diagnostic {
                    phase: rustscript_core::Phase::Parse,
                    message: format!("invalid options: {error}"),
                    span: None,
                    file_name: None,
                },
            );
            return serialize_response(&response);
        }
    };
    let response = match rustscript_core::parse(source, options.frontend) {
        Err(error) => Response::failure(rustscript_core::locate(source, &error), error),
        Ok(parsed) => match operation {
            Operation::Ast => {
                Response::success(Some(rustscript_core::debug_syntax(&parsed)), None, None)
            }
            Operation::Format => {
                Response::success(Some(rustscript_core::format_program(&parsed)), None, None)
            }
            Operation::Check | Operation::Run => match rustscript_core::check(&parsed) {
                Err(errors) => errors.into_iter().next().map_or_else(
                    || {
                        Response::failure(
                            None,
                            Diagnostic {
                                phase: rustscript_core::Phase::Type,
                                message: "checker returned no diagnostic".into(),
                                span: None,
                                file_name: None,
                            },
                        )
                    },
                    |error| Response::failure(parsed.location(&error), error),
                ),
                Ok(program) => match operation {
                    Operation::Check => Response::success(None, None, None),
                    Operation::Run => match rustscript_core::run(&program, options.runtime) {
                        Ok(execution) => {
                            Response::success(None, Some(execution.stdout), Some(execution.steps))
                        }
                        Err(failure) => {
                            let location = parsed.location(&failure.diagnostic);
                            Response::runtime_failure(location, failure)
                        }
                    },
                    Operation::Ast => {
                        Response::success(Some(rustscript_core::debug_syntax(&parsed)), None, None)
                    }
                    Operation::Format => Response::success(
                        Some(rustscript_core::format_program(&parsed)),
                        None,
                        None,
                    ),
                },
            },
        },
    };
    serialize_response(&response)
}

fn decode_options(options: JsValue) -> Result<Options, serde_wasm_bindgen::Error> {
    if options.is_null() || options.is_undefined() {
        Ok(Options::default())
    } else {
        serde_wasm_bindgen::from_value(options)
    }
}

fn serialize_response(response: &Response) -> Result<JsValue, JsValue> {
    response
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(js_error)
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    fn response(value: JsValue) -> Response {
        serde_wasm_bindgen::from_value(value).expect("response must deserialize")
    }

    #[wasm_bindgen_test]
    fn repeatedly_parses_checks_and_drops() {
        for _ in 0..100 {
            assert!(response(check("fn main() {}", JsValue::NULL).unwrap()).ok);
            assert!(!response(check("fn main( {}", JsValue::NULL).unwrap()).ok);
        }
    }

    #[wasm_bindgen_test]
    fn executes_formats_and_is_deterministic() {
        let source = "fn main() { let mut x = 0_i64; while x < 3_i64 { x = x + 1_i64; } println!(\"{}\", x); }";
        let first = response(run(source, JsValue::NULL).unwrap());
        let second = response(run(source, JsValue::NULL).unwrap());
        assert!(first.ok);
        assert_eq!(first.output, Some(b"3\n".to_vec()));
        assert_eq!(first.output, second.output);
        assert_eq!(first.steps, second.steps);
        let formatted = response(format(source, JsValue::NULL).unwrap());
        assert!(formatted.ok);
        assert!(formatted.text.is_some());
        let formatted_type_invalid =
            response(format("fn main(){let value: bool = 1_i64;}", JsValue::NULL).unwrap());
        assert_eq!(
            formatted_type_invalid.text,
            Some("fn main() {\n    let value: bool = 1_i64;\n}\n".into())
        );
        let syntax = response(ast(source, JsValue::NULL).unwrap());
        assert!(syntax.text.is_some_and(|text| text.contains("SOURCE_FILE")));
    }

    #[wasm_bindgen_test]
    fn returns_structured_diagnostics_and_enforces_limits() {
        let invalid = response(check("fn main() {\n missing;\n}", JsValue::NULL).unwrap());
        let diagnostic = invalid.error.expect("diagnostic");
        assert_eq!(diagnostic.diagnostic.phase, rustscript_core::Phase::Type);
        assert_eq!(diagnostic.location, Some(Location { line: 2, column: 2 }));

        let fuel_options = serde_wasm_bindgen::to_value(&Options {
            frontend: ParseLimits::default(),
            runtime: Limits {
                fuel: 8,
                ..Limits::default()
            },
        })
        .unwrap();
        let fuel = response(run("fn main() { while true {} }", fuel_options).unwrap());
        assert_eq!(
            fuel.error.expect("fuel error").diagnostic.message,
            "fuel exhausted"
        );

        let output_options = serde_wasm_bindgen::to_value(&Options {
            frontend: ParseLimits::default(),
            runtime: Limits {
                maximum_output_bytes: 1,
                ..Limits::default()
            },
        })
        .unwrap();
        let output =
            response(run("fn main() { println!(\"{}\", true); }", output_options).unwrap());
        assert_eq!(
            output.error.expect("output error").diagnostic.message,
            "output byte limit exceeded"
        );

        let invalid_options = response(check("fn main() {}", JsValue::from_str("bad")).unwrap());
        assert!(!invalid_options.ok);
        assert!(
            invalid_options
                .error
                .expect("options error")
                .diagnostic
                .message
                .starts_with("invalid options:")
        );
    }
}
