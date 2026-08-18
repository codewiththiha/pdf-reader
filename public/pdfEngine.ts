// =====================================================================
// pdfEngine.ts — window.PDFReader facade.
//
// Implementation lives in public/engine/* (loader, renderer, thumbnails,
// search, theme). This file only wires the public API and document teardown.
// Compiled to public/pdfEngine.js; the browser loads it as an ES module.
// =====================================================================

export {};

import type { PDFReaderApi, Stats } from "./engine/types";
import { disposeScratch, releaseCanvas } from "./engine/canvas";
import { coverDataUrl, open, takePendingFile } from "./engine/loader";
import {
  cancelPage,
  registerPage,
  renderPage,
  rerenderLivePages,
  unregisterPage,
} from "./engine/renderer";
import {
  blitThumb,
  cancelThumb,
  hasThumb,
  renderThumb,
} from "./engine/thumbnails";
import {
  buildSearchIndex,
  clearHighlights,
  search,
  setActiveMatch,
  setHighlightMode,
} from "./engine/search";
import {
  rebakeTheme,
  applyScrubMode,
  invalidatePipeline,
} from "./engine/theme";
import { installSelectionTracker } from "./engine/selection";
import {
  ENGINE_VERSION,
  loadingTask,
  releasePageSurfaces,
  releaseThumbEntry,
  setCurrentPath,
  setLoadingTask,
  setNumPages,
  setPdf,
  stateByCanvasId,
  THUMB_CACHE_MAX,
  thumbCache,
  thumbCancelled,
  thumbLive,
  thumbTasks,
  textIndex,
  highlightsByPage,
  setSearchQuery,
  setActiveMatchValue,
} from "./engine/state";

declare global {
  interface Window {
    pdfjsLib: unknown;
  }
  // eslint-disable-next-line no-var
  var pdfjsLib: unknown;
  // eslint-disable-next-line no-var
  var __TAURI__:
    | {
        core: {
          invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
          convertFileSrc: (p: string) => string;
        };
      }
    | undefined;
  // eslint-disable-next-line no-var
  var PDFReader: PDFReaderApi;
}

async function destroy(): Promise<void> {
  for (const st of stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    releasePageSurfaces(st);
  }
  stateByCanvasId.clear();
  for (const task of thumbTasks.values()) {
    try { task.cancel(); } catch (_) { /* ignore */ }
  }
  thumbTasks.clear();
  thumbCancelled.clear();
  thumbLive.clear();
  for (const entry of thumbCache.values()) releaseThumbEntry(entry);
  thumbCache.clear();
  textIndex.clear();
  highlightsByPage.clear();
  setSearchQuery("");
  setActiveMatchValue(null);
  if (loadingTask) {
    const lt = loadingTask;
    setLoadingTask(null);
    Promise.resolve(lt.destroy()).catch(() => { /* fire-and-forget */ });
  }
  setPdf(null);
  setNumPages(0);
  setCurrentPath(null);
  disposeScratch();
}

(globalThis as unknown as { __pdfDestroy?: () => Promise<void> }).__pdfDestroy = destroy;

async function refreshTheme(): Promise<void> {
  // A distinct raw raster (raw !== live canvas) can be re-baked in place.
  // Otherwise the live pixels may already be themed, so we re-render from
  // pdf.js instead of double-filtering.
  let canBake = false;
  for (const st of stateByCanvasId.values()) {
    if (!st.dead && st.rawCanvas && st.rawCanvas !== st.canvas) {
      canBake = true;
      break;
    }
  }
  invalidatePipeline();
  if (canBake) await rebakeTheme();
  else await rerenderLivePages();
}

function stats(): Stats {
  return {
    pages: stateByCanvasId.size,
    thumbs: thumbCache.size,
    thumbLimit: THUMB_CACHE_MAX,
    thumbTasks: thumbTasks.size,
  };
}

function releaseAllSurfaces(): void {
  for (const st of stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    releasePageSurfaces(st);
  }
  for (const entry of thumbCache.values()) releaseThumbEntry(entry);
  try {
    document.querySelectorAll("canvas").forEach((c) => releaseCanvas(c as HTMLCanvasElement));
  } catch (_) { /* document already torn down */ }
  disposeScratch();
}

globalThis.addEventListener("pagehide", releaseAllSurfaces);
installSelectionTracker();

globalThis.PDFReader = {
  version: () => ENGINE_VERSION,
  releaseAllSurfaces,
  open,
  destroy,
  registerPage,
  unregisterPage,
  cancelPage,
  renderPage,
  renderThumb,
  cancelThumb,
  hasThumb,
  blitThumb,
  coverDataUrl,
  stats,
  buildSearchIndex,
  search,
  setActiveMatch,
  setHighlightMode,
  clearHighlights,
  refreshTheme,
  setScrubMode: applyScrubMode,
  takePendingFile,
} satisfies PDFReaderApi;
