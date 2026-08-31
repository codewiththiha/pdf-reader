// Raster baking: apply the appearance filter + paper blend to raw page
// pixels on the CPU. Hardware `ctx.filter` is not byte-identical across
// WebKit/Blink and would regress dark-mode inversion (the reason this
// baker exists). Intermediates recycle the shared scratch canvas so a bake
// never pins a second full-page buffer after it returns.
//
// The filter arrives as a composed matrix, not a CSS string: Rust owns the
// pipeline definition (`pdf_core::appearance::filter`) and pushes it through
// `setFilterMatrix`. What used to happen here — regex-parsing the CSS string
// Rust had just produced back into the same nine coefficients — lives in
// `./filter` now, and only as a fallback for a consumer with no Rust on the
// other end.

import type { FilterMatrix, PipelineCache } from "../types";
import {
  acquirePooledCanvas,
  acquireScratch,
  blitInto,
  isSharedScratch,
  showBaked,
  type RasterThemeTag,
  releasePooledCanvas,
  releaseScratch,
} from "../canvas";
import { pipelineIsIdentity } from "./pipeline";
import { paperInfo } from "./paper";

function applyFilterPixels(
  src: HTMLCanvasElement,
  filter: FilterMatrix
): HTMLCanvasElement {
  const { m, o } = filter;
  const identity =
    m[0] === 1 && m[1] === 0 && m[2] === 0 &&
    m[3] === 0 && m[4] === 1 && m[5] === 0 &&
    m[6] === 0 && m[7] === 0 && m[8] === 1 &&
    o[0] === 0 && o[1] === 0 && o[2] === 0;
  if (identity) return src;

  const w = src.width;
  const h = src.height;
  const sctx = src.getContext("2d");
  let img: ImageData | null | undefined;
  try {
    img = sctx && sctx.getImageData(0, 0, w, h);
  } catch (_) {
    return src;
  }
  if (!img) return src;

  const SCALE = 1 << 16;
  const luts: Int32Array[] = new Array(9);
  for (let i = 0; i < 9; i += 1) {
    const coef = m[i] ?? 0;
    const lut = new Int32Array(256);
    for (let v = 0; v < 256; v += 1) lut[v] = Math.round(coef * v * SCALE);
    luts[i] = lut;
  }
  const o0 = Math.round((o[0] ?? 0) * 255 * SCALE);
  const o1 = Math.round((o[1] ?? 0) * 255 * SCALE);
  const o2 = Math.round((o[2] ?? 0) * 255 * SCALE);
  const L0 = luts[0]!, L1 = luts[1]!, L2 = luts[2]!;
  const L3 = luts[3]!, L4 = luts[4]!, L5 = luts[5]!;
  const L6 = luts[6]!, L7 = luts[7]!, L8 = luts[8]!;
  const d = img.data;
  for (let i = 0; i < d.length; i += 4) {
    const r = d[i]!;
    const g = d[i + 1]!;
    const b = d[i + 2]!;
    d[i] = (L0[r] + L1[g] + L2[b] + o0) >> 16;
    d[i + 1] = (L3[r] + L4[g] + L5[b] + o1) >> 16;
    d[i + 2] = (L6[r] + L7[g] + L8[b] + o2) >> 16;
  }

  // Always write to a scratch copy. Mutating `src` in place destroyed the
  // unbaked thumbnail raw, so the next theme change double-filtered and
  // live thumbs could not be rebaked without a pdf.js re-render.
  const out = acquireScratch(w, h);
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) return src;
  octx.putImageData(img, 0, 0);
  return out;
}
export function rasterToCanvas(src: HTMLCanvasElement | ImageBitmap): {
  canvas: HTMLCanvasElement;
  borrowed: boolean;
} {
  if (typeof (src as HTMLCanvasElement).getContext === "function") {
    return { canvas: src as HTMLCanvasElement, borrowed: false };
  }
  const c = acquirePooledCanvas(
    (src as ImageBitmap).width,
    (src as ImageBitmap).height,
  );
  const ctx = c.getContext("2d", { alpha: false });
  if (ctx) ctx.drawImage(src as CanvasImageSource, 0, 0);
  return { canvas: c, borrowed: true };
}
export function bakeRaster(
  src: HTMLCanvasElement,
  pipeline: PipelineCache
): HTMLCanvasElement {
  if (pipelineIsIdentity(pipeline)) return src;

  let filtered: HTMLCanvasElement = src;
  if (pipeline.filter !== "none") {
    // No matrix means the filter named nothing this engine can bake (the
    // string fallback found no recognised token, or a malformed push was
    // rejected). Leave the raster alone rather than baking with identity.
    if (pipeline.matrix) filtered = applyFilterPixels(src, pipeline.matrix);
  }
  if (pipeline.blend === "normal") return filtered;

  const out = acquirePooledCanvas(src.width, src.height);
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) {
    return filtered;
  }
  octx.globalCompositeOperation = "source-over";
  octx.fillStyle = paperInfo(pipeline).color;
  octx.fillRect(0, 0, out.width, out.height);
  octx.globalCompositeOperation = pipeline.blend as GlobalCompositeOperation;
  octx.drawImage(filtered, 0, 0);
  octx.globalCompositeOperation = "source-over";
  if (filtered !== src && isSharedScratch(filtered)) {
    releaseScratch(filtered);
  } else if (filtered !== src) {
    releasePooledCanvas(filtered);
  }
  return out;
}

/** Paint a baked raster into `dst` and immediately free any intermediate. */
export function bakeInto(
  dst: HTMLCanvasElement,
  src: HTMLCanvasElement,
  pipeline: PipelineCache,
  visibleTag?: RasterThemeTag,
): void {
  const baked = bakeRaster(src, pipeline);
  if (baked !== dst) {
    if (visibleTag) showBaked(dst, baked, visibleTag);
    else blitInto(dst, baked);
    if (baked !== src) {
      if (isSharedScratch(baked)) releaseScratch(baked);
      else releasePooledCanvas(baked);
    }
  }
  if (visibleTag) dst.classList.remove(visibleTag);
}
