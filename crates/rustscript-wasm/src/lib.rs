#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn check(source: &str) -> Result<JsValue, JsValue> {
    let result = rustscript_core::parse(source, rustscript_core::Limits::default());
    serde_wasm_bindgen::to_value(&result.map(|_| true).map_err(|error| error))
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
