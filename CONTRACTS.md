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

---

### Appendix entry 1 — thumbnail lane + filename (additive only)

**Engine (`window.PDFReader`), NEW functions.** Existing entries unchanged.

| fn | signature | notes |
|---|---|---|
| `renderThumb(canvasId, page, scale)` | `async () -> {ok, width, height, scale, cached}` | Cached thumbnail lane. No `registerPage` needed, never builds a text layer. Renders into a DETACHED canvas and blits the finished frame, so the live canvas is never shown mid-render. `cached:true` = the bitmap was blitted SYNCHRONOUSLY from the LRU cache before the promise suspended; the caller MUST then skip its loading skeleton (covering an already-painted thumbnail and crossfading it away is the per-row scroll flicker). |
| `cancelThumb(canvasId)` | `() -> void` | Cancels an in-flight thumbnail render. Deliberately does NOT evict the cache — a row that scrolls out and back must repaint instantly. |
| `hasThumb(page, scale)` | `() -> bool` | SYNCHRONOUS cache probe, read while a cell builds its view so a hit can mount with no skeleton and no animation classes at all. |

`destroy()` additionally cancels in-flight thumbnail renders and clears the
thumbnail cache (no cross-document bitmap bleed). Cache is LRU-bounded at 256
entries and keyed on `(page, scale)`.

**Rust bridge additions** (`src/core/bridge.rs`): `render_thumb`, `cancel_thumb`,
`has_thumb`. **`src/api/engine.rs`**: same three, plus `ThumbResult` in
`src/core/document.rs` (`{width, height, scale, cached}`).

**Text layer ownership change.** `renderPage` now builds the text layer in a
DETACHED `div.textLayer` and swaps it into the host in one mutation once the
render completes. `PageCanvas`'s `.textLayer` node is therefore a placeholder
that the engine REPLACES; Leptos must not own its contents. This removes the
overlapping-span duplicates a superseded render used to leave behind (visible as
doubled/offset text when selecting, and doubled highlight boxes).

**New module `src/core/filename.rs`** (pure, unit-tested): `display_name(title,
path)` decides what the toolbar shows. A PDF `/Title` is used only when it looks
like a title — path-shaped, URL-shaped, percent-encoded, placeholder, or
overlong titles are rejected in favour of the percent-decoded file stem.

**New component `molecules::doc_title::DocTitle(state)`**, rendered by the
toolbar. It measures the toolbar's live geometry and caps its own width so the
name folds only on a real collision. It depends on these MEASUREMENT ANCHOR ids
in `molecules/toolbar.rs`: `#toolbar-row`, `#toolbar-left-pre`, `#toolbar-right`,
and `#toolbar-center` (present only in Single mode — its presence is how the
label knows the centered nav is in play). Renaming one silently degrades the
label to "never truncate".

**`atoms::segmented::Segmented`** options are now `(value, label, title)`; the
title is required because the segments are icon-only.

**ResizeObserver lifecycle rule.** Every `ResizeObserver` MUST be
`disconnect()`ed in `on_cleanup` BEFORE its `Closure` is dropped. The browser
holds its own reference to the wasm-bindgen shim, so a resize notification
queued during teardown (e.g. a view-mode switch removing `#page-list`) otherwise
invokes freed memory and aborts the runtime with "closure invoked recursively or
after being dropped".

### Appendix entry 2 — the zoom pipeline

**Three scales, three owners.** `ViewerState` now carries `display_scale`,
`zoom_animating` and `zoom_request` alongside `scale` / `render_scale`:

| signal | meaning | written by |
| --- | --- | --- |
| `scale` | the committed, user-visible zoom (the toolbar %) | the zoom coordinator, once per gesture |
| `display_scale` | what the layout is DRAWN at right now | the zoom coordinator, every animation frame |
| `render_scale` | what the bitmaps were RASTERISED at | the zoom coordinator, once per gesture |
| `zoom_animating` | a zoom is in flight; renders + geometry write-back suspended | the zoom coordinator |
| `zoom_request` | `(target, animate, token)` — a request for a new zoom | any control, via `request_zoom` |

**The rule: a zoom is a layout animation of bitmaps that are already painted,
followed by exactly one crisp render.** It is never a render-driven relayout.

**`effects::fit::request_zoom(state, target, animate)` is the ONLY supported way
to change zoom.** No component may write `scale` or `render_scale` directly. The
single exception is `toolbar::open_path`, which seeds all three for a brand-new
document (there is no prior layout to animate from). Writing them directly from
a control reintroduces the original bug: the scale changes instantly while the
wrappers' `top:` offsets and the spacer height only catch up as each render
resolves, so the scroll offset ends up pointing at a different page.

`core::layout::anchored_scroll(scroll_top, viewport_h, heights, gap, factor,
anchor_screen_y)` is the pure anchor math (unit-tested). Page heights are linear
in scale, so a scale change is applied to the whole column in one synchronous
step; the scroll is re-anchored in that SAME step. Gaps are chrome and do not
scale. Scroll offset `<= 0.5` pins the top of the document instead of the
centre, so zooming on page 1 doesn't push page 1 off-screen.

**`PageCanvas` has two effects, and they must stay separate.** The STRETCH
effect follows the `scale` prop (callers pass `display_scale`) and only ever
CSS-resizes the host so the existing bitmap follows the layout — it never
renders. The RENDER effect follows `render_scale` and early-returns while
`zoom_animating` is true and the page already has geometry. Merging them is the
ghost/double-image bug.

**`PageList::on_geometry` returns early while `zoom_animating`.** During a zoom
the coordinator owns `page_heights`; a render resolving mid-flight would write
one page's height at the wrong scale and shift everything below it.

**Fit is frozen during a sidebar slide.** `fit_effect` compares
`window.innerWidth` between runs: `container_size` changes with no window change
mean the `<aside>` is animating, and the zoom must not move. Only a real window
resize refits, and it does so via `request_zoom(.., animate=false)` — instant,
but still anchored.

**Post-await StoredValue access must use `try_get_value` / `try_set_value`.** A
render can outlive its component (`<For>` unmounts a page whose rasterisation is
still in flight); touching a `StoredValue` after its owner is disposed panics.

**Engine addition (additive):** `PDFReader.blitThumb(canvasId, page) -> bool`
paints the cached thumbnail of `page` into a full-size page canvas as a blurry
placeholder, so a page scrolled into view isn't a flash of white. Best-effort:
returns false when nothing is cached. The pre-existing internal helper of the
same name is now `paintCached`.

### Appendix entry 3 — page counter metric, and the sidebar slide

**The page counter reports the DOMINANT page, not the top-edge page.**
`core::layout::dominant_page(scroll_top, viewport_h, heights, gap)` returns the
page occupying the most of the viewport, ties going to the lower page number.
`page_from_scroll` (top-edge) is still used for scroll targeting, where "which
page starts here" is the right question.

Why: zooming out shrinks every page, so more of the PREVIOUS page slides into
the top of the viewport. With a top-edge counter the reported page walked
1 -> 2 -> 3 -> 4 while zooming out and back down while zooming in, even though
the view was correctly anchored and the reader never moved. The view was right;
the counter was measuring the wrong thing. Area-of-viewport degrades correctly
at both extremes: zoomed in one page fills the viewport and wins outright;
zoomed out the page you see most of wins; and after a jump aligning page P's
top with the viewport top, P still wins, so jumps report where they landed.

`effects::page_tracking` effects 2 and 3 MUST use the same metric, or they
disagree about whether a jump has arrived and fight each other.

**`.pdf-page` must keep `flex-shrink: 0`.** The host is a flex child carrying an
explicit inline width/height from `PageCanvas`. With the default
`flex-shrink: 1`, any moment the host is wider than its container — mid-zoom, or
mid-sidebar-slide — the browser shrinks the width while the inline height stays,
and the page visibly squishes (a letter page measured 0.77 -> 0.58 aspect)
before snapping back at the end. The inline size is the source of truth for page
geometry; layout must never renegotiate it.

**The sidebar slide is a continuous fit animation, not a freeze.** While
`FitMode::Width`/`Page` is active, `fit_effect` follows the `<aside>`'s 300ms
width animation in LAYOUT every frame: each `container_size` burst writes
`display_scale` and re-anchors the scroll with `zoom_animating` held true, and a
120ms debounce commits ONE crisp render at the end. Freezing the scale instead
(the earlier approach) is what caused the squish-then-snap. A real window resize
takes the same path; `window.innerWidth` still distinguishes the two, but now
only to decide whether the fit target should be recomputed at all.

**`PageCanvas`'s render effect returns early whenever `zoom_animating` is true**
— including for pages with NO geometry yet. Such pages mount constantly during a
slide (a shrinking scale fits more pages on screen); rendering them at the
in-flight scale is work that is obsolete before it resolves. Measured on one
sidebar toggle: 11 renders across 2 scales became 8 renders at 1 scale. The
`blitThumb` underlay covers the gap until the commit pass.

### Appendix entry 4 — the engine owns the canvas BACKING STORE, never its CSS box

`renderPage` sets `canvas.width/height` (the backing store) and **must not** set
`canvas.style.width/height`. The canvas's CSS box belongs to the stylesheet:

```css
.pdf-page canvas { position: absolute; inset: 0; width: 100%; height: 100%; }
```

This is the same rule `paintCached` already documents for thumbnails, now true
of the page path too.

**Why it is a contract and not a detail.** Zoom is a layout animation of
already-painted bitmaps followed by one crisp render. An inline width/height on
the canvas beats the stylesheet's `100%`, which pinned the canvas to the size of
the *last completed render* while the host — and the `::before` paper texture,
which is `inset: 0` and so does track — grew every frame. Measured on a single
`+`: the host animated 1152 -> 1224px while the canvas sat at 1152 the whole
way, a 72px divergence that snapped shut only on the final frame. The page
bitmap visibly lagged its own texture and border.

`renderPage`'s return value still reports the render's CSS size (`{ok, width,
height, scale}`) and `PageCanvas` still sizes the HOST from it — the host is the
one element with an explicit pixel size, and everything inside it is `inset: 0`.

Regression guard: **section J of `verify-zoom.mjs`** samples the host and canvas
rects once per rAF across a zoom and asserts `max |canvas - host| <= 1px`, plus
`>= 3` distinct intermediate host widths so an instant zoom cannot vacuously
pass. Consumers deriving a CSS size from a canvas must divide the backing store
by `min(devicePixelRatio, 2)` (see `sizeHost` in `verify.mjs`).
