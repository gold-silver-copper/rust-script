#![cfg(target_arch = "wasm32")]
#![forbid(unsafe_code)]

use serde::Deserialize;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen(module = "/tests/browser_worker.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = browserWorkerRecovery)]
    async fn browser_worker_recovery() -> Result<JsValue, JsValue>;
}

#[derive(Deserialize)]
struct Response {
    ok: bool,
    output: Option<Vec<u8>>,
    error: Option<WasmDiagnostic>,
}

#[derive(Deserialize)]
struct WasmDiagnostic {
    #[serde(flatten)]
    diagnostic: Diagnostic,
    location: Option<Location>,
}

#[derive(Deserialize)]
struct Diagnostic {
    message: String,
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct Location {
    line: u32,
    column: u32,
}

fn response(value: JsValue) -> Response {
    serde_wasm_bindgen::from_value(value).expect("response must deserialize")
}

#[wasm_bindgen_test]
fn browser_exports_run_and_report_structured_failures() {
    let run = response(
        rustscript_wasm::run("fn main() { println!(\"{}\", 42_i64); }", JsValue::NULL)
            .expect("run response"),
    );
    assert!(run.ok);
    assert_eq!(run.output, Some(b"42\n".to_vec()));

    let invalid = response(
        rustscript_wasm::check("fn main() {\n    missing;\n}", JsValue::NULL)
            .expect("check response"),
    );
    assert!(!invalid.ok);
    let error = invalid.error.expect("diagnostic");
    assert_eq!(error.location, Some(Location { line: 2, column: 5 }));

    let limits =
        serde_wasm_bindgen::to_value(&output_limit_options()).expect("options must serialize");
    let limited = response(
        rustscript_wasm::run("fn main() { println!(\"{}\", true); }", limits)
            .expect("limited run response"),
    );
    assert!(!limited.ok);
    assert_eq!(
        limited.error.expect("limit diagnostic").diagnostic.message,
        "output byte limit exceeded"
    );
}

#[wasm_bindgen_test]
async fn browser_host_reports_worker_abort_and_replaces_worker() {
    let generations = browser_worker_recovery()
        .await
        .expect("browser worker recovery helper");
    assert_eq!(generations.as_f64(), Some(2.0));
}

#[derive(serde::Serialize)]
struct Options {
    runtime: RuntimeLimits,
}

#[derive(serde::Serialize)]
struct RuntimeLimits {
    fuel: u64,
    max_call_depth: usize,
    max_output_bytes: usize,
}

fn output_limit_options() -> Options {
    Options {
        runtime: RuntimeLimits {
            fuel: 1_000_000,
            max_call_depth: 1_024,
            max_output_bytes: 1,
        },
    }
}
