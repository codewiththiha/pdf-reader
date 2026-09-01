//! Frameless window controls (Windows/Linux): minimize, maximize/restore,
//! close, and the maximized probe that picks the caption's glyph.
//!
//! `tauri.windows.conf.json` / `tauri.linux.conf.json` remove the native
//! title bar (`decorations: false`), and the titlebar's `WindowControls`
//! cluster replaces it. macOS keeps its native traffic lights and never
//! reaches this module.
//!
//! Everything is defensive like the rest of the api: outside Tauri
//! (`trunk serve`) the calls are silent no-ops, and a window object without
//! the expected method resolves to `None` instead of unwinding the caller.

use wasm_bindgen::JsValue;

use crate::bridge;

/// The current Tauri window handle, or `None` outside Tauri. Same probe
/// contract as [`super::window::set_traffic_lights`]: `getCurrentWindow`
/// dereferences the `window.__TAURI__` chain, and the wasm-bindgen shim
/// throws when the global is absent — so the guard must come first.
fn window() -> Option<JsValue> {
    if !bridge::has_tauri() {
        return None;
    }
    let win = bridge::tauri_get_current_window();
    if win.is_undefined() || win.is_null() {
        None
    } else {
        Some(win)
    }
}

/// Call a no-arg method on the window handle and return its RESOLVED value.
///
/// Tauri v2 window methods return Promises, so the await is load-bearing:
/// handing the Promise itself back would make `isMaximized` read as
/// `as_bool() == None` — always false — and the caption would never swap
/// to its restore glyph.
async fn invoke_method(win: &JsValue, name: &str) -> Option<JsValue> {
    let method = js_sys::Reflect::get(win, &JsValue::from_str(name)).ok()?;
    if !method.is_function() {
        return None;
    }
    let func: js_sys::Function = method.into();
    let result = js_sys::Reflect::apply(&func, win, &js_sys::Array::new()).ok()?;
    if result.is_undefined() || result.is_null() {
        return Some(result);
    }
    // The cast is unchecked by design: js-sys's `Promise::try_from` cannot
    // fail either (its error type is `Infallible`), and a Tauri v2 window
    // method that returns a non-Promise is not a shape the API ships — if
    // one ever did, the await below surfaces it as `None`, not a panic.
    let promise = js_sys::Promise::from(result);
    wasm_bindgen_futures::JsFuture::from(promise).await.ok()
}

/// Minimize to the taskbar. No-op outside Tauri.
pub async fn minimize_window() {
    if let Some(win) = window() {
        invoke_method(&win, "minimize").await;
    }
}

/// Maximize ↔ restore. The drag region's built-in double-click runs the
/// same toggle (`internal_toggle_maximize`, in Tauri's injected script) —
/// one command behind two triggers, so the bar and the caption can never
/// disagree about what a double-click does.
pub async fn toggle_maximize_window() {
    if let Some(win) = window() {
        invoke_method(&win, "toggleMaximize").await;
    }
}

/// Close the window. This app runs a single window, so this is quit.
pub async fn close_window() {
    if let Some(win) = window() {
        invoke_method(&win, "close").await;
    }
}

/// Whether the window is maximized — drives the maximize/restore glyph.
/// `false` whenever the answer cannot be had (browser, missing method),
/// which is also the correct pre-maximize default.
pub async fn is_window_maximized() -> bool {
    let Some(win) = window() else {
        return false;
    };
    invoke_method(&win, "isMaximized")
        .await
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}
