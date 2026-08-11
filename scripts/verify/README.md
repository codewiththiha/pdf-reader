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

## `verify.mjs` — engine (23 checks)

Serves the repo and drives `public/pdfEngine.js` + real pdf.js against
`samples/sample.pdf`, in a DOM that mirrors `PageCanvas` / `ThumbCell`.

| group | asserts |
|---|---|
| text layer | one `.textLayer` and a stable span count after 3 racing renders; no span drawn twice at the same position |
| selection | text layer is transparent; `::selection` paints no glyph color over a translucent background; **selecting adds zero new dark pixels** (a second copy of the text would add many) |
| highlights | no duplicate boxes; every box within 1.5px of its span; boxes not re-transformed by the vendored span rule |
| thumbnails | cold probe misses, first render is real, remount is served `cached:true`, the remounted canvas is **painted synchronously**, cache is scale-keyed, `cancelThumb` preserves it, `destroy()` clears it |

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

## Fixtures

`public/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf` is generated
with `/Title (file:///F|/Mis%20docum)` to reproduce the reported metadata bug
exactly; `Good Title Book.pdf` carries a legitimate title as the control.
