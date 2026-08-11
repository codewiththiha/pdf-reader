// Headless verification of the engine-side fixes in public/pdfEngine.js.
//
// This drives the REAL pdf.js + the real engine against the bundled sample PDF
// in Chromium, mirroring the DOM that PageCanvas / ThumbCell build. It checks
// the invariants the fixes establish, not their implementation:
//
//   1. text layer: exactly ONE set of spans survives a superseded render
//      (the duplicate-span overlap behind "double text on selection")
//   2. selection: the text layer paints no glyphs (transparent fill) while
//      selected, so only the canvas glyphs show
//   3. highlights: no duplicates, and each box lands on its span's rect
//   4. thumbnails: a re-render of a cached page is served from the cache
//      (cached:true) and paints synchronously — no skeleton, no flicker
//
// Run:  node scripts/verify/verify.mjs
import { chromium } from "playwright";
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

const MIME = {
  ".html": "text/html", ".mjs": "text/javascript", ".js": "text/javascript",
  ".css": "text/css", ".pdf": "application/pdf", ".bcmap": "application/octet-stream",
  ".wasm": "application/wasm", ".svg": "image/svg+xml", ".map": "application/json",
};

const server = http.createServer((req, res) => {
  let rel = decodeURIComponent(req.url.split("?")[0]);
  if (rel === "/") rel = "/scripts/verify/harness.html";
  // Static assets live under public/ but are served from the root in the app.
  const candidates = [path.join(ROOT, rel), path.join(ROOT, "public", rel)];
  const file = candidates.find((f) => fs.existsSync(f) && fs.statSync(f).isFile());
  if (!file) { res.writeHead(404); res.end("not found: " + rel); return; }
  res.writeHead(200, { "content-type": MIME[path.extname(file)] || "application/octet-stream" });
  fs.createReadStream(file).pipe(res);
});

const results = [];
const check = (ok, label, detail = "") => {
  results.push({ ok, label, detail });
  console.log(`${ok ? "  PASS" : "  FAIL"}  ${label}${detail ? `  — ${detail}` : ""}`);
};

await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
page.on("pageerror", (e) => console.log("  [pageerror]", e.message));

await page.goto(`${base}/`, { waitUntil: "load" });
await page.waitForFunction(() => !!globalThis.PDFReader && !!globalThis.pdfjsLib);

const opened = await page.evaluate(() => PDFReader.open("/samples/sample.pdf"));
check(opened.ok === true, "engine opens the sample PDF",
  opened.ok ? `${opened.numPages} pages` : JSON.stringify(opened.error));

// ---------------------------------------------------------------- 1. text layer
console.log("\n1. text layer — no duplicate spans after superseded renders");

await page.evaluate(() => {
  PDFReader.registerPage({ page: 1, canvasId: "cv1", hostId: "pg1" });
});

// `.pdf-page` holds an absolutely-positioned canvas + text layer, so it has no
// intrinsic size — PageCanvas.rs sizes the host from the render result. Mirror
// that here or the host collapses to 0x0 and nothing is visible/screenshottable.
const sizeHost = () => page.evaluate(() => {
  const st = { w: 0, h: 0 };
  const cv = document.getElementById("cv1");
  const host = document.getElementById("pg1");
  // The engine owns the canvas BACKING STORE only; its CSS box comes from the
  // stylesheet (width/height:100% of this host), so derive the CSS size from
  // the backing store over the device pixel ratio the engine rendered at.
  const out = Math.min(window.devicePixelRatio || 1, 2);
  st.w = parseFloat(cv.style.width) || cv.width / out;
  st.h = parseFloat(cv.style.height) || cv.height / out;
  host.style.width = st.w + "px";
  host.style.height = st.h + "px";
  return st;
});

const single = await page.evaluate(async () => {
  const r = await PDFReader.renderPage("cv1", 1.0, true);
  return {
    ok: r.ok,
    layers: document.querySelectorAll("#pg1 .textLayer").length,
    spans: document.querySelectorAll("#pg1 .textLayer span").length,
  };
});
await sizeHost();
check(single.ok && single.layers === 1, "one .textLayer node after a render",
  `${single.layers} layer(s), ${single.spans} spans`);

// Fire overlapping renders — the exact race that used to leave two sets of
// spans stacked on each other (invisible until you selected them).
const raced = await page.evaluate(async () => {
  const baseline = document.querySelectorAll("#pg1 .textLayer span").length;
  const runs = [
    PDFReader.renderPage("cv1", 1.2, true),
    PDFReader.renderPage("cv1", 1.4, true),
    PDFReader.renderPage("cv1", 1.1, true),
  ];
  await Promise.all(runs);
  await new Promise((r) => setTimeout(r, 400));
  return {
    baseline,
    layers: document.querySelectorAll("#pg1 .textLayer").length,
    spans: document.querySelectorAll("#pg1 .textLayer span").length,
  };
});
check(raced.layers === 1, "still exactly one .textLayer after 3 racing renders",
  `${raced.layers} layer(s)`);
check(raced.spans === raced.baseline,
  "span count unchanged by racing renders (no stacked duplicates)",
  `${raced.spans} vs baseline ${raced.baseline}`);

// Positional duplicate probe: two spans with identical text AND identical
// geometry means the same text is rendered twice on top of itself.
const dupes = await page.evaluate(() => {
  const seen = new Map();
  let dup = 0, example = null;
  for (const s of document.querySelectorAll("#pg1 .textLayer span")) {
    const r = s.getBoundingClientRect();
    const key = `${s.textContent}@${r.x.toFixed(1)},${r.y.toFixed(1)}`;
    if (seen.has(key)) { dup++; example ??= key; }
    seen.set(key, true);
  }
  return { dup, example };
});
check(dupes.dup === 0, "no span is drawn twice at the same position",
  dupes.dup ? `${dupes.dup} duplicate(s), e.g. ${dupes.example}` : "0 duplicates");

// ---------------------------------------------------------------- 2. selection
console.log("\n2. selection — text layer stays transparent (no doubled glyphs)");

const sel = await page.evaluate(() => {
  const span = document.querySelector("#pg1 .textLayer span");
  if (!span) return { err: "no span" };
  const range = document.createRange();
  range.selectNodeContents(span);
  const s = window.getSelection();
  s.removeAllRanges();
  s.addRange(range);
  const cs = getComputedStyle(span);
  // The ::selection pseudo cannot be read via getComputedStyle, so assert the
  // authored rules instead. Collect EVERY matching rule (there are several:
  // the base rule plus the dark-theme background override) and judge them as a
  // set — the invariant is "some rule makes the glyphs transparent, and none
  // makes them opaque".
  const rules = [];
  for (const sheet of document.styleSheets) {
    let cssRules; try { cssRules = sheet.cssRules; } catch { continue; }
    for (const r of cssRules) {
      if (r.selectorText && /\.textLayer\s+::(-moz-)?selection/.test(r.selectorText)
          && !/highlight|endOfContent/.test(r.selectorText)) {
        rules.push({
          selector: r.selectorText,
          color: r.style.color,
          background: r.style.background || r.style.backgroundColor,
          fill: r.style.getPropertyValue("-webkit-text-fill-color"),
        });
      }
    }
  }
  const isTransparent = (c) => !c || /^transparent$/i.test(c) || /rgba\(0,\s*0,\s*0,\s*0\)/.test(c);
  return {
    baseColor: cs.color,
    rules,
    selected: String(s).length > 0,
    anyTransparentColor: rules.some((r) => r.color && isTransparent(r.color)),
    anyOpaqueColor: rules.some((r) => r.color && !isTransparent(r.color)),
    anyTranslucentBg: rules.some((r) => /color-mix|rgba/.test(r.background || "")),
  };
});
check(sel.selected === true, "a span can be selected", `"${sel.baseColor}" base color`);
check(sel.baseColor === "rgba(0, 0, 0, 0)", "unselected text layer is transparent",
  sel.baseColor);
check(sel.anyTransparentColor && !sel.anyOpaqueColor,
  "::selection paints NO glyph color (canvas text shows through)",
  `${sel.rules.length} rule(s); transparent=${sel.anyTransparentColor} opaque=${sel.anyOpaqueColor}`);
check(sel.anyTranslucentBg,
  "::selection background is translucent, not solid",
  sel.rules.map((r) => r.background).filter(Boolean)[0] || "n/a");

await sizeHost();
// Pixel proof of the reported symptom. Selecting must add a TINT over the
// existing glyphs, never a second copy of the text. A second copy shows up as
// new DARK pixels (the text layer's own glyphs, drawn a hair off the canvas
// ones); a tint only lifts pixels toward the accent color. So: count pixels
// that got significantly DARKER after selecting — with the old solid-background
// + painted-glyph approach that number was large; it must now be ~zero.
const beforeShot = await page.locator("#pg1").screenshot();
await page.evaluate(() => {
  const spans = [...document.querySelectorAll("#pg1 .textLayer span")];
  const range = document.createRange();
  range.setStartBefore(spans[0]);
  range.setEndAfter(spans[Math.min(6, spans.length - 1)]);
  const s = window.getSelection();
  s.removeAllRanges();
  s.addRange(range);
});
await page.waitForTimeout(150);
const afterShot = await page.locator("#pg1").screenshot();

const darker = await page.evaluate(async ([a, b]) => {
  const load = (data) => new Promise((res) => {
    const img = new Image();
    img.onload = () => res(img);
    img.src = "data:image/png;base64," + data;
  });
  const [ia, ib] = await Promise.all([load(a), load(b)]);
  const w = Math.min(ia.width, ib.width), h = Math.min(ia.height, ib.height);
  const grab = (img) => {
    const c = document.createElement("canvas");
    c.width = w; c.height = h;
    c.getContext("2d").drawImage(img, 0, 0);
    return c.getContext("2d").getImageData(0, 0, w, h).data;
  };
  const da = grab(ia), db = grab(ib);
  let newDark = 0, changed = 0;
  for (let i = 0; i < da.length; i += 4) {
    const la = (da[i] + da[i + 1] + da[i + 2]) / 3;
    const lb = (db[i] + db[i + 1] + db[i + 2]) / 3;
    if (Math.abs(lb - la) > 8) changed++;
    // A NEW glyph stroke appearing where the page was light before.
    if (la > 170 && lb < 100) newDark++;
  }
  return { newDark, changed, total: (da.length / 4) | 0 };
}, [beforeShot.toString("base64"), afterShot.toString("base64")]);

check(darker.changed > 0, "selecting visibly changes the page (highlight is drawn)",
  `${darker.changed} px changed`);
check(darker.newDark === 0,
  "selecting adds NO new dark glyph pixels (no second copy of the text)",
  `${darker.newDark} new dark px — this is the doubled-text symptom`);

// ---------------------------------------------------------------- 3. highlights
console.log("\n3. search highlights — aligned, de-duplicated");

const hl = await page.evaluate(async () => {
  await PDFReader.buildSearchIndex();
  // Pick a word that actually occurs on page 1.
  const span = [...document.querySelectorAll("#pg1 .textLayer span")]
    .map((s) => s.textContent.trim())
    .find((t) => /^[A-Za-z]{4,}$/.test(t));
  if (!span) return { err: "no searchable word on page 1" };
  const res = await PDFReader.search(span);
  // Re-render so highlights are applied to the current layer.
  await PDFReader.renderPage("cv1", 1.1, true);
  await new Promise((r) => setTimeout(r, 250));

  const host = document.getElementById("pg1").getBoundingClientRect();
  const boxes = [...document.querySelectorAll("#pg1 .highlight")];
  const spans = [...document.querySelectorAll("#pg1 .textLayer span")]
    .filter((s) => s.textContent.toLowerCase().includes(span.toLowerCase()));

  // Every highlight must sit on top of a matching span (within a pixel).
  let aligned = 0, worst = 0;
  for (const b of boxes) {
    const br = b.getBoundingClientRect();
    let best = Infinity;
    for (const s of spans) {
      const sr = s.getBoundingClientRect();
      best = Math.min(best, Math.abs(br.x - sr.x) + Math.abs(br.y - sr.y));
    }
    if (best <= 1.5) aligned++;
    worst = Math.max(worst, best === Infinity ? 999 : best);
  }
  // Duplicate boxes at the same coordinates = doubled highlight.
  const keys = boxes.map((b) => {
    const r = b.getBoundingClientRect();
    return `${r.x.toFixed(1)},${r.y.toFixed(1)}`;
  });
  const dupBoxes = keys.length - new Set(keys).size;
  const cs = boxes[0] ? getComputedStyle(boxes[0]) : null;

  return {
    query: span, total: res.total, boxes: boxes.length, spans: spans.length,
    aligned, worst: +worst.toFixed(2), dupBoxes,
    transform: cs ? cs.transform : null, insideLayer: boxes[0]
      ? !!boxes[0].closest(".textLayer") : null,
  };
});

if (hl.err) {
  check(false, "highlight checks", hl.err);
} else {
  check(hl.boxes > 0, `highlights drawn for "${hl.query}"`,
    `${hl.boxes} box(es) over ${hl.spans} matching span(s)`);
  check(hl.dupBoxes === 0, "no duplicate highlight boxes", `${hl.dupBoxes} duplicate(s)`);
  check(hl.aligned === hl.boxes, "every highlight aligns with its span (<=1.5px)",
    `${hl.aligned}/${hl.boxes} aligned, worst offset ${hl.worst}px`);
  check(hl.transform === "none" || hl.transform === "matrix(1, 0, 0, 1, 0, 0)",
    "highlight boxes are NOT re-transformed by the vendored span rule",
    `transform: ${hl.transform}`);
}

// ---------------------------------------------------------------- 4. thumbnails
console.log("\n4. thumbnails — bitmap cache serves remounts synchronously");

const thumb = await page.evaluate(async () => {
  // Cold: nothing cached yet.
  const coldProbe = PDFReader.hasThumb(1, 0.25);
  const first = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  const warmProbe = PDFReader.hasThumb(1, 0.25);

  // Simulate the virtualization remount: the cell unmounts (canvas wiped and
  // replaced by a fresh node) and comes back.
  const card = document.querySelector(".thumb-card");
  document.getElementById("thumb-1").remove();
  const fresh = document.createElement("canvas");
  fresh.id = "thumb-1";
  fresh.className = "thumb-canvas";
  fresh.style.cssText = "display:block;width:100%";
  card.appendChild(fresh);

  // Was the remounted canvas painted BEFORE the promise resolved? Sample the
  // backing store right after the synchronous portion of the call.
  const p = PDFReader.renderThumb("thumb-1", 1, 0.25);
  const el = document.getElementById("thumb-1");
  const paintedSync = (() => {
    if (!el.width || !el.height) return false;
    const ctx = el.getContext("2d");
    const d = ctx.getImageData(0, 0, el.width, Math.min(el.height, 40)).data;
    // Any non-transparent pixel means the cached frame is already there.
    for (let i = 3; i < d.length; i += 4) if (d[i] !== 0) return true;
    return false;
  })();
  const second = await p;

  // A different scale must MISS the cache (it is keyed on scale).
  const otherScale = PDFReader.hasThumb(1, 0.5);

  return { coldProbe, warmProbe, first, second, paintedSync, otherScale };
});

check(thumb.coldProbe === false, "cache probe is false before the first render");
check(thumb.first.ok && thumb.first.cached === false,
  "first render of a page is a real render (cached:false)");
check(thumb.warmProbe === true, "cache probe is true once the page is rendered");
check(thumb.second.ok && thumb.second.cached === true,
  "a remounted cell is served FROM CACHE (cached:true) — no re-render, no skeleton");
check(thumb.paintedSync === true,
  "the remounted canvas is painted SYNCHRONOUSLY (before the promise resolves)",
  "this is what removes the per-row scroll flicker");
check(thumb.otherScale === false, "cache is keyed on scale (a new scale misses)");

// A cancelled thumbnail must keep its cached bitmap (scroll out and back).
const afterCancel = await page.evaluate(() => {
  PDFReader.cancelThumb("thumb-1");
  return PDFReader.hasThumb(1, 0.25);
});
check(afterCancel === true, "cancelThumb (cell unmount) does NOT evict the cache");

// Opening a new document must drop everything.
const afterDestroy = await page.evaluate(async () => {
  await PDFReader.destroy();
  return PDFReader.hasThumb(1, 0.25);
});
check(afterDestroy === false, "destroy() clears the thumbnail cache (no cross-document bleed)");

await browser.close();
server.close();

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
if (failed.length) {
  console.log("FAILED:");
  for (const f of failed) console.log(`  - ${f.label} ${f.detail}`);
  process.exit(1);
}
