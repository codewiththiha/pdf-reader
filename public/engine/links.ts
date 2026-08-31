// Link-layer construction (annotation -> DOM <a>), split out of renderer.ts.
//
// Two caches sit in front of it, both document-scoped:
//
// * `pageLinks` — a page's annotations resolved to concrete targets, with
//   rects kept in PDF USER SPACE so the entry survives a zoom.
// * `destPages` — a named/explicit destination resolved to a page number.
//   A book with a real table of contents points many annotations at the same
//   handful of destinations, so this turns N round trips into one.
//
// Both exist because this used to run `getAnnotations` plus a serial
// `getDestination`/`getPageIndex` per internal link on EVERY render — zoom
// commit, mode flip, scroll remount — for a target set that never changes
// between them. The DOM layer itself is also reused while it is still mounted
// at the scale it was built for.

import type {
  Annotation,
  PageState,
  PDFPageProxy,
  Viewport,
} from "./types";
import { pdf } from "./state";

type ResolvedLink =
  | { kind: "url"; href: string; rect: [number, number, number, number] }
  | { kind: "page"; page: number; rect: [number, number, number, number] };

/** Page number per resolved destination, memoised for the open document. */
const destPages = new Map<string, number | null>();
/** Resolved links per page number, in annotation order. */
const pageLinks = new Map<number, ResolvedLink[]>();

/** New document (or teardown): nothing here outlives the book. */
export function resetPageLinks(): void {
  destPages.clear();
  pageLinks.clear();
}

function destKey(dest: string | unknown[]): string | null {
  if (typeof dest === "string") return "s:" + dest;
  const ref = dest[0];
  if (typeof ref === "number") return "n:" + ref;
  if (ref && typeof ref === "object") {
    const r = ref as { num?: unknown; gen?: unknown };
    if (typeof r.num === "number") return "r:" + r.num + ":" + (typeof r.gen === "number" ? r.gen : 0);
  }
  // Unkeyable destination: resolve it, just do not remember the answer.
  return null;
}

async function destToPage(dest: string | unknown[] | null | undefined): Promise<number | null> {
  if (!pdf || !dest) return null;
  const key = destKey(dest);
  if (key && destPages.has(key)) return destPages.get(key) ?? null;

  let resolved: number | null = null;
  try {
    const target = typeof dest === "string" ? await pdf.getDestination(dest) : dest;
    if (Array.isArray(target) && target.length) {
      const ref = target[0];
      if (typeof ref === "object" && ref !== null) {
        resolved = (await pdf.getPageIndex(ref)) + 1;
      } else if (Number.isInteger(ref)) {
        resolved = (ref as number) + 1;
      }
    }
  } catch (_) {
    resolved = null;
  }
  if (key) destPages.set(key, resolved);
  return resolved;
}

function safeExternalUrl(raw: string): string | null {
  if (typeof raw !== "string" || !raw) return null;
  let u: URL;
  try {
    u = new URL(raw, globalThis.location ? globalThis.location.href : undefined);
  } catch (_) {
    return null;
  }
  return ["http:", "https:", "mailto:"].includes(u.protocol) ? u.href : null;
}

/** Resolve a page's annotations once. Rects stay in PDF user space; the
 *  caller converts them per viewport. */
async function resolvePageLinks(
  source: PDFPageProxy,
  owned: boolean
): Promise<ResolvedLink[]> {
  let annots: Annotation[] = [];
  try {
    annots = await source.getAnnotations({ intent: "display" });
  } catch (_) {
    annots = [];
  }
  if (owned) {
    try { source.cleanup(); } catch (_) { /* ignore */ }
  }

  const links = annots.filter(
    (a): a is Annotation & { rect: [number, number, number, number] } =>
      !!a && a.subtype === "Link" && Array.isArray(a.rect),
  );

  // Destinations resolve in parallel: they are independent worker round trips
  // and the old serial loop paid one per internal link, in order.
  const targets = await Promise.all(
    links.map(async (a) => {
      const url = safeExternalUrl(a.url ?? "");
      if (url) return { url, page: null as number | null };
      return { url: null, page: await destToPage(a.dest ?? null) };
    }),
  );

  const out: ResolvedLink[] = [];
  for (let i = 0; i < links.length; i += 1) {
    const a = links[i];
    const t = targets[i];
    if (!a || !t) continue;
    if (t.url) out.push({ kind: "url", href: t.url, rect: a.rect });
    else if (t.page) out.push({ kind: "page", page: t.page, rect: a.rect });
  }
  return out;
}

async function linksForPage(
  pageNumber: number,
  page: PDFPageProxy | null
): Promise<ResolvedLink[]> {
  const hit = pageLinks.get(pageNumber);
  if (hit) return hit;

  let source: PDFPageProxy | null = page;
  let owned = false;
  if (!source && pdf) {
    try {
      source = await pdf.getPage(pageNumber);
      owned = true;
    } catch (_) {
      source = null;
    }
  }
  // No document and no page handed in: do not memoise an empty answer, so a
  // later render with a real page still gets a real link layer.
  if (!source) return [];

  const resolved = await resolvePageLinks(source, owned);
  pageLinks.set(pageNumber, resolved);
  return resolved;
}

export async function buildLinkLayer(
  st: PageState,
  viewport: Viewport,
  page: PDFPageProxy | null
): Promise<void> {
  const { host } = st;
  if (!host) return;

  // Still mounted at the scale it was built for: nothing to do. The rect
  // maths below is the only scale-dependent part, and its inputs have not
  // changed since the layer was built.
  if (st.linkLayerEl && st.linkLayerEl.isConnected && st.linkLayerScale === viewport.scale) {
    return;
  }

  const resolved = await linksForPage(st.page, page);

  const layer = document.createElement("div");
  layer.className = "linkLayer";

  for (const link of resolved) {
    const [x1, y1] = viewport.convertToViewportPoint(link.rect[0], link.rect[1]);
    const [x2, y2] = viewport.convertToViewportPoint(link.rect[2], link.rect[3]);
    const x = Math.min(x1, x2);
    const y = Math.min(y1, y2);
    const w = Math.abs(x2 - x1);
    const h = Math.abs(y2 - y1);
    if (!(w > 0) || !(h > 0)) continue;

    const aEl = document.createElement("a");
    aEl.className = "pdf-link";
    aEl.style.left = x + "px";
    aEl.style.top = y + "px";
    aEl.style.width = w + "px";
    aEl.style.height = h + "px";

    if (link.kind === "url") {
      aEl.href = link.href;
      aEl.target = "_blank";
      aEl.rel = "noopener noreferrer";
      aEl.title = link.href;
    } else {
      const p = link.page;
      aEl.href = "#";
      aEl.title = "Go to page " + p;
      aEl.dataset.page = String(p);
      aEl.addEventListener("click", (ev) => {
        ev.preventDefault();
        globalThis.dispatchEvent(
          new CustomEvent("pdfreader:navigate", { detail: { page: p } })
        );
      });
    }
    layer.appendChild(aEl);
  }

  const live = host.querySelector(".linkLayer");
  if (live && live.parentNode) {
    live.replaceWith(layer);
  } else {
    host.appendChild(layer);
  }
  st.linkLayerEl = layer;
  st.linkLayerScale = viewport.scale;
}
