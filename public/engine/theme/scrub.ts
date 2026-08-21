// Theme refresh + appearance-scrub mode: re-bake every live page and
// thumbnail after a theme change, and swap raw/unbaked pixels in/out while
// the appearance sliders are being dragged.

import { bakeInto } from "./bake";
import { blitInto, el } from "../canvas";
import {
  dropRawIfIdle,
  setScrubbing,
  scrubbing,
  stateByCanvasId,
  thumbCache,
  thumbLive,
} from "../state";
import { readPipeline } from "./pipeline";
import { ensureEntryCurrent, paintAllVisibleThumbs, thumbRaw } from "./thumbnails";

export async function rebakeTheme(): Promise<void> {
  try {
    await refreshThemeInternal();
  } catch (e) {
    const msg = (e as { message?: string })?.message ?? e;
    console.warn("[pdfEngine] refreshTheme failed:", msg);
  }
}

/** Blit every cached thumb onto its live DOM canvas (and any stray `.thumb-canvas`). */
async function refreshThemeInternal(): Promise<void> {
  if (scrubbing) return;
  const pipeline = readPipeline();

  for (const st of stateByCanvasId.values()) {
    // Only re-bake from a DISTINCT raw raster. If raw === live canvas the
    // pixels may already be themed; baking again double-filters.
    if (st.dead || !st.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) continue;
    bakeInto(st.canvas, st.rawCanvas, pipeline);
    dropRawIfIdle(st);
  }

  for (const entry of thumbCache.values()) {
    await ensureEntryCurrent(entry);
    if (scrubbing) return;
  }

  paintAllVisibleThumbs();
}
export async function applyScrubMode(on: boolean): Promise<void> {
  try {
    await setScrubModeInternal(on);
  } catch (e) {
    const msg = (e as { message?: string })?.message ?? e;
    console.warn("[pdfEngine] setScrubMode failed:", msg);
  }
}
async function setScrubModeInternal(on: boolean): Promise<void> {
  if (on) {
    setScrubbing(true);
    // Restore UNBAKED pixels first. Adding the CSS class while the canvas
    // still holds a baked Dark/Dim raster double-applies invert/brightness
    // (the "flash to light" / "goes dimmer" glitch).
    for (const st of stateByCanvasId.values()) {
      if (st.dead || !st.canvas) continue;
      const raw = st.rawCanvas;
      if (raw && raw !== st.canvas) blitInto(st.canvas, raw);
    }
    for (const [canvasId, { page }] of thumbLive) {
      const entry = thumbCache.get(page);
      const live = el(canvasId) as HTMLCanvasElement | null;
      const raw = entry ? thumbRaw(entry) : null;
      if (live && raw) blitInto(live, raw);
    }
    document.documentElement.classList.add("appearance-scrubbing");
    return;
  }

  if (!scrubbing) return;
  setScrubbing(false);

  const pipeline = readPipeline();
  for (const [canvasId, { page }] of thumbLive) {
    const entry = thumbCache.get(page);
    if (!entry) continue;
    if (!(await ensureEntryCurrent(entry))) continue;
    if (scrubbing) return;
  }
  for (const st of stateByCanvasId.values()) {
    if (st.dead || !st.canvas) continue;
    const raw = st.rawCanvas;
    if (!raw) continue;
    bakeInto(st.canvas, raw, pipeline);
    dropRawIfIdle(st);
  }
  paintAllVisibleThumbs();
  document.documentElement.classList.remove("appearance-scrubbing");
}
