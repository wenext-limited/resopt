use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct PngResult {
    bytes: Vec<u8>,
    summary: String,
}
#[wasm_bindgen]
impl PngResult {
    #[wasm_bindgen(getter)]
    pub fn summary(&self) -> String {
        self.summary.clone()
    }
    pub fn take_bytes(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}

#[wasm_bindgen]
pub fn optimize_png(input: &[u8], effort: u8, reductions: bool) -> Result<PngResult, JsError> {
    let output = resopt::portable::optimize_png(input, effort, reductions)
        .map_err(|e| JsError::new(&format!("{e:#}")))?;
    Ok(PngResult {
        bytes: output.bytes,
        summary: serde_json::to_string(&output.summary)
            .map_err(|e| JsError::new(&e.to_string()))?,
    })
}

#[wasm_bindgen]
pub fn score_srgb_rgba(
    width: usize,
    height: usize,
    original: &[u8],
    candidate: &[u8],
) -> Result<f64, JsError> {
    resopt::portable::score_srgb_rgba(width, height, original, candidate)
        .map_err(|e| JsError::new(&format!("{e:#}")))
}
