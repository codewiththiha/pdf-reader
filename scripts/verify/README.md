# Verification suite

Headless browser checks for behaviour that unit tests cannot reach: pdf.js
rendering, text-layer geometry, selection painting, and thumbnail scrolling.
Both scripts assert **user-visible invariants**, not implementation details, and
both were confirmed to FAIL against the pre-fix code — a test that cannot catch
the bug it guards is worthless.

```bash
npm i -D playwright && npx playwright install chromium   # once

node scripts/verify/verify.mjs        # engine-level (starts its own server)
trunk serve --port 1420 &             # app-level needs the app running
node scripts/verify/verify-ui.mjs
```

## `verify.mjs` — engine (29 checks)

Serves the repo and drives `public/pdfEngine.js` + real pdf.js against
`samples/sample.pdf`, in a DOM that mirrors `PageCanvas` / `ThumbCell`.

| group | asserts |
|---|---|
| text layer | one `.textLayer` and a stable span count after 3 racing renders; no span drawn twice at the same position |
| selection | text layer is transparent; `::selection` paints no glyph color over a translucent background; **selecting adds zero new dark pixels** (a second copy of the text would add many) |
| highlights | no duplicate boxes; every box within 1.5px of its span; boxes not re-transformed by the vendored span rule |
| thumbnails | cold probe misses, first render is real, remount is served `cached:true`, the remounted canvas is **painted synchronously**, cache is scale-keyed, `cancelThumb` preserves it, `destroy()` clears it |
| memory | `unregisterPage` zeros the page canvas and drops the registration; `cancelThumb` zeros the live thumb but keeps the cache; LRU bound is 64; `destroy()` leaves `stats()` at 0 |

## `verify-ui.mjs` — full app (19 checks)

Drives the running Leptos app. Documents are opened through the app's own
`toolbar::open_path` flow via the placeholder's "Open last" button (seeded
through persisted settings), so no test hooks exist in the production build.

| group | asserts |
|---|---|
| document name | a junk `/Title` (`file:///F\|/Mis%20docum`) falls back to the file name; a real title still wins; no path/URL artifacts |
| adaptive folding | full name at wide widths (no fixed 160px cap); never overlaps the right controls or the centered page nav; folds then hides as the window narrows; the budget shrinks monotonically (no oscillation) |
| thumbnails | sampling every animation frame while scrolling **up**, no cover is ever opaque over an already-rendered row and no pulse restarts |
| selection | selecting in the real viewer is visible and adds zero new dark pixels |
| console | no page/console errors for the whole run |

### Regression sensitivity (measured)

| fix disabled | result |
|---|---|
| selection CSS reverted | `402 new dark px` (engine) — the doubled text |
| `starts_cached` forced false | `504/1132 opaque cover samples, peak opacity 1.00` — the scroll flicker |

## `verify-zoom.mjs` — Preview-style zoom & navigation

`node scripts/verify/verify-zoom.mjs` (needs `trunk serve` on :1420). 14 checks.

Everything here asks one of two questions: does the thing the user is LOOKING AT
stay where it was, and did we render more times than we needed to? Renders are
counted by wrapping `PDFReader.renderPage` in the page — the same boundary the
Rust side calls through.

| group | asserts |
|---|---|
| A. anchoring | the page under the viewport centre is the same page after a zoom |
| B. retargeting | 3 fast `+` clicks advance 3 presets (none swallowed by the animation) and still cost ~one render pass, not one each |
| C. clamping | zooming at the end of the document stays within `[0, max_scroll]` |
| D. sidebar | toggling the sidebar changes neither the zoom % nor renders a single page |
| E. render count | one gesture = one crisp pass (~one render per visible page, not per frame) |
| F. navigation | `ArrowRight` advances exactly one page, the status counter agrees, and the scrollport actually lands there |
| G. reduced motion | with `prefers-reduced-motion` the zoom is instant but still anchored |

Typical numbers on the 40-page fixture at 1100x800: a zoom costs **7**
`renderPage` calls (the visible window), a 3-click burst also **7**, and a
sidebar toggle **0**.

**Sections H/I run in the DEFAULT fit-width state** — deliberately without
applying a zoom preset first, because a preset clears `FitMode::Width` and takes
a different code path. Both follow-up bug reports (the counter walking during
zoom, and the sidebar squish-then-snap) lived in exactly that gap: every earlier
check set a preset first and so never exercised the state a real user is in
right after opening a document.

| check | asserts |
|---|---|
| H. counter stability | the page counter holds still through a 4-out/4-in zoom cycle |
| I. slide continuity | the page moves through intermediate sizes (no hold-then-snap) |
| I. slide geometry | the page keeps its true aspect ratio on every frame |
| I. slide cost | the whole slide is one render pass, at the end |

**Harness note:** sections A-G start from the 100% preset. The document opens
at fit-width, which is >200% for this fixture, and repeatedly rasterising
full pages at that size crashes the headless shell's renderer — a harness
limit, not an app one. Every behaviour checked here is scale-independent.

## Fixtures

`public/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf` is generated
with `/Title (file:///F|/Mis%20docum)` to reproduce the reported metadata bug
exactly; `Good Title Book.pdf` carries a legitimate title as the control.
