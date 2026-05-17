//! Wasm (`wasm-bindgen`) helpers built on [`authdog-cli`] **without** the terminal OAuth stack.
//!
//! JWT payload parsing does **not** verify signatures (same caveat as `/whoami` claim preview).

use wasm_bindgen::prelude::*;

fn err_js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Parse JWT payload (middle segment) to pretty JSON (**signature not verified**).
#[wasm_bindgen(js_name = decodeJwtPayloadPretty)]
pub fn decode_jwt_payload_pretty(access_token: &str) -> Result<String, JsValue> {
    let claims = authdog_cli::whoami::decode_jwt_claims(access_token)
        .map_err(|e| err_js(format!("{e:#}")))?;
    serde_json::to_string_pretty(&claims).map_err(|e| err_js(e))
}

/// Render the CLI-style claim lines for a JWT (**signature not verified**).
#[wasm_bindgen(js_name = renderWhoamiClaimsFromJwt)]
pub fn render_whoami_claims_from_jwt(access_token: &str) -> Result<String, JsValue> {
    authdog_cli::whoami::describe_access_token(access_token).map_err(|e| err_js(format!("{e:#}")))
}

#[wasm_bindgen(js_name = defaultApiOrigin)]
pub fn default_api_origin() -> String {
    authdog_cli::whoami::DEFAULT_API_ORIGIN.to_string()
}
