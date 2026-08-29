// Dominant paper colour discovery for the blend backdrop, in three scopes.
//
// The canvas background is the PDF's own paper — whatever colour the file
// ships (white, cream, scanned grey) — run through the theme pipeline. No
// app-side colour (the --color-paper token, the tint result, the engine's
// paper composite) can equal it, because all of those ignore the document's
// intrinsic paper. So we sample the raw raster, find its dominant colour,
// and publish it as --pdf-paper; the shell then paints that colour through
// the EXACT same filter + mix-blend-mode the raw canvases use, over the same
// app-paper backdrop they blend against. Same inputs, same pipeline ⇒
// backdrop = canvas background by construction, at any slider position and
// in the same frame.
//
// The reader chooses WHERE the colour comes from (layout.blend_scope):
//
// * "single" — the original behaviour. The dominant colour of the first page
//   that renders (≤3 attempts) stands in for the whole document.
// * "document" — one colour for the whole book: every page is sampled (up
//   to BLEND_SCAN_MAX_PAGES), the buckets are pooled across pages, and the
//   largest bucket overall wins. The single-page colour paints the backdrop
//   as an interim until the scan lands.
// * "continuous" — a colour PER PAGE. Every raster that renders is sampled
//   for its own page, the next page's colour is resolved ahead of the
//   reader (a tiny offscreen render, so it is known before they arrive),
//   and the Rust shell reports how far the viewport has travelled between
//   the two; the backdrop is the linear blend of the pair at that progress.
//
// The first two scopes PERSIST their result per document path, so reopening
// a book reuses the colour it already found with zero sampling work.
//
// Cost: one ≤96×96 downscale and one pixel readback per sampled raster, at
// most three attempts per document in the single scope. A page dominated by
// artwork has no paper to find, in which case that sample simply
// contributes nothing and the backdrop keeps whatever it already holds.

import {
  blendPair,
  blendScope,
  currentPath,
  numPages,
  pagePapers,
  pdf,
  resetBlendPair,
  setBlendMixValue,
  setBlendPairValue,
  setBlendScopeValue,
  setDetectedPaper,
} from "./state";
import type { BlendScope } from "./types";
import { releaseCanvas } from "./canvas";

const MAX_TRIES = 3;
/** The document scope samples at most this many pages (see
 * `BLEND_SCAN_MAX_PAGES` on the Rust side, which drives the same number). */
const BLEND_SCAN_MAX_PAGES = 100;
/** Longest edge of an offscreen sample raster. Small enough that a page
 * render for colour purposes is near-free, large enough that a paper/plain
 * region survives the downscale. */
const SAMPLE_EDGE = 96;
/** A book's paper has to own at least this share of the pooled pixels; a
 * photo-heavy document has no paper majority and keeps its interim colour. */
const PAPER_SHARE = 0.1;
/** Per-document cache: path → the colours each scope found. */
const CACHE_KEY = "pdfreader.blend-paper.v1";
const CACHE_MAX = 16;

let tries = 0;
/** This document's single-scope colour (null until a raster yields one). */
let singlePaper: string | null = null;
/** This document's document-scope colour (null until the scan lands). */
let documentPaper: string | null = null;
/** Generation token: bumping it abandons an in-flight scan. */
let scanGen = 0;
/** Generation token for per-page look-ahead sampling, so a sample started
 * for one book can never land in the next book's palette. */
let sampleGen = 0;
/** Pages whose colour an offscreen render is currently resolving. */
const sampling = new Set<number>();

// --------------------------------------------------------------------------
// Per-document cache
// --------------------------------------------------------------------------

type CacheEntry = { single?: string; document?: string };

function readCache(): Record<string, CacheEntry> {
  try {
    const raw = globalThis.localStorage?.getItem(CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, CacheEntry>;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

function cacheEntry(path: string): CacheEntry {
  return readCache()[path] ?? {};
}

/** Remember `patch` for `path`, re-inserting the path so the store stays
 * roughly least-recently-touched and pruning to CACHE_MAX books. */
function writeCache(path: string, patch: CacheEntry): void {
  if (!path) return;
  try {
    const all = readCache();
    const merged = { ...(all[path] ?? {}), ...patch };
    delete all[path];
    all[path] = merged;
    const keys = Object.keys(all);
    for (const k of keys.slice(0, Math.max(0, keys.length - CACHE_MAX))) {
      delete all[k];
    }
    globalThis.localStorage?.setItem(CACHE_KEY, JSON.stringify(all));
  } catch {
    /* storage unavailable: colours re-detect next open */
  }
}

// --------------------------------------------------------------------------
// Colour math
// --------------------------------------------------------------------------

/** Downscale `src` to ≤ SAMPLE_EDGE and hand the pixels to the bucket walk. */
function detectPaperColor(src: HTMLCanvasElement): string | null {
  const k = Math.min(SAMPLE_EDGE / src.width, SAMPLE_EDGE / src.height, 1);
  const w = Math.max(16, Math.floor(src.width * k));
  const h = Math.max(16, Math.floor(src.height * k));
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  let out: string | null = null;
  const ctx = c.getContext("2d", { willReadFrequently: true });
  if (ctx) {
    ctx.drawImage(src, 0, 0, w, h);
    try {
      const data = ctx.getImageData(0, 0, w, h).data;
      out = dominantBucket(data, w * h);
    } catch {
      out = null;
    }
  }
  releaseCanvas(c);
  return out;
}

/** Quantise into 4-bits-per-channel buckets, take the largest bucket and
 * average its members: the result is the exact mean of the dominant region,
 * not a bucket centre. Requires ≥10 % of the pixels to share it, so
 * photo-only rasters bail out instead of guessing. */
function dominantBucket(data: Uint8ClampedArray, pixels: number): string | null {
  const buckets = new Map<number, [number, number, number, number]>(); // n,r,g,b
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i], g = data[i + 1], b = data[i + 2];
    const key = ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4);
    const e = buckets.get(key);
    if (e) { e[0]++; e[1] += r; e[2] += g; e[3] += b; }
    else buckets.set(key, [1, r, g, b]);
  }
  let best: [number, number, number, number] | null = null;
  for (const e of buckets.values()) if (!best || e[0] > best[0]) best = e;
  if (!best || best[0] < pixels * PAPER_SHARE) return null; // no clear paper majority
  return rgbToHex(
    Math.round(best[1] / best[0]),
    Math.round(best[2] / best[0]),
    Math.round(best[3] / best[0]),
  );
}

function hexToRgb(hex: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim());
  return m
    ? [parseInt(m[1]!, 16), parseInt(m[2]!, 16), parseInt(m[3]!, 16)]
    : null;
}

function rgbToHex(r: number, g: number, b: number): string {
  const hx = (n: number) => n.toString(16).padStart(2, "0");
  return `#${hx(r)}${hx(g)}${hx(b)}`;
}

/** Linear blend of two hex colours; `t` 0 returns `a`, 1 returns `b`. */
function lerpHex(a: string, b: string, t: number): string | null {
  const ca = hexToRgb(a);
  const cb = hexToRgb(b);
  if (!ca || !cb) return null;
  return rgbToHex(
    Math.round(ca[0] + (cb[0] - ca[0]) * t),
    Math.round(ca[1] + (cb[1] - ca[1]) * t),
    Math.round(ca[2] + (cb[2] - ca[2]) * t),
  );
}

// --------------------------------------------------------------------------
// Offscreen sampling (the document scan + the continuous prefetch)
// --------------------------------------------------------------------------

/** Render `page` offscreen at a tiny scale and hand the raster to `sink`.
 * Best-effort: a failed render leaves the callback unfired. */
async function samplePage(
  page: number,
  sink: (raster: HTMLCanvasElement) => void,
): Promise<void> {
  const doc = pdf;
  if (!doc) return;
  try {
    const p = await doc.getPage(page);
    const vp1 = p.getViewport({ scale: 1 });
    const k = Math.min(SAMPLE_EDGE / vp1.width, SAMPLE_EDGE / vp1.height, 1);
    const c = document.createElement("canvas");
    c.width = Math.max(8, Math.floor(vp1.width * k));
    c.height = Math.max(8, Math.floor(vp1.height * k));
    const ctx = c.getContext("2d", { willReadFrequently: true });
    if (!ctx) return;
    const task = p.render({ canvasContext: ctx, viewport: p.getViewport({ scale: k }) });
    try {
      await task.promise;
      sink(c);
    } catch {
      /* cancelled: nothing to sample */
    } finally {
      releaseCanvas(c);
      try { p.cleanup(); } catch { /* already cleaned */ }
    }
  } catch {
    /* page unavailable: colour stays unknown */
  }
}

/** Pool one sampled raster's pixels into the scan's shared buckets. */
function accumulate(
  data: Uint8ClampedArray,
  buckets: Map<number, [number, number, number, number]>,
): number {
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i], g = data[i + 1], b = data[i + 2];
    const key = ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4);
    const e = buckets.get(key);
    if (e) { e[0]++; e[1] += r; e[2] += g; e[3] += b; }
    else buckets.set(key, [1, r, g, b]);
  }
  return data.length / 4;
}

/** The document scope: sample up to BLEND_SCAN_MAX_PAGES pages, pool every
 * pixel, and keep the colour owning the largest share of the whole book.
 * Runs in the background — pages are rendered one at a time with a yield
 * between them — and publishes + persists the result when it lands. */
async function scanDocumentPaper(): Promise<void> {
  const gen = ++scanGen;
  const doc = pdf;
  const path = currentPath;
  if (!doc || numPages <= 0) return;
  const limit = Math.min(numPages, BLEND_SCAN_MAX_PAGES);
  const buckets = new Map<number, [number, number, number, number]>();
  let total = 0;
  for (let page = 1; page <= limit; page += 1) {
    if (gen !== scanGen || pdf !== doc || currentPath !== path) return; // superseded
    await samplePage(page, (raster) => {
      const ctx = raster.getContext("2d", { willReadFrequently: true });
      if (!ctx) return;
      try {
        total += accumulate(ctx.getImageData(0, 0, raster.width, raster.height).data, buckets);
      } catch {
        /* unreadable page: contributes nothing */
      }
    });
    // Yield between pages so live renders never queue behind the scan.
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  if (gen !== scanGen) return;
  let best: [number, number, number, number] | null = null;
  for (const e of buckets.values()) if (!best || e[0] > best[0]) best = e;
  if (!best || total <= 0 || best[0] < total * PAPER_SHARE) return; // no book-wide paper
  const hex = rgbToHex(
    Math.round(best[1] / best[0]),
    Math.round(best[2] / best[0]),
    Math.round(best[3] / best[0]),
  );
  documentPaper = hex;
  if (path) writeCache(path, { document: hex });
  if (blendScope === "document") applyBlendPaper();
}

/** Resolve one page's colour offscreen if it is not known yet (the
 * continuous scope's look-ahead: the next page's paper is found before the
 * reader arrives at it). */
function ensurePagePaper(page: number): void {
  if (page < 1 || page > numPages || pagePapers.has(page) || sampling.has(page)) return;
  if (!pdf) return;
  const gen = sampleGen;
  sampling.add(page);
  void samplePage(page, (raster) => {
    if (gen !== sampleGen) return; // the book changed under the sample
    const hex = detectPaperColor(raster);
    if (hex) pagePapers.set(page, hex);
  }).finally(() => {
    if (gen !== sampleGen) return;
    sampling.delete(page);
    if (blendScope === "continuous") applyBlendPaper();
  });
}

// --------------------------------------------------------------------------
// Publication
// --------------------------------------------------------------------------

/** Publish the colour the active scope resolves RIGHT NOW. Unknown colours
 * never clear what is already published — the backdrop must not flash to
 * the theme paper while a sample is still in flight. */
function applyBlendPaper(): void {
  if (blendScope === "continuous") {
    const { cur, next, mix } = blendPair;
    const a = pagePapers.get(cur);
    const b = pagePapers.get(next);
    let hex: string | null = null;
    if (a && b && next !== cur) hex = lerpHex(a, b, mix);
    else hex = a ?? b ?? null;
    if (hex) setDetectedPaper(hex);
    return;
  }
  // single: the page the reader opened on. document: the book's colour,
  // falling back to the single-page interim until the scan lands.
  const hex = blendScope === "document" ? documentPaper ?? singlePaper : singlePaper;
  if (hex) setDetectedPaper(hex);
}

// --------------------------------------------------------------------------
// Public API (pdfEngine facade)
// --------------------------------------------------------------------------

/** The reader's blend scope changed (or is being restated on mount). */
export function setBlendScope(scope: BlendScope): void {
  if (blendScope === scope) return;
  setBlendScopeValue(scope);
  // Leaving the document scope abandons its in-flight scan; entering it
  // starts one unless the book's colour is already known.
  if (scope !== "document") scanGen += 1;
  if (scope === "document" && !documentPaper && pdf) void scanDocumentPaper();
  applyBlendPaper();
}

/** A new document gets a fresh attempt budget, an empty per-page palette,
 * and — when the cache has met this book before — its remembered colours,
 * published immediately so the backdrop is right on the first paint. */
export function resetPaperForDocument(): void {
  tries = 0;
  singlePaper = null;
  documentPaper = null;
  scanGen += 1; // abandon any scan still running for the previous book
  sampleGen += 1; // and any look-ahead sample
  sampling.clear();
  resetBlendPair();
  const path = currentPath;
  if (path) {
    const entry = cacheEntry(path);
    singlePaper = entry.single ?? null;
    documentPaper = entry.document ?? null;
  }
  applyBlendPaper();
  if (blendScope === "document" && !documentPaper && pdf) void scanDocumentPaper();
}

/** Abandon all in-flight sampling (document teardown). */
export function cancelPaperWork(): void {
  scanGen += 1;
  sampleGen += 1;
  sampling.clear();
}

/** Sample `src` for the document's paper colour. Called from the renderer
 * while the raster is still raw (before the theme bake tints it). */
export function maybeDetectPaper(page: number, src: HTMLCanvasElement | null): void {
  if (!src || src.width < 8 || src.height < 8) return;
  if (blendScope === "continuous") {
    if (pagePapers.has(page)) return;
    const hex = detectPaperColor(src);
    if (!hex) return;
    pagePapers.set(page, hex);
    if (page === blendPair.cur || page === blendPair.next) applyBlendPaper();
    return;
  }
  // single / document: the first raster that yields a colour wins (≤3
  // attempts). In the document scope it doubles as the interim colour.
  if (singlePaper || tries >= MAX_TRIES) return;
  tries += 1;
  const hex = detectPaperColor(src);
  if (!hex) return;
  singlePaper = hex;
  const path = currentPath;
  if (path) writeCache(path, { single: hex });
  applyBlendPaper();
}

/** The continuous scope's page pair, as the Rust shell resolves it from the
 * dominant page. The next page's colour is sampled ahead of the reader. */
export function setBlendPages(cur: number, next: number): void {
  if (blendPair.cur === cur && blendPair.next === next) return;
  setBlendPairValue(cur, next);
  if (blendScope !== "continuous") return;
  ensurePagePaper(cur);
  ensurePagePaper(next);
  applyBlendPaper();
}

/** How far the viewport has travelled from the pair's first page to its
 * second (0..1). No-ops outside the continuous scope. */
export function setBlendProgress(mix: number): void {
  if (blendScope !== "continuous") return;
  const t = Math.min(1, Math.max(0, mix));
  if (!Number.isFinite(t) || Math.abs(t - blendPair.mix) < 0.001) return;
  setBlendMixValue(t);
  applyBlendPaper();
}
