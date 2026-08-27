// Dominant paper colour discovery for the blend backdrop.
//
// The canvas background is the PDF's own paper — whatever colour the file
// ships (white, cream, scanned grey) — run through the theme pipeline. No
// app-side colour (the --color-paper token, the tint result, the engine's
// paper composite) can equal it, because all of those ignore the document's
// intrinsic paper. So we sample the raw raster once, find its dominant
// colour, and publish it as --pdf-paper; the shell then paints that colour
// through the EXACT same filter + mix-blend-mode the raw canvases use, over
// the same app-paper backdrop they blend against. Same inputs, same
// pipeline ⇒ backdrop = canvas background by construction, at any slider
// position and in the same frame.
//
// Cost: one downscale to ≤96×96 and one pixel readback, at most three
// attempts per document. A page dominated by artwork has no paper to find,
// in which case detection bails and the backdrop falls back to the app
// paper (var fallback in the CSS).

import { detectedPaper, setDetectedPaper } from "./state";

const MAX_TRIES = 3;
let tries = 0;

/** A new document gets a fresh attempt budget. Called from the loader when
 *  a document is opened. */
export function resetPaperTries(): void {
  tries = 0;
}

/** Sample `src` for the document's paper colour. Called from the renderer
 *  while the raster is still raw (before the theme bake tints it). No-ops
 *  once a colour is known or the attempt budget for this document is spent. */
export function maybeDetectPaper(src: HTMLCanvasElement | null): void {
  if (detectedPaper || tries >= MAX_TRIES || !src || src.width < 8 || src.height < 8) return;
  tries += 1;
  const hex = detectPaperColor(src);
  if (hex) setDetectedPaper(hex);
}

/** Downscale, quantise into 4-bits-per-channel buckets, take the largest
 *  bucket and average its members: the result is the exact mean of the
 *  dominant region, not a bucket centre. Requires ≥10 % of the pixels to
 *  share it, so photo-only pages bail out instead of guessing. */
function detectPaperColor(src: HTMLCanvasElement): string | null {
  const S = 96;
  const k = Math.min(S / src.width, S / src.height, 1);
  const w = Math.max(16, Math.floor(src.width * k));
  const h = Math.max(16, Math.floor(src.height * k));
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const ctx = c.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;
  ctx.drawImage(src, 0, 0, w, h);
  let data: Uint8ClampedArray;
  try {
    data = ctx.getImageData(0, 0, w, h).data;
  } catch {
    return null;
  }
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
  if (!best || best[0] < w * h * 0.1) return null; // no clear paper majority
  const r = Math.round(best[1] / best[0]);
  const g = Math.round(best[2] / best[0]);
  const b = Math.round(best[3] / best[0]);
  const hx = (n: number) => n.toString(16).padStart(2, "0");
  return `#${hx(r)}${hx(g)}${hx(b)}`;
}
