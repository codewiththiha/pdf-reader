# Architecture

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

- page gap and render budget constants
- toolbar inset and view-mode state
- page rendering, text/search overlays, and chrome
- measurement storage in `css_heights`

`css_heights` is the shared measurement store. It seeds the virtualizer, receives measured page heights, and is rescaled by the viewer engine — every frame in the horizontal strip, once at the commit in the others. Geometry queries themselves go through the virtualizer and the layout APIs rather than through a parallel app-local model.

## Reader motion principles

1. How a zoom reaches the screen is the view mode's choice, because the two
   scroll modes are laid out by different machinery. Both write the same
   live display scale, so nothing downstream has to know which one ran.
2. HORIZONTAL animates the LAYOUT: every frame rescales the strip through the
   viewer engine, and the virtualizer's own rescale anchor holds the reader's
   view steady while the item sizes underneath it move. Nothing is captured
   before the zoom and nothing is restored after it.
3. VERTICAL and the PAGINATED modes animate ONE continuous CSS transform of
   the whole content surface (the zoom stage), pivoted on the centre of the
   page being read. Their layout is untouched for the whole tween; the commit
   replaces the transform with real geometry at exactly the same visual size
   and restores that page centre on the same screen pixel.
4. Page hosts stretch the bitmap they already hold. Nothing re-rasterises
   while the scale is moving; the crisp render is issued once, at the settled
   scale, when the transition commits.
5. The virtualizer's window never drives the page number mid-zoom.
6. Recently evicted virtual items become zombies briefly (bounded set,
   grace longer than the tween), bridging the window across a zoom.
7. Zombie items never trigger a new PDF render; they keep their DOM and
   their last bitmap until their grace expires.
8. Page turns and reader surfaces (popovers, search, toasts) do not
   fade, slide or bounce in — document content appears instantly.
9. Layout chrome (the sidebar width, overlays) may use short STRUCTURAL CSS
   transitions; decorative entrance keyframes do not come back.
10. PDF renders happen only at transaction boundaries, never per frame.

## Continuous reader flow

1. `ReaderPage` builds one `Virtualizer` for the continuous surface.
2. `PageList` binds the scroll container, renders `v.items()`, and reports measured page heights back into both `css_heights` and the virtualizer.
3. Navigation sync uses the virtualizer for dominant-page tracking and page-to-scroll jumps.
4. Search reveal uses virtualizer offsets plus virtualizer scroll commands.
5. Zoom runs through one controller: commands resolve to a target, and the tween drives the live display scale frame by frame — relaying the horizontal strip's layout out through the engine, or scaling the other modes' content surface through one CSS transform — before one commit installs the geometry and the render scale.

## Thumbnail panel flow

The thumbnail sidebar is a separate grid virtualizer:

- width-aware row windowing lives in `virtual-list`
- DOM/reactive wiring lives in `virtual-list-leptos`
- panel-specific constants stay in `src/components/panels/thumbnails`

That keeps list and grid virtualization on the same geometry stack while letting each surface keep its own rendering policy.
