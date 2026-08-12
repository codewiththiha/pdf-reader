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
| `version()` | `() -> string` | "0.1.0" (engine handshake) |
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

### Appendix entry 5 — only ONE blended canvas may be visible per `.pdf-page`

`.pdf-page canvas` carries `mix-blend-mode: var(--canvas-blend)` and
`filter: var(--canvas-filter)`. Those are per-theme (`multiply` for
light/sepia/green, `screen` for dark/night, `soft-light` for dim).

Blend modes do not compose idempotently. On the commit frame of a zoom the
`.page-snapshot` underlay and the freshly rendered canvas are both in the host,
so the backdrop composites as `B*L*S` instead of `B*L` and the theme filter is
applied **twice** — green's `hue-rotate(70deg)` effectively lands as `140deg`.
Measured on the blank paper of the sepia theme, that single frame went
`rgb(238,230,206)` -> `rgb(232,224,197)`; in light mode the shift is exactly `0`
because white multiplied by white is white. Hence the user-visible symptom: an
end-of-zoom flicker that was invisible in light mode and obvious in
sepia/green/dim and on textured paper.

The invariant is therefore **not** "at most one canvas in the host" (the
snapshot exists precisely so there are two) but **at most one VISIBLE blended
canvas**:

```css
.pdf-page:has(canvas.page-snapshot) canvas:not(.page-snapshot) {
  visibility: hidden;
}
```

Deliberately `:has()` and `visibility`, not a Rust-toggled class and not
`display`/opacity:

- the rule stops matching the instant the snapshot leaves the DOM, so a failed
  render or a missed cleanup can strand nothing — where `:has()` is unsupported
  it degrades to the old one-frame flicker, never to a blank page;
- `visibility: hidden` keeps the element laid out, so the canvas keeps its
  geometry and the stretch effect keeps tracking the host underneath.

Anything that adds a further stacked layer inside `.pdf-page` must keep this
invariant. Regression guard: **section K of `verify-zoom.mjs`** runs a zoom in
light/sepia/green/dim and asserts the max number of visible canvases never
exceeds 1 and settles at exactly 1.

### Appendix entry 6 — outline indent is capped; thumbnail pulse freezes at resolve

Two independent fixes, both "the state at the moment of a transition".

**Outline rows must always have room for text.** The indent was an unbounded
`8 + depth * 14` px against a fixed 288px (`w-72`) sidebar, so from about depth
12 the padding consumed the entire row and the `truncate` class
(`overflow:hidden` + `text-overflow:ellipsis`) collapsed the label to a bare
"...". Real-world TOCs nest 5-10 levels (part > chapter > section > ...), so
deep entries rendered as dots carrying no information. `indent_px(depth)` in
`outline_panel.rs` now uses a 12px step capped at `INDENT_MAX = 120`, keeping
>= 150px for every label at any depth, and each row carries a `title` attribute
so a genuinely long name is still recoverable on hover. Unit-tested for depths
0..64 against the panel geometry; depth beyond the cap is still conveyed by
tree order.

**A thumbnail's skeleton tint must not move while the cover fades.** The pulse
(`thumb-skeleton-pulse`, 1.6s) swings a long way — measured in sepia, 54
luminance units, min 159.7 / max 213.7 — and is deliberately left running
through the 300ms fade-out, because REMOVING the class mid-fade cancels the
animation and snaps the tint back to base in one frame. But a render can resolve
at ANY phase of that cycle, so every freshly rendered cell began its reveal from
a different brightness and kept oscillating while fading; cells in the same row
resolve a few ms apart, so they shimmered against one another. That is the
residual flicker on newly rendered thumbnails during virtual scrolling (the
cached remount path was already clean and is unchanged).

```css
.thumb-skeleton-settling { animation-play-state: paused; }
```

added at resolve, and the EXISTING `PULSE_STOP_MS` timer now removes
`thumb-skeleton-loading` **and** `thumb-skeleton-settling` together once the
cover is fully transparent. Pausing rather than removing is the whole point: the
computed background stays exactly where the animation left it, so nothing snaps,
and the reveal becomes a plain monotonic fade from a stable colour. Measured
after the fix: tint movement during a reveal is **0** across 24 cells (was ~54).

Regression guards in `verify-ui.mjs`: outline rows all keep >= 100px of text
width, the 6-level-deep entry renders its real title, every row has a tooltip;
and thumbnails reveal with a worst tint swing <= 1 with zero cells left covered
or stuck paused. Fixture: `public/samples/Outlined Book.pdf` (12pp, 14 outline
entries — UTF-16BE, accented, CJK, plain-ASCII titles and a 6-level branch).

### Appendix entry 7 — texture is page-anchored; blank outline titles; unpainted thumb canvases

**Textures scale with the page.** `.pdf-page::before` carries every paper
texture as a repeating background. Their geometry used to be hard-coded in CSS
px — 26px rules, an 8px dot pitch, a 180x180 noise tile — so a zoom grew the
page while the pattern pitch stayed identical: measured at 145%, the host was
1.454x larger and `background-size` was still `auto`, so the lines slid across
the text rather than moving with it. That is what reads as "the texture is just
a filter, not attached to the page".

`.pdf-page::before` now derives its geometry from the page's own scale:

```css
--tex-scale: var(--scale-factor, 1);
--tex-unit:  calc(26px * var(--tex-scale));
--tex-line:  clamp(0.5px, calc(1px * var(--tex-scale)), 2.5px);
background-origin: border-box;
background-position: 0 0;
```

`--scale-factor` is the right hook because `page_canvas.rs` writes it in the
SAME inline style as the host's width/height (the text layer already depends on
it), so the texture rescales on exactly the frame the page does — including
every intermediate frame of a zoom animation, with no extra plumbing. `--tex-line`
is clamped so hairlines neither vanish at small zoom nor become bars at large
zoom. Measured after: page x1.018 / texture x1.018, host-to-pitch ratio spread
0.0003 across 50 frames. Any NEW texture variant must express its geometry in
`--tex-unit` / `--tex-scale`, never raw px. Guard: **section L of
`verify-zoom.mjs`**.

**Outline titles must survive normalisation.** `it.title || "(untitled)"` only
catches the empty string. Real PDFs also carry whitespace-only ("   "),
newline-only ("\r\n") and zero-width (U+200B/U+FEFF/U+00AD) bookmark titles;
those rendered a row whose text had no height, so the row collapsed from 28px to
**8px** — a sliver that reads as a dot. `outlineTitle()` in `pdfEngine.js` strips
zero-width characters, collapses whitespace runs (so an embedded newline cannot
double a row's height either) and falls back to "(untitled)". `min-h-7` +
`leading-5` on the row is the backstop so no future title can collapse the box.

**An unpainted thumbnail canvas must not composite.** `.thumb-canvas` carries
`mix-blend-mode` + the theme filter. A canvas with no width/height attributes
defaults to **300x150** — a 2:1 box stretched over a ~3:4 card — so before its
first render each cell blended an empty, wrong-aspect surface under the fading
cover, tinting it (worst in the multiply themes, sepia/green) and then changing
shape when the real 153x198 bitmap arrived. `.thumb-canvas-blank
{ visibility: hidden }` is applied while `!loaded` and dropped exactly when the
cover begins to fade. Same invariant as appendix entry 5: never blend a layer
with no real pixels in it. Measured: the sepia reveal's downward hook fell from
10 luminance units to 1.

### Appendix 8 — a zoom round-trip must land on the page it started from

Zooming out below 100% and back in moved the reader tens of pages backwards
(reported: 256 → 232). Two independent defects compounded, both in the
continuous viewer's height bookkeeping.

**1. Every page was seeded with page 1's height.** `PageList` filled
`doc.page_heights` with `page1_size.height * scale` for the whole document and
relied on `on_geometry` to correct each entry — but `on_geometry` only fires for
pages that have actually rendered, i.e. the visible window. In a mixed-size PDF
(plates, legal inserts, a landscape map) every off-screen page therefore carried
the wrong height, so the document's total height and every page offset below the
viewport shifted as pages were measured for the first time. Zooming out pulls a
batch of never-measured pages into view at once, so a whole block of corrections
lands in one frame and drags the content out from under the scroll anchor.

`engine.open()` now returns `pageHeights` — the intrinsic (scale-1) height of
every page, read from each page's viewport. `getPage` only parses the page
dictionary, not its content stream, so this is cheap even for 300+ pages.
`doc.page_sizes` holds it and `PageList` seeds each page from its OWN height.
The seed runs once per document, NOT per scale change: the zoom coordinator
rescales that same vector frame by frame and re-anchors scroll against it, so a
competing write mid-gesture fights the anchor and reintroduces the drift.

**2. The scroll write was clamped by a stale scrollHeight.** `relayout_to`
rescales the heights and then writes `scrollTop` synchronously, but Leptos
applies the spacer div's new height in a *later* effect pass. When zooming IN
the container is still its old, shorter self at the moment of the write, so the
browser clamps `scrollTop` to the old `scrollHeight - clientHeight`. The scroll
listener then reads that truncated value straight back into `viewer.scroll_top`,
so every frame of the gesture loses a little more distance — always toward the
end of the document, where the clamp bites hardest. `relayout_to` now writes the
spacer's height directly before moving the scrollbar; Leptos writes the same
value moments later, so it is idempotent rather than a competing source of truth.

Guarded by `zoom_round_trip_keeps_the_same_page_in_a_mixed_size_document` and
`page_offsets_follow_each_pages_own_height` in `core::layout`.

### Appendix 9 — the sidebar resizes the page; panels do not close on navigation

**A sidebar slide must rescale the page whether or not a fit mode is active.**
`fit_effect` returned early on `FitMode::None`, and every manual zoom sets
exactly that (`zoom_controls.rs`, `shortcuts.rs`). So the behaviour worked
until the reader touched the zoom, and from then on opening the panel slid it
over a page that kept its old width instead of making room — "it works first,
then once I zoom it stops". With a fit mode the target is still recomputed from
the container. Without one, the slide is followed PROPORTIONALLY: the page keeps
the same fraction of the container width it had, so opening shrinks it and
closing restores it to the pixel.

This is deliberately scoped to a sidebar toggle, tracked by watching
`state.sidebar` directly rather than inferring intent from `container_size`. A
window resize with no fit mode leaves the scale alone — growing the window must
not silently re-zoom the document — and `following_slide` is cleared by the same
120ms debounce that commits the render, so only the frames belonging to the
slide are treated proportionally.

**Navigating from a panel keeps it open.** Both `thumbnails/cell.rs` and
`outline_panel.rs` called `sidebar.set(SidebarMode::None)` on click. Browsing is
a loop — jump, look, jump again — and closing the panel on every jump forces the
reader to reopen it each time. It also left the outline highlight with nowhere
to show. The rail buttons remain toggles, so the panel is still one click from
closed.

**The outline shows where the reader is.** `active_outline_index` picks the last
entry at or before the current page: a TOC entry owns every page from its own up
to the next entry's. Ties go to the LATER (deeper) entry, since a section is a
more specific answer than the chapter that starts on the same page, and nothing
is highlighted before the first entry's page — a cover belongs to no section.
The row carries the accent on its already-reserved `border-l-2` (so nothing
shifts when it lights up), plus a tinted ground, full-strength ink and
`aria-current` for screen readers. It is derived from `viewer.page`, so it
tracks scrolling, not just clicks.

### Appendix 10 — the appearance model: base + computed tint + presets

**Six fixed themes became three bases and a continuous tint.** Sepia and Green
were only ever "light, tinted brown" and "light, tinted green" — the same
structure with a different hue, hand-written twice. That does not generalise: a
reader who wants slightly cooler paper had no way to ask, and every new tint
meant another CSS block. There are now three BASE modes (Light / Dark / Dim)
that decide the structural family — does the canvas invert, do textures
multiply or screen — plus a `{hue, strength}` tint applied by the same maths on
any base.

**Sepia, Green and Night still exist, as presets.** `{Light, 34°, 45}`,
`{Light, 104°, 40}` and `{Dark, 110°, 35}` reproduce them. That is the
compatibility guarantee that allowed those CSS blocks to be deleted, and
`the_retired_themes_survive_as_presets` guards it. A persisted `theme_id` from
before the change is migrated on load (`core/settings.rs`), so an install
reading in Sepia stays in Sepia; the legacy fields are dropped on the next
write, so the migration runs once.

**Why the tint is a filter chain, not a coloured overlay.** Painting a
translucent colour over the page washes out the black text along with the
paper — the muddy look that makes tinted reading modes unpleasant. The chain is
`sepia(t) saturate(1+t·k) hue-rotate(H−34°)`: `sepia()` collapses to a single
warm band FIRST, so near-black glyphs (almost no luminance to tint) stay black
while light paper takes the colour. This generalises the trick the two
hand-written themes already used rather than inventing a mechanism. The 34°
offset is `sepia()`'s own output hue, which makes `tint_hue` an absolute angle
that means the same thing on every base. `sepia()` is capped at 0.55: past that
photographs read as duotone. On Dark the invert runs BEFORE the tint so the
colour lands on the visible paper, not the pre-inversion white.

**COLOUR-SPACE TRAP.** The UI-token anchor must be sRGB-hued (`hsl(H 60% 55%)`)
because `hue-rotate()` works in sRGB. An oklch anchor at the same angle
disagrees badly — oklch 34 is pink where sRGB 34 is warm tan — so the page went
brown while the chrome went pink. The MIX stays `in oklch`, which holds
perceived lightness (and therefore text contrast) steady as strength rises. UI
mixing is deliberately gentler than the page (≈half), and ink moves least of
all, because chrome is a large flat area where the strength that looks right on
paper is overwhelming.

**Grain blend direction.** The overlay used `mix-blend-mode: overlay`, which is
nearly a NO-OP at both ends of the range: it preserves highlights and shadows,
so mid-gray noise over white paper stayed white and over a near-black theme
stayed black. Measured, 80% grain on `#fff` and on `#131316` were both
indistinguishable from no grain — the control did nothing on exactly the two
backgrounds people read on. It now follows the same family rule as the paper
textures: `multiply` on light, `screen` on dark. `verify-appearance.mjs` proves
it by diffing real screenshots, not by asserting a class name.

**Animated grain is a transform, not a re-seed.** Re-randomising `feTurbulence`
per frame re-rasterises a full-viewport SVG filter 60×/s and drops frames while
scrolling. Instead an oversized tile is MOVED with `steps(1)` sampling: smooth
sliding reads as a texture panning across the screen, while discrete jumps read
as real frame-to-frame grain, and a pure transform stays on the compositor.
Disabled under `prefers-reduced-motion` — constant peripheral motion is exactly
the vestibular-discomfort case — while the grain itself remains.

**Preset thumbnails are real miniature pages.** The swatch root carries that
preset's appearance as inline custom properties (`Appearance::preview_style`);
because custom properties inherit, everything inside resolves against THAT look
while a different one is applied to the document. The swatch mirrors the real
DOM — unfiltered themed backdrop, filtered stand-in canvas on top — because the
filter belongs to the canvas alone. Filtering the whole swatch inverted the
backdrop too, so every dark preset rendered as a light rectangle.

**Toolbar glyph blend must not leak into popovers.** `.toolbar-glass span`
applies `mix-blend-mode: difference` so glyphs auto-invert against the glass.
The old opt-out (`.menu-popover span { mix-blend-mode: revert }`) had lower
specificity than the rule it tried to override, so it never won — dropdown
glyphs were difference-blended all along. Harmless for text, but it inverted
the thumbnails: a white "Light" swatch differenced against a light popover
renders BLACK. Popover contents are now excluded at the source with `:not(:is(
.menu-popover, .menu-popover *))`.

### Appendix 11 — clickable links

Link targets live in a page's ANNOTATIONS, a separate stream from both the
content and the text layer. The reader only ever built a canvas and a text
layer, so a URL was rendered as pixels with selectable text over it and nothing
else: it looked like a link and did nothing. Regex-scanning the text layer is
not a substitute — it would miss real links whose anchor text is not a URL, and
invent links the document never declared.

`buildLinkLayer` (public/pdfEngine.js) is built detached and swapped in, for the
same reason the text layer is: a superseded render must never drop half a set of
anchors on top of the live ones. Rects are mapped with
`viewport.convertToViewportPoint` on BOTH corners and then normalised — the
older `convertToViewportRectangle` helper is absent from the vendored build, and
after a rotation the PDF's bottom-left corner may not be the min corner on
screen.

**Security.** External URLs are allow-listed to `http/https/mailto` and opened
with `target=_blank rel="noopener noreferrer"`. A PDF is untrusted input and can
carry any URI, including `javascript:` (script execution in our origin) and
`file:` (local filesystem probing); an allow-list makes a new exotic scheme
inert by default. Without `noopener` the opened page gets a handle on this
window via `window.opener`.

**Internal jumps go through Rust.** The anchor dispatches a
`pdfreader:navigate` CustomEvent that `effects/link_nav.rs` turns into a
`viewer.page` write — the same entry point the outline and thumbnails use — so
there is one source of truth for the current page and the existing jump/settle
logic is reused. The destination is clamped to the document: a malformed or
stale target must not scroll into the void.

**Stacking.** The link layer is z-index 3, above the text layer's 2, because the
text layer's transparent spans cover the whole page and would otherwise eat
every click. Only the anchors take `pointer-events`, so text selection still
works everywhere except the few pixels that are a link.

### Appendix 12 — tinting the UI without destroying it

**Preserve lightness; move only hue and chroma.** The first tint used
`color-mix(in oklch, <base>, <tint> N%)`. Mixing *toward* a mid-lightness
colour drags every token toward THAT lightness, so at 90% strength in light
mode paper (L=1.00), surface (0.97) and line (0.93) all landed near 0.88 — they
converged, and the page, sidebar, toolbar and thumbnail cards became one flat
slab with no edges. The accent also stayed blue on a green tint, because mixing
a saturated blue 31% toward green barely moves its hue. Dark mode hid both
problems: its bases start low, so being pulled up still left them dark enough
to read as chrome, which is why it "looked fine in dark, wrong in light".

`ui_overrides` now converts each base to OKLCH (`core/oklch.rs`), keeps L
exactly, rotates H toward the tint by `strength` (the SHORT way round the
circle, so 350° -> 10° does not sweep the spectrum), and raises C toward a
per-token ceiling. Because L never moves, contrast ratios are preserved by
construction at any strength and the ladder survives 100%. Per-token ceilings
keep large flat areas restrained and let accents saturate; ink is capped near
zero because coloured ink on coloured paper is what makes tinted themes murky.

**sRGB vs OKLCH hue.** `tint_hue` is an sRGB angle (the page tint is applied
with `hue-rotate()`, which works in sRGB). OKLCH's circle is rotated relative
to it and not by a constant — sRGB 34° is a warm tan, OKLCH 34° is pink.
`ui_hue_oklch` converts a fully saturated sRGB colour at the requested angle
and reads back its OKLCH hue, so one slider drives page and chrome to the same
colour.

**Toolbar glyph auto-inversion is opt-in.** Every toolbar span used to be
white + `mix-blend-difference` so it would invert against whatever scrolled
under the glass. That works for near-black or near-white ink but FAILS for
mid-grey: `--color-muted` differenced against the light glass resolves to
near-white, and the "/ 12" page total rendered at a measured luminance spread
of 7/255 — invisible — while the input beside it (opaque background, never
blended) stayed dark. Only `.glass-invert` blends now; everything else keeps
its themed colour, which is safe because the header is `bg-surface/60` and
every token is already chosen for contrast against surface. The status bar is
unaffected: its blend lives on the footer, over the page itself.

**Animated grain in a thumbnail must overhang.** `noise-crawl` translates by up
to 14px and a swatch is ~49x65px, so at `inset: 0` the tile's own edge was
dragged into frame — the Cinema thumbnail showed a hard vertical seam and a
bare corner. The swatch grain uses `inset: -100%` (as the full-page overlay
already did) plus a 40px tile, since the 160px tile scaled into a 49px swatch
shows only a few blobs and reads as smudges.

### Appendix 13 — the sidebar follows the reader

Both panels rendered from the top, so on a long document the reader had to hunt
for their position and the outline highlight was useless until you scrolled to
it.

**Passive follow is conditional.** `reveal_in_scroll_parent` scrolls ONLY when
the target is off screen. Unconditional centring on every page change would
yank the list out from under someone reading down it — the row they were about
to click slides away.

**The explicit gesture centres unconditionally.** Re-clicking the ACTIVE rail
tab dispatches `pdfreader:reveal-active`; both panels listen and centre on the
reader's position. Re-click no longer closes the panel — the toolbar's Toggle
sidebar button is for closing, and a reader who has scrolled the panel away has
no other way back. The thumbnail panel clears its user-drive grace when
honouring this, since being mid-scroll is exactly how you get lost.

**Retry across frames.** The reveal effect and the row list are driven by the
same signals, so the effect can run before the rows for the new page exist, and
one `requestAnimationFrame` is not always enough (rows are re-keyed, so the
node for an index may still be the old one). The effect retries up to 4 frames
and stops at the first laid-out row.

**Harness note.** Both panels stay MOUNTED (the inactive one is
`visibility:hidden`), so "are there outline rows in the DOM" is TRUE even while
Thumbs is showing. A test that uses that as its "is Outline open?" check will
click the tab twice and toggle straight back. Check computed `visibility`
instead. The rail buttons are toggles and their active state is not a stable
class token.

### Appendix 14 — a thumbnail's backdrop IS its paper

`--thumb-bg` must resolve to `--color-paper`, the same backdrop the real page
host uses. Not `--color-surface`, and not a mix with `--color-line`.

**Why.** A thumbnail canvas has no `.pdf-page` host, so it re-implements the
theme's canvas pipeline: `filter: var(--canvas-filter)` +
`mix-blend-mode: var(--canvas-blend)` over `.thumb-card`'s background. In the
light bases that blend is `multiply`, and multiplying a white page pixel by the
backdrop returns THE BACKDROP. So whatever `--thumb-bg` is, that is literally
the paper colour the reader sees in every thumbnail — and the page's paper is
`--color-paper`.

While the themes were fixed palettes, surface and paper were close enough that
the mismatch was invisible. The computed tint (appendix 12) pulls them apart on
purpose — paper keeps L=1.00 while surface sits at 0.967 with more chroma — so
at high tint every thumbnail rendered darker and more saturated than the page
it depicts: measured rgb(204,255,219) against the page's rgb(230,255,225).

Card separation does NOT depend on this variable: `.thumb-card` carries a ring
and the panel behind it is `--color-surface`, so a paper-coloured card still
reads as a distinct sheet in every base (verified in dark, where the card is
darker than the panel).

**Diagnostic to remember.** The user reported that the CORRECT colour flashed
for a single frame while a cell re-rendered. That is diagnostic, not
incidental: the skeleton cover is deliberately unblended and unfiltered, so it
shows the honest themed tint, and the wrong colour only appeared once the
blended canvas composited over the wrong backdrop. **A one-frame flash of the
right colour means the final composite is wrong, not the render** — look at
blend backdrops before suspecting the render path.

**Testing note.** Comparing CSS variables cannot catch this class of bug: the
filter and blend on `.thumb-canvas` and `.pdf-page canvas` were already
byte-identical, and only the backdrop underneath differed. Gate L samples real
pixels from a blank region of the page and the same relative region of a
thumbnail and requires a max channel delta <= 6.

### Appendix 15 — the page fits the space it actually has

**`shown = min(desired, fit_width)`.** Two numbers, and keeping them separate
is the whole design:

- `viewer.desired_scale` — what the READER asked for. Written only by
  `request_zoom` (every control and shortcut already funnels through it) and by
  an active fit mode, which is equally deliberate. A resize NEVER writes it;
  that is exactly what makes the memory survive a resize.
- `viewer.scale` — what is shown: the desired scale, or the width-fit scale
  when the desired one would be cropped.

`core::math::constrained_scale` is the whole rule, and it is pure so it can be
unit-tested without a browser.

**Why.** A hand-zoomed page used to keep its scale when the window narrowed or
the sidebar opened, so the document was simply CROPPED — content pushed off
screen with no affordance to get it back. It now shrinks to fit, and when the
room returns it grows back and STOPS at the reader's zoom. Growing past it
would be the app overriding a deliberate choice.

**Recompute from `desired`, never from the current scale.** The previous code
carried the zoom across a sidebar slide *proportionally*, multiplying the live
scale by the container ratio on every run. That drifted — three open/close
cycles never quite returned to the starting zoom — and it only ran during a
slide, which is why narrowing the WINDOW cropped the page instead. Deriving the
shown scale from `desired` each time makes shrinking lossless: a container
dragged through any number of intermediate widths returns to `desired` exactly.

**Scope, confirmed with the user.**
- Width only. Continuous scrolling down a page is normal reading, so height is
  not constrained; only horizontal cropping is a loss of access.
- Zooming IN past the fit stays allowed. It is a deliberate request to inspect
  detail, and every desktop reader permits it. The constraint applies to the
  space changing, not to the reader asking.
- A page that still fits is left ALONE. Shrinking a page that has room would
  mean smaller text for no reason — that is what section D of verify-zoom now
  asserts, having previously asserted the opposite.

**Guards.** A container width of `<= 1px` (mount, mid-animation) is ignored
rather than treated as "fits nothing", which would slam the page to
`MIN_SCALE`. `constrained_scale` also ignores non-finite or non-positive fit
widths for the same reason.

**Harness note.** Resize the viewer container directly (set a width on the app
root) rather than calling `setViewportSize` repeatedly — the fit effect reads
`container_size` from a ResizeObserver either way, so the code path is
identical, but repeated OS-window resizes crash the headless renderer. Large
PDFs at high zoom can also OOM the sandbox; zoom OUT to reach a fitting scale
instead of zooming in.

### Appendix 16 — exactly one animator owns the layout at a time

Two independent things write `display_scale`, and both raise `zoom_animating`:

- `zoom_system` — the rAF tween behind a zoom GESTURE (button, shortcut, preset).
- `fit_effect` — the follow for a sidebar slide or a window resize.

Because both raise the same flag, `zoom_animating` cannot arbitrate between
them. Ownership is tracked separately, by two thread-locals in
`src/effects/fit.rs`:

- `ZOOM_GESTURE` — claimed in `request_zoom` immediately after
  `desired_scale.set(target)`, released as the first statement of
  `commit_scale`, read via `gesture_owns_layout()`.
- `COMMIT_ECHO` — set by `commit_scale`, consumed once by the `fit_effect` run
  that its own signal writes trigger (`take_commit_echo()`).

**The rules, each paid for by a real bug:**

1. **`fit_effect` returns early on `gesture_owns_layout()` ALONE** — never on
   `zoom_animating && owned`. `apply_zoom` writes `fit = None` before
   `request_zoom`, so the effect re-runs at the very start of every zoom. The
   claim is taken before `zoom_system` has raised `zoom_animating`, so a
   conjunction leaves that gap open: the effect recomputed the same target,
   wrote `display_scale` straight to it, and the tween then started with
   `from == target` and had nothing to interpolate. **Every zoom snapped in one
   frame.**

2. **Every exit from `zoom_system` goes through `commit_scale`** — including the
   `(target - from).abs() < 1e-9` "nothing to move" path. That path hand-rolled
   its three signal writes and so never released the claim; `fit_effect` was
   then locked out permanently and the sidebar stopped resizing the page at all.

3. **The run after a commit reconciles nothing — it returns.** `commit_scale`
   writes `scale`/`display_scale`/`render_scale`, which re-runs `fit_effect`.
   Re-entering the slide path there raised `zoom_animating` again and armed
   another commit: a self-feeding loop, **one zoom costing 42–49 render passes**
   instead of one. Detect that run with `COMMIT_ECHO`, not by comparing
   container widths — the effect legitimately runs *twice per container size*
   during a slide (measured), so width-comparison misclassifies half of a real
   slide's frames as an echo and the sidebar stops moving the page.

4. **The shrink-to-fit ceiling (appendix 15) answers "the space got smaller",
   never "the reader asked for more."** Applying it on the post-commit run
   clamped the gesture itself: from a fit-width start every `+` was computed,
   animated, then instantly undone, so the page could never grow past the
   window. Zooming in past the fit is deliberate — the page overflows and
   scrolls. The ceiling still applies on the next genuine container change.

**Probe for smoothness:** sample `.pdf-page` `getBoundingClientRect().width`
every rAF for ~700ms after a click. 6+ distinct widths = smooth; 1–2 = a snap.
Assert distinct aspect ratios ≤ 2 in the same pass to catch the flex-squish
described in appendix 3. Keep such probes to a single short loop — several
concurrent in-page rAF accumulators destabilise the sandbox.

**Test identity:** the zoom readout is found by `button[data-zoom-readout]`, not
by its `title`. Its tooltip is reactive — when `is_space_constrained(desired,
fit_w)` holds it explains the held-back zoom ("Zoom — fitted to the window;
returns to N% when there is room"), otherwise plain "Zoom". Selecting on
user-visible prose broke the gate into `NaN%` readings. Before changing any
user-facing string, grep `scripts/verify/` for selectors that depend on it.

### Appendix 17 — an effect that writes a signal it tracks must have an OFF switch

`fit_effect` reads `zoom_animating` reactively (appendix 16) and its 120ms
debounce timer also *writes* it on the quiet path — "already rendered at this
scale, just release the gate". Those two facts together are a cycle:

```
timer fires -> zoom_animating.set(false) -> fit_effect re-runs
            -> arms another timer -> timer fires -> ...
```

Leptos notifies subscribers on `set` even when the value is unchanged, so
writing `false` over `false` still re-triggers. The loop ran at ~33 renders a
second forever.

It hid well. The page never moved, so every width-sampling probe reported the
layout as perfectly stable — the symptom was pure render churn, visible to the
reader as constant flicker but invisible to the existing gates. Reproduce it by
zooming IN past the fit in a narrow window and then widening: the page lands on
a scale that already fits and is already rendered, so every run takes the quiet
path. Clicking zoom in/out appears to "fix" it because a gesture ends in
`commit_scale`, whose `COMMIT_ECHO` makes the next run return early.

**Rules:**

1. **Return before arming the timer when there is nothing to do.** If `target`
   already equals both `render_scale` and `display_scale` and `zoom_animating`
   is already false, the layout is settled: do not arm a debounce that will
   write a signal this effect tracks.
2. **Never write a tracked signal unconditionally from that timer.** The quiet
   path is guarded by `if zoom_animating.get_untracked()`, so the write only
   happens when it actually changes something.

Both are needed: (1) stops the cycle starting, (2) stops it if some other path
arms the timer on a settled layout.

**Gate:** verify-zoom section M, "a settled layout renders nothing while idle" —
counts `renderPage` calls for 2.5s after everything settles and requires ≤ 2.
Measured 100 before the fix, 0 after. Any effect that both reads and writes a
signal needs a check of this shape; a width/geometry assertion cannot see it.
