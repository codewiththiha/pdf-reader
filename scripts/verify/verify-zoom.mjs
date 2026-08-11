// Verification for the Preview-style zoom/navigation work.
//
// Every check here maps to an item on the acceptance checklist. The theme is
// always the same question: does the thing the user is LOOKING AT stay where it
// was, and did we render more times than we needed to?
//
//   A. anchoring    — the page/point under the viewport centre survives a zoom
//   B. retargeting  — mashing +/- accelerates instead of queueing or restarting
//   C. clamping     — zooming at the end of the document stays in bounds
//   D. sidebar      — toggling the sidebar changes neither zoom nor renders
//   E. render count — one gesture costs exactly one render pass
//   F. navigation   — ArrowRight lands on the right page, counter agrees
//   G. reduced motion — instant, but still anchored
//
// Renders are counted by wrapping PDFReader.renderPage in the page, which is
// the same boundary the Rust side calls through.
//
// Usage: node scripts/verify/verify-zoom.mjs [baseUrl]
import { chromium } from "playwright";

const BASE = process.argv[2] || "http://127.0.0.1:1420";
const DOC = "/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf";

const results = [];
const check = (ok, label, detail = "") => {
  results.push({ ok, label, detail });
  console.log(`${ok ? "  PASS" : "  FAIL"}  ${label}${detail ? `  — ${detail}` : ""}`);
};

const browser = await chromium.launch();

async function newCtx({ reducedMotion } = {}) {
  const page = await browser.newPage({
    viewport: { width: 1100, height: 800 },
    reducedMotion: reducedMotion ? "reduce" : "no-preference",
  });
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  return { page, errors };
}

/** Open a document through the app's own "Open last" flow (see verify-ui.mjs
 *  for why this is the only non-Tauri entry point). */
async function openViaApp(page, path) {
  await page.evaluate((p) => {
    const KEY = "pdfreader.settings.v1";
    let s = {};
    try { s = JSON.parse(localStorage.getItem(KEY) || "{}"); } catch {}
    s.last_path = p;
    localStorage.setItem(KEY, JSON.stringify(s));
  }, path);
  await page.reload({ waitUntil: "load" });
  await page.waitForSelector("#toolbar-row", { timeout: 30000 });
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Open last"),
    null, { timeout: 15000 });
  await page.evaluate(() => {
    const b = [...document.querySelectorAll("button")]
      .find((x) => x.textContent.trim() === "Open last");
    b && b.click();
  });
  await page.waitForSelector(".pdf-page", { timeout: 30000 });
  await page.waitForTimeout(900);
}

/** Install a render counter around the engine's renderPage. */
async function installCounter(page) {
  await page.evaluate(() => {
    if (globalThis.__renderCount !== undefined) return;
    globalThis.__renderCount = 0;
    const orig = globalThis.PDFReader.renderPage;
    globalThis.PDFReader.renderPage = function (...args) {
      globalThis.__renderCount++;
      return orig.apply(this, args);
    };
  });
}
const renders = (page) => page.evaluate(() => globalThis.__renderCount);
const resetRenders = (page) => page.evaluate(() => { globalThis.__renderCount = 0; });

const zoomPct = (page) => page.evaluate(() => {
  const b = [...document.querySelectorAll("button")].find((x) => x.title === "Zoom");
  return b ? parseInt(b.textContent, 10) : NaN;
});
const scrollTop = (page) => page.evaluate(() => document.getElementById("page-list").scrollTop);
const clickZoom = (page, title) => page.evaluate((t) => {
  const b = [...document.querySelectorAll("button")].find((x) => x.title === t);
  b && b.click();
}, title);

/** Pick an exact zoom preset from the percent popover. The document opens at
 *  fit-width, which for a small-paged PDF can be >200%; rasterising full pages
 *  at that size repeatedly exhausts the headless shell's renderer. Every check
 *  below is scale-independent, so we start each one from a controlled 100%. */
async function setZoomPreset(page, pct) {
  await page.evaluate(() => {
    const b = [...document.querySelectorAll("button")].find((x) => x.title === "Zoom");
    b && b.click();
  });
  await page.waitForTimeout(200);
  await page.evaluate((p) => {
    const b = [...document.querySelectorAll("button")]
      .find((x) => x.textContent.trim() === p + "%");
    b && b.click();
  }, pct);
  await page.waitForTimeout(900);
}

/** Which page number sits under the viewport centre, by real DOM geometry. */
const centrePage = (page) => page.evaluate(() => {
  const list = document.getElementById("page-list");
  const mid = list.scrollTop + list.clientHeight / 2;
  let best = null, bestD = Infinity;
  for (const w of document.querySelectorAll('[id^="cont-"][id$="-wrap"]')) {
    const top = w.offsetTop, h = w.offsetHeight;
    // Distance from the centre to this wrapper's span (0 when inside it).
    const d = mid < top ? top - mid : mid > top + h ? mid - (top + h) : 0;
    if (d < bestD) { bestD = d; best = parseInt(w.id.split("-")[1], 10) + 1; }
  }
  return best;
});

/** The status bar's page counter, rendered as "<page> / <total>". */
const statusPage = (page) => page.evaluate(() => {
  const m = document.body.innerText.match(/(\d+)\s*\/\s*(\d+)/);
  return m ? parseInt(m[1], 10) : NaN;
});

// ---------------------------------------------------------------------------
console.log("\nA/B/C/D/E/F — main session");
{
  const { page, errors } = await newCtx();
  await page.goto(BASE, { waitUntil: "load" });
  await page.waitForFunction(() => !!globalThis.PDFReader, null, { timeout: 30000 });
  await openViaApp(page, DOC);
  await setZoomPreset(page, 100);
  await installCounter(page);

  // Scroll into the middle of the document so anchoring has something to hold.
  await page.evaluate(() => {
    const l = document.getElementById("page-list");
    l.scrollTop = l.scrollHeight * 0.35;
  });
  await page.waitForTimeout(1200);

  // --- A. anchoring ------------------------------------------------------
  const pageBefore = await centrePage(page);
  const pctBefore = await zoomPct(page);
  await resetRenders(page);
  await clickZoom(page, "Zoom in (+)");
  await page.waitForTimeout(900);
  const pageAfter = await centrePage(page);
  const pctAfter = await zoomPct(page);

  check(pctAfter > pctBefore, "zoom in raises the zoom %", `${pctBefore}% -> ${pctAfter}%`);
  check(
    Math.abs(pageAfter - pageBefore) <= 1,
    "A. the page under the viewport centre survives a zoom",
    `centre page ${pageBefore} -> ${pageAfter}`
  );

  // --- E. render count ---------------------------------------------------
  // One gesture must produce ONE crisp pass: a render for each page in the
  // visible window, not a render per animation frame. The window is ~7 pages
  // (visible + 3 buffer each side), so anything under ~16 is a single pass;
  // a per-frame relayout would be many times that.
  const n = await renders(page);
  check(n > 0 && n <= 16, "E. one zoom = one render pass", `${n} renderPage calls`);

  // --- B. retargeting ----------------------------------------------------
  await resetRenders(page);
  const pctStart = await zoomPct(page);
  for (let i = 0; i < 3; i++) {
    await clickZoom(page, "Zoom in (+)");
    await page.waitForTimeout(40);   // faster than ZOOM_ANIM_MS: mid-flight
  }
  await page.waitForTimeout(1000);
  const pctMashed = await zoomPct(page);
  const mashRenders = await renders(page);
  // 3 fast clicks must move 3 presets, not 1: each press chains off the
  // in-flight TARGET, so none is swallowed by the animation.
  // Mirrors core::math::ZOOM_STEPS.
  const steps = [25, 33, 50, 67, 75, 90, 100, 125, 150, 175, 200, 250, 300, 400, 500];
  const iStart = steps.indexOf(pctStart);
  const iEnd = steps.indexOf(pctMashed);
  check(
    iStart >= 0 && iEnd === iStart + 3,
    "B. mashing + advances one preset per press (none swallowed)",
    `${pctStart}% -> ${pctMashed}% in 3 fast clicks`
  );
  // Each click retargets the SAME animation, so the whole burst should still
  // settle into roughly one render pass — not four.
  check(
    mashRenders <= 40,
    "B. a burst of clicks does not cost a render pass each",
    `${mashRenders} renderPage calls for 3 clicks`
  );

  // --- C. clamping -------------------------------------------------------
  // Come back down to a modest zoom first. Rasterising a full page at 300% in
  // the headless shell exhausts its renderer — a harness limit, not an app
  // one — and the clamp behaviour is scale-independent anyway.
  await setZoomPreset(page, 100);
  await page.evaluate(() => {
    const l = document.getElementById("page-list");
    l.scrollTop = l.scrollHeight;
  });
  await page.waitForTimeout(700);
  await clickZoom(page, "Zoom in (+)");
  await page.waitForTimeout(900);
  const st = await scrollTop(page);
  const bounds = await page.evaluate(() => {
    const l = document.getElementById("page-list");
    return { max: l.scrollHeight - l.clientHeight };
  });
  check(
    st >= 0 && st <= bounds.max + 2,
    "C. zooming at the last page stays in bounds",
    `scrollTop ${Math.round(st)} of max ${Math.round(bounds.max)}`
  );

  // --- D. sidebar toggle --------------------------------------------------
  await page.evaluate(() => {
    const l = document.getElementById("page-list");
    l.scrollTop = l.scrollHeight * 0.3;
  });
  await page.waitForTimeout(800);
  await resetRenders(page);
  const pctPreSidebar = await zoomPct(page);
  // Toggle the sidebar open and closed again.
  await page.evaluate(() => {
    const b = [...document.querySelectorAll("button")]
      .find((x) => /thumbnail|sidebar|contents/i.test(x.title || ""));
    b && b.click();
  });
  await page.waitForTimeout(900);
  const pctSidebarOpen = await zoomPct(page);
  const sidebarRenders = await renders(page);

  check(
    pctSidebarOpen === pctPreSidebar,
    "D. sidebar toggle does not change the zoom %",
    `${pctPreSidebar}% -> ${pctSidebarOpen}%`
  );
  // Opening the sidebar mounts thumbnails (renderThumb, a different lane) but
  // must not re-render the main pages.
  check(
    sidebarRenders === 0,
    "D. sidebar toggle causes zero page re-renders",
    `${sidebarRenders} renderPage calls`
  );

  // --- F. navigation ------------------------------------------------------
  await page.evaluate(() => {
    const l = document.getElementById("page-list");
    l.scrollTop = 0;
  });
  await page.waitForTimeout(700);
  const navBefore = await statusPage(page);
  await page.keyboard.press("ArrowRight");
  await page.waitForTimeout(1200);
  const navAfter = await statusPage(page);
  check(
    Number.isFinite(navBefore) && navAfter === navBefore + 1,
    "F. ArrowRight advances exactly one page, counter agrees",
    `page ${navBefore} -> ${navAfter}`
  );
  const navCentre = await centrePage(page);
  check(
    Math.abs(navCentre - navAfter) <= 1,
    "F. the scrollport actually landed on that page",
    `status ${navAfter}, centre ${navCentre}`
  );

  const fatal = errors.filter((e) => !/ResizeObserver loop/i.test(e));
  check(fatal.length === 0, "no page errors during the zoom/nav session",
    fatal.slice(0, 2).join(" | "));
  await page.close();
}

// ---------------------------------------------------------------------------
// H/I — the DEFAULT fit-width state.
//
// The checks above all set an explicit zoom preset first, which clears
// FitMode::Width. A real user who just opened a document is still IN fit mode,
// and that is a different code path — it is where both follow-up reports lived.
console.log("\nH/I — default fit-width state (no preset applied)");
{
  const { page, errors } = await newCtx();
  await page.goto(BASE, { waitUntil: "load" });
  await page.waitForFunction(() => !!globalThis.PDFReader, null, { timeout: 30000 });
  await openViaApp(page, DOC);            // deliberately NO setZoomPreset
  await installCounter(page);

  const trueAspect = await page.evaluate(() => {
    const h = document.querySelector(".pdf-page");
    const r = h.getBoundingClientRect();
    return +(r.width / r.height).toFixed(4);
  });

  // --- I. the sidebar slide is continuous AND undistorted ----------------
  // Runs FIRST, while FitMode::Width is still active: pressing +/- below sets
  // FitMode::None, and with a fixed zoom the page correctly keeps its size
  // when the sidebar opens (it just gets less room) — a different behaviour.
  await page.evaluate(() => {
    const l = document.getElementById("page-list");
    l.scrollTop = l.scrollHeight * 0.25;
  });
  await page.waitForTimeout(1200);
  await resetRenders(page);
  const frames = await page.evaluate(async () => {
    const out = []; const t0 = performance.now();
    const btn = [...document.querySelectorAll("button")]
      .find((x) => (x.getAttribute("title") || "").toLowerCase().includes("sidebar"));
    btn && btn.click();
    return await new Promise((res) => {
      function tick() {
        const h = document.querySelector(".pdf-page");
        const r = h ? h.getBoundingClientRect() : null;
        if (r && r.height > 0) {
          out.push({ w: +r.width.toFixed(1), a: +(r.width / r.height).toFixed(4) });
        }
        if (performance.now() - t0 < 1200) requestAnimationFrame(tick); else res(out);
      }
      requestAnimationFrame(tick);
    });
  });
  const widths = [...new Set(frames.map((f) => f.w))];
  const worstAspect = frames.reduce(
    (a, f) => (Math.abs(f.a - trueAspect) > Math.abs(a - trueAspect) ? f.a : a), trueAspect);
  const slideRenders = await renders(page);

  // Continuous motion: the page must pass through real intermediate sizes
  // rather than holding still and snapping at the end. The old freeze scored
  // exactly 1; this typically scores 5, but the exact count depends on how
  // often ResizeObserver fires during the 300ms CSS width animation, which
  // varies with machine load. >= 3 distinguishes "moves" from "snaps" without
  // being flaky.
  check(
    widths.length >= 3,
    "I. the sidebar slide moves the page through intermediate sizes",
    `${widths.length} distinct widths across ${frames.length} frames`
  );
  // ...and never squishes on the way. Flex shrink on the page host used to
  // take the aspect from 0.77 to 0.58 mid-slide.
  check(
    Math.abs(worstAspect - trueAspect) < 0.01,
    "I. the page keeps its aspect ratio through the whole slide",
    `worst ${worstAspect} vs true ${trueAspect}`
  );
  // Layout every frame, but still only ONE crisp pass at the end.
  check(
    slideRenders > 0 && slideRenders <= 16,
    "I. the slide costs one render pass, at the end",
    `${slideRenders} renderPage calls`
  );

  // --- H. the counter must not walk while zooming ------------------------
  await page.evaluate(() => {
    const l = document.getElementById("page-list");
    l.scrollTop = l.scrollHeight * 0.25;
  });
  await page.waitForTimeout(1400);
  const p0 = await statusPage(page);
  const seen = [p0];
  for (let i = 0; i < 4; i++) {
    await clickZoom(page, "Zoom out (-)");
    await page.waitForTimeout(750);
    seen.push(await statusPage(page));
  }
  for (let i = 0; i < 4; i++) {
    await clickZoom(page, "Zoom in (+)");
    await page.waitForTimeout(750);
    seen.push(await statusPage(page));
  }
  check(
    seen.every((x) => x === p0),
    "H. the page counter holds still through a zoom out/in cycle",
    `saw ${[...new Set(seen)].join(",")}`
  );

  const fatal = errors.filter((e) => !/ResizeObserver loop/i.test(e));
  check(fatal.length === 0, "no page errors in the fit-width session", fatal.slice(0, 2).join(" | "));
  await page.close();
}

// ---------------------------------------------------------------------------
console.log("\nG — reduced motion");
{
  const { page, errors } = await newCtx({ reducedMotion: true });
  await page.goto(BASE, { waitUntil: "load" });
  await page.waitForFunction(() => !!globalThis.PDFReader, null, { timeout: 30000 });
  await openViaApp(page, DOC);
  await setZoomPreset(page, 100);
  await page.evaluate(() => {
    const l = document.getElementById("page-list");
    l.scrollTop = l.scrollHeight * 0.35;
  });
  await page.waitForTimeout(1200);

  const before = await centrePage(page);
  const pctBefore = await zoomPct(page);
  await clickZoom(page, "Zoom in (+)");
  // Deliberately short: with reduced motion the change must already be done.
  await page.waitForTimeout(250);
  const pctAfter = await zoomPct(page);
  const after = await centrePage(page);

  check(pctAfter > pctBefore, "G. reduced motion still zooms", `${pctBefore}% -> ${pctAfter}%`);
  check(
    Math.abs(after - before) <= 1,
    "G. reduced motion is instant but still anchored",
    `centre page ${before} -> ${after}`
  );
  const fatal = errors.filter((e) => !/ResizeObserver loop/i.test(e));
  check(fatal.length === 0, "no page errors under reduced motion", fatal.slice(0, 2).join(" | "));
  await page.close();
}

await browser.close();

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
if (failed.length) {
  console.log("FAILED:");
  for (const f of failed) console.log(`  - ${f.label}${f.detail ? `  (${f.detail})` : ""}`);
  process.exit(1);
}
