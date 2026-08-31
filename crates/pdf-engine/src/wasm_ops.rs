//! WASM-side implementations of the engine's hot paths.
//!
//! The engine (`public/pdfEngine.js`) is deliberately a standalone JS module:
//! it loads before the wasm app and must keep working without it (that is
//! exactly how the smoke harness runs it). So the compiled functions are not
//! imported by the engine — the app hands them INTO the engine through the
//! registration calls ([`crate::bridge::set_wasm_baker`]), and the engine
//! keeps its own JS implementation as the standalone/fallback path.
//!
//! The registered closures are module-lifetime singletons stored in
//! thread-locals and deliberately never dropped: the engine holds the JS
//! function reference for the whole life of the webview, and dropping the
//! `Closure` while JS still holds that handle would turn every later call
//! into a dangling trap. One instance, created once, is the bounded cost.

use std::cell::OnceCell;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use pdf_core::appearance::FilterMatrix;

type BakeFn = dyn FnMut(
    js_sys::Uint8ClampedArray,
    js_sys::Float64Array,
    js_sys::Float64Array,
) -> js_sys::Uint8ClampedArray;

thread_local! {
    static BAKER: OnceCell<Closure<BakeFn>> = OnceCell::new();
}

/// Register the compiled pixel baker with the engine. Idempotent; a no-op
/// when the engine is absent (host tests, or `trunk serve` without the
/// engine bundle) so the JS fallback simply stays in charge.
pub fn install() {
    if !crate::host::has_pdf_reader() {
        return;
    }
    BAKER.with(|cell| {
        if cell.get().is_some() {
            return;
        }
        let closure = Closure::<BakeFn>::new(
            |data: js_sys::Uint8ClampedArray,
             m: js_sys::Float64Array,
             o: js_sys::Float64Array|
             -> js_sys::Uint8ClampedArray {
                let mut pixels = data.to_vec();
                // A malformed matrix (wrong lengths) bakes nothing: the JS
                // caller re-checks identity and skips on no-op results.
                if let Some(filter) = FilterMatrix::from_slice(&m.to_vec(), &o.to_vec()) {
                    pdf_core::appearance::bake_pixels(&mut pixels, &filter);
                }
                let Ok(out) =
                    js_sys::Uint8ClampedArray::new_with_byte_length(pixels.len() as u32)
                else {
                    // Allocation refused (or the length overflowed): hand the
                    // original buffer back instead of trapping — the caller
                    // treats an unchanged buffer as "leave the page unbaked".
                    return data;
                };
                out.copy_from(&pixels);
                out
            },
        );
        crate::bridge::set_wasm_baker(
            closure.as_ref().unchecked_ref::<js_sys::Function>().clone(),
        );
        let _ = cell.set(closure);
    });
}
