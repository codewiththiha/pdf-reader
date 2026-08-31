// Bounded concurrency for the engine's fan-out work.
//
// Three places in the engine need "run N jobs, at most K at a time": the
// whole-document search scan, outline destination resolution, and the
// theme re-bake of live pages. They all share one requirement — the results
// must come back in the ORDER THE JOBS WERE QUEUED, not the order they
// finished, because document order is the contract the Rust side depends on
// (`pdf_core::search` documents "one entry per occurrence in document order").
//
// `runLimited` gets that by writing each result to its own index rather than
// pushing. Do not "simplify" it to a `push()`.

/** Run `jobs` with at most `limit` in flight. Results are indexed by job
 *  position, so `out[i]` is always `jobs[i]`'s answer. */
export async function runLimited<T>(jobs: Array<() => Promise<T>>, limit = 2): Promise<T[]> {
  const out: T[] = [];
  let i = 0;
  const workers = Array.from(
    { length: Math.min(Math.max(limit, 1), Math.max(jobs.length, 1)) },
    async () => {
      while (i < jobs.length) {
        const idx = i;
        i += 1;
        const job = jobs[idx];
        if (job) out[idx] = await job();
      }
    },
  );
  await Promise.all(workers);
  return out;
}

/** Pages scanned at once during a whole-document search. Extraction is
 *  lighter than a raster render but there are far more of it; 3 keeps a cold
 *  scan fast without outrunning the worker the live render path needs. */
export const SEARCH_CONCURRENCY = 3;

/** Outline destinations resolved at once. These are xref lookups rather than
 *  text extraction, so they tolerate a little more overlap than a search —
 *  but `resolveOutline` runs while the reader renders its first pages, so it
 *  stays bounded. */
export const OUTLINE_CONCURRENCY = 4;

/** Live pages re-rendered at once after a theme change. Matches the
 *  pre-existing limit this helper was extracted from. */
export const RERENDER_CONCURRENCY = 2;
