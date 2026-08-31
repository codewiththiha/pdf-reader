import { PDFReader, setFakePageTexts } from "./harness.js";

// Search: parallel extraction + matching. The wasm matcher is not present in
// this harness, so the local matcher answers — then a fake matcher exercises
// the delegation plumbing and the throwing-matcher fallback.

function approx(a: number, b: number, label: string): void {
  if (Math.abs(a - b) > 1e-6) throw new Error(label + ": " + a + " vs " + b);
}

export async function run(): Promise<void> {
  setFakePageTexts({
    1: [{ str: "Nothing to see here", transform: [12, 0, 0, 12, 72, 700], width: 150, height: 12 }],
    2: [
      { str: "the theory of the mind", transform: [12, 0, 0, 12, 72, 650], width: 220, height: 12 },
      { str: "THE quick brown fox", transform: [12, 0, 0, 12, 72, 620], width: 180, height: 12 },
    ],
    3: [],
    4: [{ str: "nested the the the occurrences", transform: [12, 0, 0, 12, 100, 600], width: 310, height: 12 }],
  });

  // 1. The local matcher: every occurrence, in document order, with
  // per-page ordinals, interpolated rects, and original-case snippets.
  const res = await PDFReader.search("the");
  if (!res.ok) throw new Error("search failed: " + JSON.stringify(res));
  if (res.total !== 7 || res.matches.length !== 7) {
    throw new Error("expected 7 matches, got " + res.total + " / " + res.matches.length);
  }
  const pages = res.matches.map((m) => m.page);
  if (pages.join(",") !== "2,2,2,2,4,4,4") {
    throw new Error("match order broken: " + pages.join(","));
  }
  for (let i = 0; i < 4; i += 1) {
    if (res.matches[i]!.index !== i) throw new Error("page-2 ordinal " + i + " is " + res.matches[i]!.index);
  }
  for (let i = 0; i < 3; i += 1) {
    if (res.matches[4 + i]!.index !== i) throw new Error("page-4 ordinal " + i + " is " + res.matches[4 + i]!.index);
  }
  const first = res.matches[0]!;
  approx(first.x, 72, "first x");
  approx(first.y, 800 - 650 - 12 * 0.8, "first y");
  approx(first.w, (220 * 3) / 22, "first w");
  approx(first.h, 12, "first h");
  approx(res.matches[1]!.x, 72 + (220 * 4) / 22, "second x");
  approx(res.matches[2]!.x, 72 + (220 * 14) / 22, "third x");
  if (res.matches[3]!.text !== "THE quick brown fox") {
    throw new Error("snippet lost original casing: " + res.matches[3]!.text);
  }
  if (res.matches[0]!.text !== "the theory of the mind") {
    throw new Error("unexpected snippet: " + res.matches[0]!.text);
  }
  console.log("search (local matcher) ok:", res.total, "matches over pages", pages.join(","));

  // 2. An empty or hitless query reports zero without touching the app.
  const empty = await PDFReader.search("");
  if (!empty.ok || empty.total !== 0 || empty.matches.length !== 0) {
    throw new Error("empty query should clear, got " + JSON.stringify(empty));
  }
  const miss = await PDFReader.search("zzz");
  if (!miss.ok || miss.total !== 0) throw new Error("hitless query should be empty");
  console.log("search (empty + miss) ok");

  // 3. WASM matcher delegation: a registered matcher owns the answer, page
  // by page, and receives the extracted positioned text.
  const seen: Array<{
    page: number;
    query: string;
    items: Array<{ s: string; x: number; y: number; w: number; h: number }>;
  }> = [];
  PDFReader.setPageMatcher((payload) => {
    seen.push(payload);
    return payload.items.map((it, i) => ({
      page: payload.page,
      index: i,
      text: "FROM_WASM:" + it.s,
      x: it.x,
      y: it.y,
      w: it.w,
      h: it.h,
    }));
  });
  const delegated = await PDFReader.search("the");
  if (!delegated.ok) throw new Error("delegated search failed");
  if (delegated.total !== 4 || delegated.matches.length !== 4) {
    throw new Error("delegation should answer 1 item per non-empty page, got " + delegated.total);
  }
  if (!delegated.matches[0]!.text.startsWith("FROM_WASM:")) {
    throw new Error("delegated answer not used: " + delegated.matches[0]!.text);
  }
  if (delegated.matches.map((m) => m.page).join(",") !== "1,2,2,4") {
    throw new Error("delegated order broken: " + delegated.matches.map((m) => m.page).join(","));
  }
  const page2 = seen.find((p) => p.page === 2);
  if (!page2 || page2.items.length !== 2 || page2.query !== "the") {
    throw new Error("matcher payload for page 2 is wrong: " + JSON.stringify(page2));
  }
  approx(page2.items[0]!.x, 72, "payload x");
  approx(page2.items[0]!.y, 800 - 650 - 12 * 0.8, "payload y");
  approx(page2.items[0]!.w, 220, "payload w");
  console.log("search (wasm matcher delegation) ok:", delegated.total, "marker matches");

  // 4. A matcher that throws is dropped; the local matcher answers instead.
  PDFReader.setPageMatcher(() => {
    throw new Error("busted matcher");
  });
  const fallback = await PDFReader.search("the");
  if (!fallback.ok) {
    throw new Error("fallback search failed: " + JSON.stringify(fallback));
  }
  if (fallback.total !== 7) {
    throw new Error("throwing matcher should fall back to 7, got " + fallback.total);
  }
  if (fallback.matches[0]!.text.startsWith("FROM_WASM:")) {
    throw new Error("fallback still used the dead matcher");
  }
  PDFReader.setPageMatcher(null);
  console.log("search (throwing matcher fallback) ok");
}
