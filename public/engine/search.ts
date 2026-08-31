// On-demand streaming search. Pages are extracted as we scan and then
// discarded — we do not keep a permanent Map of every TextItem in the heap.
//
// The scan is bounded-parallel (SEARCH_CONCURRENCY): a 1000-page book used to
// be 1000 serialised worker round trips before the first result landed. Order
// is preserved by indexing each page's answer back to its position rather
// than by completion order, and `index` is a per-page ordinal, so no global
// counter has to be shared between the workers.
//
// The match maths below is a transcription of `pdf_core::search::match_page`,
// which is the definition of record and the place the behaviour is tested.

import type { SearchMatch, SearchRect, SearchResult, TextItem } from "./types";
import { fail } from "./canvas";
import { SEARCH_CONCURRENCY, runLimited } from "./concurrent";
import { refreshHighlights } from "./highlights";
import {
  highlightsByPage,
  numPages,
  pdf,
  setActiveMatchValue,
  setSearchQuery,
  stateByCanvasId,
} from "./state";

function itemRect(item: TextItem, pageH: number): SearchRect {
  const t = item.transform || [1, 0, 0, 1, 0, 0];
  const fontSize = Math.hypot(t[2] ?? 0, t[3] ?? 0);
  const ascent = (fontSize || 0) * 0.8;
  return {
    x: t[4] ?? 0,
    y: (pageH || 0) - (t[5] ?? 0) - ascent,
    w: item.width || 0,
    h: item.height || 0,
  };
}

function snippetText(str: string, q: string, from: number): string {
  const start = Math.max(0, from - 25);
  const end = Math.min(str.length, from + q.length + 30);
  const pre = start > 0 ? "…" : "";
  const post = end < str.length ? "…" : "";
  return pre + str.slice(start, end) + post;
}

/** One page's worth of hits, or null if the page could not be read. */
type PageScan = { matches: SearchMatch[]; rects: SearchRect[] } | null;

async function scanPage(page: number, q: string, qlen: number): Promise<PageScan> {
  if (!pdf) return null;
  try {
    const pg = await pdf.getPage(page);
    const tc = await pg.getTextContent();
    const pageH = pg.getViewport({ scale: 1 }).height;
    const pageMatches: SearchRect[] = [];
    const pageHits: SearchMatch[] = [];
    // `index` is the ordinal WITHIN the page, so it is page-local and needs
    // no coordination between the concurrent scans.
    let ord = 0;
    for (const item of tc.items) {
      if (!item.str) continue;
      const r = itemRect(item, pageH);
      if (r.w <= 0) continue;
      const lower = item.str.toLowerCase();
      const len = lower.length || 1;
      for (
        let at = lower.indexOf(q);
        at !== -1;
        at = lower.indexOf(q, at + qlen)
      ) {
        const rect: SearchRect = {
          x: r.x + (r.w * at) / len,
          y: r.y,
          w: Math.max(1, (r.w * qlen) / len),
          h: r.h,
        };
        pageMatches.push(rect);
        pageHits.push({
          page,
          index: ord,
          text: snippetText(item.str, q, at),
          ...rect,
        });
        ord += 1;
      }
    }
    pg.cleanup();
    return { matches: pageHits, rects: pageMatches };
  } catch (_) {
    /* skip unreadable page */
    return null;
  }
}

/** No-op: search() streams pages itself. Same `{ok, count}` envelope as the rest. */
export async function buildSearchIndex(): Promise<{ ok: true; count: number }> {
  return { ok: true, count: 0 };
}

export async function search(query: string): Promise<SearchResult> {
  if (!pdf) return fail("no_document", "No document open");
  const q = String(query || "").toLowerCase().trim();
  if (!q) {
    setSearchQuery("");
    setActiveMatchValue(null);
    highlightsByPage.clear();
    return { ok: true, query: "", total: 0, matches: [] };
  }

  setSearchQuery(q);
  highlightsByPage.clear();
  const qlen = q.length;

  const jobs: Array<() => Promise<PageScan>> = [];
  for (let page = 1; page <= numPages; page += 1) {
    jobs.push(() => scanPage(page, q, qlen));
  }
  const scanned = await runLimited(jobs, SEARCH_CONCURRENCY);

  // Concatenate in DOCUMENT order: `runLimited` indexed each answer back to
  // its job position, so completion order never reaches the result.
  const matches: SearchMatch[] = [];
  for (let i = 0; i < scanned.length; i += 1) {
    const hit = scanned[i];
    if (!hit) continue;
    if (hit.rects.length) highlightsByPage.set(i + 1, hit.rects);
    for (const m of hit.matches) matches.push(m);
  }

  setActiveMatchValue(null);
  refreshHighlights();
  try {
    if (pdf) await pdf.cleanup();
  } catch (_) {
    /* advisory */
  }
  return { ok: true, query, total: matches.length, matches };
}

export function setActiveMatch(page: number, index: number): void {
  const next =
    Number.isFinite(page) && page > 0 && Number.isFinite(index) && index >= 0
      ? { page, index: index | 0 }
      : null;
  setActiveMatchValue(next);
  for (const st of stateByCanvasId.values()) {
    if (!st.textLayerEl) continue;
    const wanted = next && next.page === st.page ? String(next.index) : null;
    for (const d of st.textLayerEl.querySelectorAll(".highlight") as NodeListOf<HTMLElement>) {
      d.classList.toggle("is-active", wanted !== null && d.dataset.match === wanted);
    }
  }
}

export function clearHighlights(): void {
  highlightsByPage.clear();
  setSearchQuery("");
  setActiveMatchValue(null);
  for (const st of stateByCanvasId.values()) {
    if (st.host) {
      st.host.querySelectorAll(".highlight").forEach((n) => n.remove());
    }
  }
}


