// On-demand streaming search. Pages are extracted as we scan and then
// discarded — we do not keep a permanent Map of every TextItem in the heap.
// Extraction runs a bounded window of pages at a time (one pdf.js worker
// round trip per page used to be the whole-document latency); the matching
// itself runs in the wasm module the app registers, with the local matcher
// kept as the engine-standalone fallback.

import type {
  SearchMatch,
  SearchRect,
  SearchResult,
  TextItem,
  WasmPageMatcher,
} from "./types";
import { fail } from "./canvas";
import { runLimited } from "./concurrency";
import { refreshHighlights } from "./highlights";
import {
  highlightsByPage,
  numPages,
  pdf,
  setActiveMatchValue,
  setSearchQuery,
  stateByCanvasId,
  textIndex,
} from "./state";

/** Pages extracted (and held) at once — also the memory ceiling: one window
 *  of page texts lives at a time, preserving the streaming profile the
 *  serial scan had. */
const EXTRACT_WINDOW = 4;

// The compiled page matcher, registered by the wasm app (pdf_engine's
// wasm_ops) at boot. Absent in the engine-standalone configuration (the
// smoke harness); a matcher that returns junk or throws is dropped and the
// local matcher answers for that page.
let wasmMatchPage: WasmPageMatcher | null = null;

/** Register (or, with null, remove) the wasm-compiled page matcher. */
export function setPageMatcher(fn: WasmPageMatcher | null): void {
  wasmMatchPage = typeof fn === "function" ? fn : null;
}

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

function snippetText(str: string, q: string, from: number | undefined): string {
  const idx = from === undefined ? str.toLowerCase().indexOf(q) : from;
  const start = Math.max(0, idx - 25);
  const end = Math.min(str.length, idx + q.length + 30);
  const pre = start > 0 ? "…" : "";
  const post = end < str.length ? "…" : "";
  return pre + str.slice(start, end) + post;
}

/** No-op: search() streams pages itself. Same `{ok, count}` envelope as the rest. */
export async function buildSearchIndex(): Promise<{ ok: true; count: number }> {
  textIndex.clear();
  return { ok: true, count: 0 };
}

/** One positioned text item: the string plus its scale-1 rect. The pdf.js
 *  transform → rect derivation stays here with the item shape; everything
 *  after this is the matcher's job. */
type GeoItem = { s: string; x: number; y: number; w: number; h: number };

/** Read one page's text through the pdf.js worker. `null` = unreadable page
 *  (skipped, exactly like the old serial loop's per-page catch). */
async function extractPage(page: number): Promise<{ page: number; items: GeoItem[] } | null> {
  if (!pdf) return null;
  try {
    const pg = await pdf.getPage(page);
    const tc = await pg.getTextContent();
    const pageH = pg.getViewport({ scale: 1 }).height;
    const items: GeoItem[] = [];
    for (const item of tc.items) {
      if (!item.str) continue;
      const r = itemRect(item, pageH);
      if (r.w <= 0) continue;
      items.push({ s: item.str, x: r.x, y: r.y, w: r.w, h: r.h });
    }
    try { pg.cleanup(); } catch (_) { /* advisory */ }
    return { page, items };
  } catch (_) {
    return null;
  }
}

/** The local matcher — the loop the engine ran before the matching moved
 *  into the wasm module, kept verbatim as the standalone/fallback path. */
function matchPageJs(items: GeoItem[], q: string, page: number): SearchMatch[] {
  const out: SearchMatch[] = [];
  const qlen = q.length;
  let ord = 0;
  for (const item of items) {
    const lower = item.s.toLowerCase();
    const len = lower.length || 1;
    for (
      let at = lower.indexOf(q);
      at !== -1;
      at = lower.indexOf(q, at + qlen)
    ) {
      out.push({
        page,
        index: ord,
        text: snippetText(item.s, q, at),
        x: item.x + (item.w * at) / len,
        y: item.y,
        w: Math.max(1, (item.w * qlen) / len),
        h: item.h,
      });
      ord += 1;
    }
  }
  return out;
}

function matchPage(items: GeoItem[], q: string, page: number): SearchMatch[] {
  if (wasmMatchPage) {
    try {
      const res = wasmMatchPage({ page, query: q, items });
      if (Array.isArray(res)) return res as SearchMatch[];
    } catch (_) {
      // A throwing matcher is a dead matcher: drop it and match locally.
      wasmMatchPage = null;
    }
  }
  return matchPageJs(items, q, page);
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
  const matches: SearchMatch[] = [];

  // Chunked extraction: a bounded window of pages is in flight at a time and
  // each chunk is consumed in page order, so the match list stays in
  // document order and the heap holds at most one window of page texts.
  for (let first = 1; first <= numPages; first += EXTRACT_WINDOW) {
    const chunk: number[] = [];
    for (let p = first; p < first + EXTRACT_WINDOW && p <= numPages; p += 1) {
      chunk.push(p);
    }
    const extracted = await runLimited(
      chunk.map((p) => () => extractPage(p)),
      chunk.length,
    );
    for (const page of extracted) {
      if (!page) continue;
      const pageMatches = matchPage(page.items, q, page.page);
      matches.push(...pageMatches);
      if (pageMatches.length) {
        highlightsByPage.set(
          page.page,
          pageMatches.map(({ x, y, w, h }) => ({ x, y, w, h })),
        );
      }
    }
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
