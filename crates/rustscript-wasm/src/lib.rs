#![forbid(unsafe_code)]

use rustscript_core::{Diagnostic, Limits, Location, RuntimeLimits};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[derive(Clone, Copy, Deserialize, Default)]
struct Options {
    #[serde(default)]
    frontend: Limits,
    #[serde(default)]
    runtime: RuntimeLimits,
}

#[derive(Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize)]
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

    fn failure(source: &str, diagnostic: Diagnostic) -> Self {
        let location = rustscript_core::locate(source, &diagnostic);
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
}

#[wasm_bindgen]
pub fn check(source: &str, options: JsValue) -> Result<JsValue, JsValue> {
    respond(source, options, Operation::Check)
}

#[wasm_bindgen]
pub fn run(source: &str, options: JsValue) -> Result<JsValue, JsValue> {
    respond(source, options, Operation::Run)
}

#[wasm_bindgen]
pub fn ast(source: &str, options: JsValue) -> Result<JsValue, JsValue> {
    respond(source, options, Operation::Ast)
}

#[wasm_bindgen]
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
    let options = if options.is_null() || options.is_undefined() {
        Options::default()
    } else {
        serde_wasm_bindgen::from_value(options).map_err(js_error)?
    };
    let response = match rustscript_core::check_source(source, options.frontend) {
        Err(error) => Response::failure(source, error),
        Ok(program) => match operation {
            Operation::Check => Response::success(None, None, None),
            Operation::Ast => {
                Response::success(Some(rustscript_core::debug_ir(&program)), None, None)
            }
            Operation::Format => {
                Response::success(Some(rustscript_core::format(&program)), None, None)
            }
            Operation::Run => match rustscript_core::run(&program, options.runtime) {
                Ok(execution) => {
                    Response::success(None, Some(execution.output), Some(execution.steps))
                }
                Err(error) => Response::failure(source, error),
            },
        },
    };
    serde_wasm_bindgen::to_value(&response).map_err(js_error)
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
        let source = "fn main() { let mut x = 0_i64; while x < 3_i64 { x = x + 1_i64; } }";
        let first = response(run(source, JsValue::NULL).unwrap());
        let second = response(run(source, JsValue::NULL).unwrap());
        assert!(first.ok);
        assert_eq!(first.output, second.output);
        assert_eq!(first.steps, second.steps);
        let formatted = response(format(source, JsValue::NULL).unwrap());
        assert!(formatted.ok);
        assert!(formatted.text.is_some());
    }
}
