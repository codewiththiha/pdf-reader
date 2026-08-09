# CONTRACTS.md — frozen API contracts

Single source of truth for the interfaces every feature branch builds against.
**Branches MAY NOT change anything here.** If a branch needs more surface, it
adds a NEW contract entry + a note in the appendix, never a modification.

Ownership & rules are in the [Git / branch plan](#git--branch-plan).

---

## 1. Engine JS API — `window.PDFReader` (public/pdfEngine.js)

All functions **resolve, never reject**. Error shape: `{ok:false, error:{name,message}}`.
Success shape: `{ok:true, ...}`. Rust reads `ok` first, then deserializes via
`serde_wasm_bindgen` with the camelCase field names below.

| fn | signature | notes |
|---|---|---|
| `version()` | `() -> string` | "0.1.0" (selftest) |
| `storageGet(key)` / `storageSet(key, val)` | `() -> string\|null` / `() -> void` | localStorage wrappers |
| `open(path)` | `async () -> {ok, numPages, title, author, outline, page1Size}` | fetches bytes itself (convertFileSrc in Tauri), destroys prior doc. outline = flattened `[{title, page, depth}]` |
| `destroy()` | `async () -> void` | `loadingTask.destroy()`, clears state |
| `pageCount()` | `() -> number` | |
| `registerPage({page, canvasId, hostId})` | `() -> void` | hostId optional → no text layer (thumbnails) |
| `unregisterPage(canvasId)` | `() -> void` | cancels render + text, removes entry |
| `cancelPage(canvasId)` | `() -> void` | cancels in-flight render only |
| `renderPage(canvasId, scale, renderText)` | `async () -> {ok, width, height, scale}` | HiDPI; builds TextLayer when renderText && host; re-applies stored highlights |
| `renderPages(entries, scale)` | `async () -> [{ok,...}]` | batch for continuous; entry: `{page, canvasId, hostId?, renderText?}` |
| `updatePage(canvasId, scale)` | `async () -> {ok, width, height, scale}` | cancel render+text, re-render at scale |
| `buildSearchIndex()` | `async () -> number` | walks 1..numPages getTextContent, caches rects @scale1 |
| `search(query)` | `async () -> {ok, query, total, results:[{page, text, matches:[{x,y,w,h}]}]}` | rects @scale1 CSS px |
| `clearHighlights()` | `() -> void` | removes all .highlight elements |

Canvas / host element **ids** are Rust-chosen unique strings. The engine resolves
elements via `getElementById` — no DOM nodes cross the wasm boundary.

## 2. Rust bridge externs — `src/core/bridge.rs`

- Tauri dialog: `window.__TAURI__.dialog.open({multiple:false, directory:false, filters:[{name:"PDF", extensions:["pdf"]}]}) -> Promise<string|null>`.
- Engine (js_namespace `["window","PDFReader"]`): `version`, `storage_get`, `storage_set`,
  `open`, `destroy`, `page_count`, `register_page`, `unregister_page`, `cancel_page`,
  `render_page`, `render_pages`, `update_page`, `build_search_index`, `search`, `clear_highlights`.
- Async fns return `js_sys::Promise`-resolved `JsValue`, awaited via
  `wasm_bindgen_futures::JsFuture` inside the `#[wasm_bindgen]` glue itself.
- Only `crate::api::engine` calls engine fns.

## 3. CSS variables + theme ids — styles/input.css

- `@theme` tokens: `--color-paper/ink/muted/surface/line/accent/accent-soft`
  → utilities `bg-paper text-ink bg-surface text-muted border-line bg-accent bg-accent-soft`.
- `@custom-variant dark (&:where(.dark, .dark *));` — `.dark` toggled on `<html>`.
- Runtime vars on `:root`: `--canvas-filter`, `--canvas-blend`, `--noise-opacity`.
- Theme ids (MUST match `:root[data-theme="..."]` blocks AND `src/core/themes.rs`):
  `light`, `dark`, `sepia`, `green`, `night`, `dim`.
- Texture classes on `.pdf-page`: `texture-none` (implicit), `texture-paper`,
  `texture-lined`, `texture-grid`, `texture-dotted`, `texture-cross`.
- Noise: `body.noise-enabled` shows `.noise-overlay`; `--noise-opacity` = intensity/100.

## 4. Store schemas — `src/core/state.rs` (RwSignal fields, stable names)

`AppState { settings, doc, viewer, search, sidebar }` (provided via context).

- `Settings` (serde, localStorage `pdfreader.settings.v1`):
  `theme_id: String`, `texture: TextureMode`, `noise_enabled: bool`,
  `noise_intensity: u8 (0..=100)`, `default_zoom: f64`, `last_path: Option<String>`.
- `DocumentState`: `status(DocStatus)`, `error`, `path`, `num_pages`, `title`,
  `author`, `outline(Vec<OutlineNode>)`, `page1_size(Option<PageSize>)`, `page_heights(Vec<f64>)`.
- `ViewerState`: `mode(ViewMode)`, `page(u32 1-based)`, `scale(f64)`, `fit(FitMode)`,
  `scroll_top(f64)`, `render_scale(f64)`, `container_size((f64,f64))`.
- `SearchState`: `query`, `total`, `results`, `active`, `index_built`.
- `SidebarMode`: `None | Outline | Search | Thumbs`.

## 5. Component prop interfaces

- **Atoms**: `Icon(name, size?)`, `Button(on_click, kind?, icon?, label?, title?, active?, disabled?, children?)`,
  `Tooltip(text, side?, children)`, `Select<T>(options, value, on_change, title?)`,
  `Slider(value, min, max, step, on_change, label?)`, `Toggle(checked, on_change, label?)`,
  `Separator(vertical?)`, `Kbd(children)`.
- **`PageCanvas`** (foundation-owned, shared by both views):
  `page: u32, scale: ReadSignal<f64>, canvas_id: String, host_id: String,
  render_text: bool, class: String = "", on_geometry: Option<Callback<(u32, f64, f64)>>`.
- **Organisms/Molecules**: `Toolbar(state)`, `ZoomControls(state)`, `PageNav(state)`,
  `ThemeMenu(state)`, `TextureMenu(state)`, `NoiseToggle(state)`, `SearchBox(state)`,
  `SidebarItem(icon, label, active, badge, on_click)`, `Sidebar(state)`,
  `OutlinePanel(state)`, `SearchPanel(state)`, `ThumbnailsPanel(state)`,
  `PageList(state)`, `StatusBar(state)`.

## 6. Module tree (frozen after foundation)

`mod.rs` files declare all modules and are NEVER edited by branches. Files exist
as empty stubs with `// TODO(<branch>)` until their owner fills them.

## 7. Git / branch plan

Baseline (scaffold) commit exists on `main`. Foundation commit adds everything
above. Four parallel branches fork from foundation:

| # | branch | fills |
|---|---|---|
| A | `viewer/continuous` | `continuous_view.rs`, `page_list.rs`, `effects/continuous_scroll.rs` |
| B | `viewer/chrome` | `single_page_view.rs`, `toolbar.rs`, `zoom_controls.rs`, `page_nav.rs`, `status_bar.rs`, `effects/shortcuts.rs` |
| C | `panels/sidebar` | `sidebar.rs`, `outline_panel.rs`, `search_panel.rs`, `search_box.rs`, `sidebar_item.rs`, `effects/search_effects.rs` |
| D | `panels/settings` | `theme_menu.rs`, `texture_menu.rs`, `noise_toggle.rs`, `thumbnails_panel.rs`, `effects/theme_ui.rs` |

Conflict-avoidance rules:
1. `mod.rs` files are frozen after foundation — never edit.
2. Ownership table above is authoritative. Never edit a file not on your list.
3. Extend contracts by addition only (append to this file's appendix).
4. The coordinator owns ALL slot wiring (reader_view.rs, app.rs, toolbar assembly).
5. Each branch must pass its own build gate before merge: `cargo check --target wasm32-unknown-unknown -p pdf-ui` + `trunk build`.
6. Merge order: A + B first, then C, then D, then the coordinator's integration commit.

### Appendix — additions (branches append here, never modify above)

_(none yet)_
