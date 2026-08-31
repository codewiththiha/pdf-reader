//! WASM-side implementations of the engine's hot paths.
//!
//! The engine (`public/pdfEngine.js`) is deliberately a standalone JS module:
//! it loads before the wasm app and must keep working without it (that is
//! exactly how the smoke harness runs it). So the compiled functions are not
//! imported by the engine — the app hands them INTO the engine through the
//! registration calls ([`crate::bridge::set_wasm_baker`] and
//! [`crate::bridge::set_page_matcher`]), and the engine keeps its own JS
//! implementations as the standalone/fallback path.
//!
//! The registered closures are module-lifetime singletons stored in
//! thread-locals and deliberately never dropped: the engine holds the JS
//! function reference for the whole life of the webview, and dropping the
//! `Closure` while JS still holds that handle would turn every later call
//! into a dangling trap. One instance each, created once, is the bounded
//! cost.

use std::cell::OnceCell;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use pdf_core::appearance::FilterMatrix;

type BakeFn = dyn FnMut(
    js_sys::Uint8ClampedArray,
    js_sys::Float64Array,
    js_sys::Float64Array,
) -> js_sys::Uint8ClampedArray;

type PageMatchFn = dyn FnMut(JsValue) -> JsValue;

thread_local! {
    static BAKER: OnceCell<Closure<BakeFn>> = OnceCell::new();
    static MATCHER: OnceCell<Closure<PageMatchFn>> = OnceCell::new();
}

/// Register the compiled pixel baker and page matcher with the engine.
/// Idempotent; a no-op when the engine is absent (host tests, or `trunk
/// serve` without the engine bundle) so the JS fallbacks simply stay in
/// charge.
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
                // caller re-checks identity and treats an unchanged buffer
                // as "leave the page unbaked".
                if let Some(filter) = FilterMatrix::from_slice(&m.to_vec(), &o.to_vec()) {
                    pdf_core::appearance::bake_pixels(&mut pixels, &filter);
                }
                js_sys::Uint8ClampedArray::new_from_slice(&pixels)
            },
        );
        crate::bridge::set_wasm_baker(
            closure.as_ref().unchecked_ref::<js_sys::Function>().clone(),
        );
        let _ = cell.set(closure);
    });
    MATCHER.with(|cell| {
        if cell.get().is_some() {
            return;
        }
        // One page's positioned text + the query in, the page's matches out.
        // Any deserialize/serialize failure answers `null`, which the engine
        // treats as "no wasm answer for this page" and matches locally.
        let closure = Closure::<PageMatchFn>::new(|payload: JsValue| -> JsValue {
            match serde_wasm_bindgen::from_value::<pdf_core::search::PageTextPayload>(payload) {
                Ok(page) => {
                    serde_wasm_bindgen::to_value(&pdf_core::search::search_page(&page))
                        .unwrap_or(JsValue::NULL)
                }
                Err(_) => JsValue::NULL,
            }
        });
        crate::bridge::set_page_matcher(
            closure.as_ref().unchecked_ref::<js_sys::Function>().clone(),
        );
        let _ = cell.set(closure);
    });
}
