// Theme pipeline discovery: read the root CSS variables that describe the
// current appearance and cache them until the root style changes.
//
// The filter arrives in two shapes. Rust pushes the composed MATRIX through
// `setFilterMatrix` right before it writes `--canvas-filter`, which is the
// path the app uses — the engine bakes with numbers and never re-parses a
// string. The CSS string is still read (it is what the cascade and the scrub
// mode run on), and for a consumer that never pushed a matrix the engine
// parses it back as a fallback.

import type { FilterMatrix, PipelineCache } from "../types";
import { coerceFilterMatrix, parseFilter } from "./filter";
import { paperInfo } from "./paper";

export const pipelineCache: PipelineCache = {
  token: null,
  filter: "none",
  matrix: null,
  blend: "normal",
  paperInfo: null,
  gen: 0,
};

/** The matrix Rust handed over for the appearance now being painted, or null
 *  when nothing has pushed one. Held here rather than on the cache itself
 *  because it arrives BEFORE the CSS variable that invalidates the cache —
 *  invalidation must not discard it. */
let pushedMatrix: FilterMatrix | null = null;

/** Rust's composed filter matrix, or null to stop using one. Called by the
 *  theme applier immediately before it writes `--canvas-filter`. */
export function setFilterMatrix(matrix: unknown): void {
  pushedMatrix = coerceFilterMatrix(matrix);
}

export function invalidatePipeline(): void {
  pipelineCache.token = null;
  pipelineCache.matrix = null;
  pipelineCache.gen += 1;
}

export function readPipeline(): PipelineCache {
  const root = document.documentElement;
  const token = root.getAttribute("style") || "";
  // The token is the whole invalidation contract: a null matrix is a valid
  // cached answer ("this filter named nothing we can bake"), so re-deriving
  // it would just reproduce the null and burn a generation.
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
  // Pushed matrix wins; the string parser is the fallback for a consumer
  // that has no Rust on the other end.
  pipelineCache.matrix = pushedMatrix ?? parseFilter(filter);
  pipelineCache.blend = blend;
  pipelineCache.paperInfo = null;
  pipelineCache.gen += 1;
  return pipelineCache;
}

export function pipelineIsIdentity(pipeline: PipelineCache): boolean {
  if (pipeline.filter !== "none") return false;
  if (pipeline.blend === "normal") return true;
  if (pipeline.blend === "multiply") {
    const rgb = paperInfo(pipeline).rgb;
    return rgb[0] === 255 && rgb[1] === 255 && rgb[2] === 255;
  }
  return false;
}
