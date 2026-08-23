// Theme refresh + appearance-scrub mode: re-bake every live page and
// thumbnail after a theme change, and swap raw/unbaked pixels in/out while
// the appearance sliders are being dragged.

import { bakeInto } from "./bake";
import { blitInto } from "../canvas";
import {
  dropRawIfIdle,
  setThemeScrubActive,
  themeScrubActive,
  stateByCanvasId,
  thumbCache,
  thumbLive,
} from "../state";
import { readPipeline } from "./pipeline";
import { ensureEntryCurrent, paintAllVisibleThumbs } from "./thumbnails";
import { preparePagesForScrub, rerenderLivePages } from "../renderer";

export async function rebakeTheme(): Promise<void> {
  if (themeScrubActive) return;
  const pipeline = readPipeline();

  for (const st of stateByCanvasId.values()) {
    // Only re-bake from a DISTINCT raw raster. If raw === live canvas the
    // pixels may already be themed; baking again double-filters.
    if (st.dead || !st.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) continue;
    bakeInto(st.canvas, st.rawCanvas, pipeline);
    st.canvas.classList.remove("canvas-raw");
    dropRawIfIdle(st);
  }

  for (const entry of thumbCache.values()) {
    await ensureEntryCurrent(entry);
    if (themeScrubActive) return;
  }

  // `paintCached` selects baked displays while scrub is off, retaining the
  // stale baked canvas until each async replacement is ready.
  paintAllVisibleThumbs();
}

/**
 * Enter and leave the live-CSS appearance pipeline as one atomic operation.
 * This function is called only through pdfEngine's serialized theme queue.
 */
export async function setScrubModeInternal(on: boolean): Promise<void> {
  if (themeScrubActive === on) return;

  if (on) {
    // The class must precede both raw page renders and raw thumbnail blits.
    // That preserves `(raw pixels) ⇔ (live CSS)` for every visible frame.
    document.documentElement.classList.add("appearance-scrubbing");
    setThemeScrubActive(true);
    await preparePagesForScrub();
    // `preparePagesForScrub` only renders pages that no longer retain a
    // distinct raw backing. Restore existing raws as well, but only after
    // the class is active so no raw frame can reach the compositor unthemed.
    for (const st of stateByCanvasId.values()) {
      if (st.dead || !st.canvas) continue;
      const raw = st.rawCanvas;
      if (raw && raw !== st.canvas) {
        blitInto(st.canvas, raw);
      }
      st.canvas.classList.add("canvas-raw");
    }
    paintAllVisibleThumbs();
    return;
  }

  // Keep the class up while async bakes replace raw rasters. A page whose
  // live canvas became its only raw backing during scrub cannot be baked in
  // place without double-filtering, so re-render it before releasing CSS.
  const needsRerender = [...stateByCanvasId.values()].some(
    (st) => !st.dead && !!st.canvas && (!st.rawCanvas || st.rawCanvas === st.canvas),
  );
  setThemeScrubActive(false);
  await rebakeTheme();
  if (needsRerender) await rerenderLivePages();
  document.documentElement.classList.remove("appearance-scrubbing");
}
