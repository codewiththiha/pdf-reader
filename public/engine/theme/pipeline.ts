// Theme pipeline discovery: read the root CSS variables that describe the
// current appearance and cache them until the root style changes.

import type { FilterMatrix, PipelineCache } from "../types";
import { paperInfo } from "./paper";

export const pipelineCache: PipelineCache = {
  token: null,
  filter: "none",
  blend: "normal",
  matrix: null,
  paperInfo: null,
  gen: 0,
};

// The structured filter matrix, handed over by the Rust theme applier
// (paint_appearance_now → PDFReader.setFilterMatrix) in the same synchronous
// task that writes `--canvas-filter`. Latest delivery wins: the app is the
// only writer of that CSS variable, so a matrix delivery and a filter-string
// change always travel together — which also means unrelated root-style
// writes (the paper variable, texture vars) can never invalidate a matrix
// that is still current.
let providedMatrix: FilterMatrix | null = null;

/** Store (or, with null, clear) the structured filter matrix. Invalidates
 *  the cached pipeline so the next read picks it up. */
export function setFilterMatrix(m: FilterMatrix | null): void {
  providedMatrix = m;
  invalidatePipeline();
}

export function invalidatePipeline(): void {
  pipelineCache.token = null;
  pipelineCache.gen += 1;
}
export function readPipeline(): PipelineCache {
  const root = document.documentElement;
  const token = root.getAttribute("style") || "";
  if (pipelineCache.token === token) return pipelineCache;
  let filter = "none";
  let blend = "normal";
  try {
    const cs = getComputedStyle(root);
    filter = (cs.getPropertyValue("--canvas-filter") || "none").trim() || "none";
    blend = (cs.getPropertyValue("--canvas-blend") || "normal").trim() || "normal";
  } catch (_) {
    /* identity */
  }
  pipelineCache.token = token;
  pipelineCache.filter = filter;
  pipelineCache.blend = blend;
  pipelineCache.matrix = providedMatrix;
  pipelineCache.paperInfo = null;
  pipelineCache.gen += 1;
  return pipelineCache;
}

/** Exact identity test for a structured filter, mirroring the raster baker's
 *  early-out: an identity transform must not touch the pixels at all. */
export function matrixIsIdentity(m: FilterMatrix): boolean {
  return (
    m.m[0] === 1 && m.m[1] === 0 && m.m[2] === 0 &&
    m.m[3] === 0 && m.m[4] === 1 && m.m[5] === 0 &&
    m.m[6] === 0 && m.m[7] === 0 && m.m[8] === 1 &&
    m.o[0] === 0 && m.o[1] === 0 && m.o[2] === 0
  );
}

export function pipelineIsIdentity(pipeline: PipelineCache): boolean {
  // The structured matrix is authoritative when it was delivered; the CSS
  // string is the engine-standalone fallback.
  if (pipeline.matrix) {
    if (!matrixIsIdentity(pipeline.matrix)) return false;
  } else if (pipeline.filter !== "none") {
    return false;
  }
  if (pipeline.blend === "normal") return true;
  if (pipeline.blend === "multiply") {
    const rgb = paperInfo(pipeline).rgb;
    return rgb[0] === 255 && rgb[1] === 255 && rgb[2] === 255;
  }
  return false;
}
