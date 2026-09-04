# Architecture

> Scope note: this document covers the virtualization, motion and format-pipeline design — the
> parts of the reader with the most subtle invariants. For the feature tour, build setup and the
> crate map, see the README.

This repo now splits virtual scrolling into three layers.

## 1. `virtual-list`: pure geometry

`crates/virtual-list` owns the reusable math:

- list and grid layout contracts
- mounted-window selection from viewport + budget
- dominant-item selection
- offset and total-size queries
- anchor correction for measurement changes and uniform rescaling

It has no DOM, no framework coupling, and is the long-term public crate surface.

## 2. `virtual-list-leptos`: reactive adapter

`crates/virtual-list-leptos` wraps the geometry kernel in a Leptos-friendly adapter:

- `VirtualizerCore` is still pure and unit-testable
- `use_virtualizer` binds browser scroll containers and resize observers
- the public `Virtualizer` exposes reactive mounted items/rows, total size, padding, dominant item, scrolling state, and scroll-to APIs

This layer is responsible for DOM measurement flow, scroll scheduling, and keeping the geometry authoritative.

## 3. Reader app: policy + rendering

The app uses the adapter and keeps only app-specific policy locally:

- view-mode state, plus the format-agnostic view policy (page gap, render
  budget, spread arithmetic) that lives in `crates/reader-core`'s `view` module
- toolbar inset
- page rendering, text/search overlays, and chrome
- measurement storage in `css_heights`

`css_heights` is the shared measurement store. It seeds the virtualizer, receives measured page heights, and is rescaled by the viewer engine on every frame of a zoom. Geometry queries themselves go through the virtualizer and the layout APIs rather than through a parallel app-local model.

## Reader motion principles

1. Zoom animates the LAYOUT. Every frame of the tween rescales the strips
   through the viewer engine, so the document genuinely resizes under the
   reader's eyes.
2. The engine holds the document point under the viewport centre exactly
   where it is while it does so — computed gap-aware, because page heights
   scale and the gap between pages does not. Nothing is captured before a
   zoom and nothing is restored after it.
3. Zoom never scales a frozen surface with a CSS transform. A transform
   scales the page gaps along with the pages, the layout deliberately does
   not, and the whole accumulated difference lands at once when the transform
   is swapped for real geometry — which reads as the document jumping.
4. Page hosts stretch the bitmap they already hold. Nothing re-rasterises
   while the scale is moving; the crisp render is issued once, at the settled
   scale, when the transition commits. A container follow is what makes that
   rule non-trivial: a sidebar slide or a window drag relays the layout out on
   every frame and holds its COMMIT until the size has been quiet, so the burst
   costs one render pass rather than one per frame.
5. The virtualizer's window never drives the page number mid-zoom — and it
   catches the page up when the transaction lands, which is what keeps a long
   held follow from leaving the counter on a page the reader has scrolled past.
6. Recently evicted virtual items become zombies briefly (bounded set,
   grace longer than the tween), bridging the window across a zoom.
7. Zombie items never trigger a new PDF render; they keep their DOM and
   their last bitmap until their grace expires.
8. Page turns and reader surfaces (popovers, search, toasts) do not
   fade, slide or bounce in — document content appears instantly.
9. Layout chrome (the sidebar width, overlays) may use short STRUCTURAL CSS
   transitions; decorative entrance keyframes do not come back. Whether they run
   at all is the reader's call: Settings → Layout holds the master switch, and
   Settings → Animations holds one switch per motion the reader models. Both are
   projected into `state::reader::Motion` by the shell, which is what every gate
   reads — the master is applied once, there, and nothing downstream asks twice.
10. The page host is sized by its inline width/height, which ARE the page
    geometry, so it never lets the flex engine renegotiate them — a shrink
    takes the width while the height stays and the paper visibly squishes.
    There is no reflow exception for a resize, and none is needed: the layout
    follows the container on every frame of a slide or a drag, so the host is
    never left wider than its line for a frame the reader can see. A
    hand-picked zoom that overflows the window keeps overflowing — that is the
    page scrolling rather than squishing, exactly as a fit page in the
    horizontal strip does.
11. PDF renders happen only at transaction boundaries, never per frame.
12. A switched-off animation never skips the CHANGE, only the frames. Each gate
    sits where the interpolation happens, not where the change is decided:
    `animation.rs` declines to tween, `follow_watcher` drops the per-frame posts
    of a *gated* burst (a window drag) and lands the end frame once it is quiet,
    `scroll_mode` resolves to `Instant`, and the rail's class list gains
    `no-slide`. So a frozen reader is not a reader with fewer features — it is
    the same geometry, arriving at once. The one resize that is never gated is
    the follow of the sidebar's own width: there the frames ARE the correctness
    (a page may not be wider than the box the flex engine gave it), and landing
    in the frame the container was measured is what keeps a frozen slide to a
    single step instead of a step followed by a correction.
13. An animation that IS the information is not decoration, so the nets do not
    silence it. The loading mark keeps its loop under `prefers-reduced-motion`
    and under the master switch, as a fade in place: a spinner that stopped
    moving reports a hang, which is a different fact and a wrong one. The same
    rule decides how it moves — `transform` or `opacity`, and nothing else,
    because those are the two properties a compositor animates while the main
    thread is blocked, and the frames this mark must not miss are precisely the
    ones a document open takes. (It used to step three `background-position`
    gradients on one box; that is painted, so it froze during the parse it was
    on screen to cover.) Any animation that has to outlive the work it reports
    on is written the same way.

## Continuous reader flow

1. `ReaderPage` builds one `Virtualizer` for the continuous surface.
2. `PageList` binds the scroll container, renders `v.items()`, and reports measured page heights back into both `css_heights` and the virtualizer.
3. Navigation sync uses the virtualizer for dominant-page tracking and page-to-scroll jumps.
4. Search reveal uses virtualizer offsets plus virtualizer scroll commands.
5. Zoom runs through one controller: commands resolve to a target, the tween relays the layout out through the engine frame by frame — `css_heights`, both strips and the page hosts all follow the live display scale — and the render scale catches up once, at the end.

## Thumbnail panel flow

The thumbnail sidebar is a separate grid virtualizer:

- width-aware row windowing lives in `virtual-list`
- DOM/reactive wiring lives in `virtual-list-leptos`
- panel-specific constants stay in `src/components/shell/sidebar/panels/thumbnails`

That keeps list and grid virtualization on the same geometry stack while letting each surface keep its own rendering policy.

## Formats: one host, one pipeline per family

The reader has two axes that must not multiply: how a document is *viewed* (single,
spread, two scroll modes) and what it *is* (PDF, plain text, Markdown). The UI is split
along the first axis and the crates along the second, and exactly one file joins them.

- `src/components/viewer/` is shape: the mode dispatch, the four layouts, the shells that
  hold the scroll container. A layout may not name a format; adding a view mode touches
  this directory and `reader-core`'s `view` module, and no format crate.
- `src/components/formats/` is substance: `pdf/`, `reflow/`, `txt/`, `md/`. Adding a format
  touches this directory, one parser crate, and one match arm in the open flow.
- `src/components/viewer/page_host.rs` is the seam, and the only file in the viewer layer
  allowed to ask which format is open. `UniversalPageHost` takes a page plus a `PageSlot`
  (single, spread left, spread right) and mounts either `PdfPageCanvas` or `ReflowPage`;
  `UniversalStripHost` does the same for the virtualized strip; `UniversalStreamHost`
  answers for continuous reading, the one case where the two pipelines disagree about the
  *surface* rather than the *content*. Both page components take the same props — page,
  scale, host id, `class` — read the page texture from context, and answer for exactly one
  page number; the PDF's extras (canvas id, gloss overlay, geometry callback) are built from
  the slot inside the host, so no layout ever passes a format-specific prop. The host also
  owns the DOM identity of a page (the `sp-`/`dp-`/`hp-`/`cont-` ids), which is why the
  floating chapter label and a selection anchor address a page of Markdown exactly as they
  address a page of pixels.

The state mirrors it: `state::reader::document` holds the document's identity (path, title,
format, page count, outline) and, beside it, a `DocumentContent` with `pdf` and `reflow`
halves. Both halves publish page sizes into the same `css_heights`/`intrinsic` store, so the
virtualizers, the zoom coordinator and the progress chrome never ask who measured what.

## Text and Markdown pipeline

Plain text and Markdown share a pipeline that reuses the page machinery above the leaf
renderer. The split is deliberate: PDF pages are rasters the engine paints, text pages
are A4 hosts the reader lays out with real type — but both report the same per-page sizes into
the same virtualized strips, so view modes, zoom, navigation and search reveal stay
format-agnostic.

- `crates/reflow-core` is the shared half of that, pure (no DOM, no Leptos): the block shape
  and its splitting rules, the A4 page geometry and its spine sides, the block-granular page
  cutter, the height estimate, the typography resolution (schema lives in `reader-core`'s
  `settings::typography`) and a substring search over the blocks. `crates/txt-core` and
  `crates/md-core` sit on top of it with one parser each — normalising and paragraph-cutting
  for text, construct classification, prose subdivision, front-matter metadata and heading
  extraction for Markdown — so a format owns its syntax and nothing else. Everything is
  unit-testable on the host.
- On open, the file is parsed into blocks, oversized prose paragraphs are subdivided on line
  boundaries into continuation-flagged chunks (`subdivide`, five lines each), and an estimate cut
  is published immediately, so the reader is up the instant the bytes land. The subdivision is
  what lets the paginator pack pages tightly: no single block is taller than a few lines of type,
  so a page bottom never carries a blank band a pushed-over paragraph used to leave. A hidden
  measure column (carrying the page content's own `tx-content` rules, so it measures the reading
  face and not the browser's fallback) then renders every block once at scale 1 with the live
  typography, reads the true heights, and republishes the cut. Pagination is therefore
  measurement-true, and re-cuts whenever a typography knob moves — holding the reader on the
  block they were reading.
- `apply_heights` is the one place the cut, its inverse block→page map and the per-page size
  bookkeeping are written together, so a split can never disagree with its map.
- Zoom never re-paginates. The cut is computed at scale 1; a page host is sized `A4 × scale` and
  its type resolves through a scale-1 CSS variable times the host's own `--ts`, so the layout is
  identical at every scale and uniform scaling provably preserves the cut. During the tween the
  mounted pages reflow frame by frame at the live display scale — cheap, because the window is
  bounded — while the cut itself stays put.
- Vertical reading is the one deliberate deviation from "pages everywhere": a reflowable document
  in the vertical mode renders as the CONTINUOUS STREAM (`components::formats::reflow::stream`), which
  virtualizes the blocks themselves — not page-cut units — on the shared scroller id, with the
  window itself painted as the paper and the blocks flowing in a reading column narrowed by the
  page margin and positioned by the column-alignment setting. The page cut still backs the paged
  modes, the page bookkeeping and the resume flow; it simply is not what scrolls. The stream owns
  its zoom relayout (the page virtualizer is unbound there, and `navigation_sync`'s two page arms
  stand down), maps its dominant block back onto `viewer.page` for progress persistence, reveals
  search hits through its own virtualizer, and reports its rendered item heights back so a column
  narrower than the page model still lays out truthfully. Single, spread and horizontal keep real
  A4 sheets.
- A Markdown document gets the sidebar's outline panel for real: `md_core::headings_of_blocks`
  finds the headings among the final blocks and `effects::reader::reflow_outline` projects them
  onto the live block→page table, so the chapter tree follows every re-cut instead of going
  stale. The tree lands in the same `document.outline` signal a PDF's `/Outlines` dictionary
  fills, in the same `reader_core::outline::OutlineNode` shape — the panel cannot tell the two
  apart, which is the point.
- Format questions are asked once: `Format::is_reflowable` in `reader-core` is the predicate,
  `ReaderState::reflowable()` is the tracked read of it, and the UI never tests an extension
  or a document variant inline. The leaf renderer is the same deal one level down:
  `components::formats::block_view::BlockView` dispatches a block to the text or Markdown view
  from the document's format, so the page, the stream and the measure column share one
  answer and none of them knows what Markdown is.
- Text never enters blend mode and never touches the paper session: a text page is recoloured by
  its own tokens, so the backdrop's colour machine is gated off for the format (the Theme tab
  hides the Paper and Rendering sections accordingly). Body ink has its own comfort dial —
  `ink_contrast`, a `color-mix` of the theme's ink toward its paper, exposed as the "Text ink
  intensity" slider while a text document is open. Search scans the blocks in-process (the
  document is its own index), mapping hits through the current page cut. Progress persistence
  saves the fractional stream position alongside the page, so a continuous read resumes where it
  stopped, not at a page top.
