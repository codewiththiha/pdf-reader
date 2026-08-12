// End-to-end UI verification against the running Leptos app (trunk serve).
//
// Drives the real app in Chromium and checks the user-visible behaviour of the
// UI-side fixes:
//
//   A. filename  — a PDF whose /Title is producer junk shows its FILE NAME;
//                  a PDF with a real title still shows the title
//   B. folding   — the name is only truncated when it would actually collide,
//                  adapts to window width, and hides itself when hopeless
//   C. thumbs    — scrolling the thumbnail grid up and down does not re-run the
//                  loading skeleton on rows that were already rendered
//   D. selection — selecting text in the real viewer adds no second copy
//
// Usage: node scripts/verify/verify-ui.mjs [baseUrl]
import { chromium } from "playwright";

const BASE = process.argv[2] || "http://127.0.0.1:1420";
const JUNK = "/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf";
const GOOD = "/samples/Good Title Book.pdf";

const results = [];
const check = (ok, label, detail = "") => {
  results.push({ ok, label, detail });
  console.log(`${ok ? "  PASS" : "  FAIL"}  ${label}${detail ? `  — ${detail}` : ""}`);
};

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });

await page.goto(BASE, { waitUntil: "load" });
await page.waitForFunction(() => !!globalThis.PDFReader, null, { timeout: 30000 });
// Wait for the wasm app to mount its toolbar.
await page.waitForSelector("#toolbar-row", { timeout: 30000 });
check(true, "app mounts (toolbar present)");

/** Open a document through the app's OWN open-flow, with no test hooks in the
 *  production build.
 *
 *  The real entry points are a native dialog (Tauri-only) and drag-drop
 *  (Tauri-only), neither of which is reachable from a browser. The third is the
 *  placeholder's "Open last" button, which calls exactly the same
 *  `toolbar::open_path` flow — so seeding `last_path` in the persisted settings
 *  and clicking that button exercises the genuine code path, including the Rust
 *  state writes (doc.title / doc.path) the filename logic depends on. */
async function openViaApp(path) {
  await page.evaluate((p) => {
    const KEY = "pdfreader.settings.v1";
    let s = {};
    try { s = JSON.parse(localStorage.getItem(KEY) || "{}"); } catch {}
    s.last_path = p;
    localStorage.setItem(KEY, JSON.stringify(s));
  }, path);
  await page.reload({ waitUntil: "load" });
  await page.waitForSelector("#toolbar-row", { timeout: 30000 });
  // The placeholder renders "Open last" only while Idle with a persisted path.
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Open last"),
    null, { timeout: 15000 });
  await page.evaluate(() => {
    const b = [...document.querySelectorAll("button")]
      .find((x) => x.textContent.trim() === "Open last");
    b && b.click();
  });
  // Wait for the document to actually be ready (a page host appears).
  await page.waitForSelector(".pdf-page", { timeout: 30000 });
  await page.waitForTimeout(600);
}

// ---------------------------------------------------------------- A. filename
console.log("\nA. document name");

async function openAndRead(path) {
  await openViaApp(path);
  await page.waitForTimeout(1200);
  return page.evaluate(() => {
    const el = document.getElementById("toolbar-doc-title");
    if (!el) return null;
    return {
      text: el.textContent.trim(),
      title: el.getAttribute("title"),
      maxWidth: getComputedStyle(el).maxWidth,
      width: el.getBoundingClientRect().width,
      scrollWidth: el.scrollWidth,
      clientWidth: el.clientWidth,
      hidden: getComputedStyle(el).display === "none",
    };
  });
}

const junk = await openAndRead(JUNK);
if (junk) {
  check(
    junk.text === "Programming Pearls (2nd Edition) - Jon Bentley",
    "junk /Title falls back to the real file name",
    `shows "${junk.text}"`
  );
  check(!/file:|%20|F\|/.test(junk.text), "no path/URL artifacts in the displayed name", junk.text);
} else {
  check(false, "document name element found", "#toolbar-doc-title missing");
}

const good = await openAndRead(GOOD);
if (good) {
  check(good.text === "The Art of Computer Programming",
    "a trustworthy /Title is still preferred over the file name",
    `shows "${good.text}"`);
}

// ---------------------------------------------------------------- B. folding
console.log("\nB. adaptive folding");

await openViaApp(JUNK);
await page.waitForTimeout(1000);

async function nameMetrics() {
  return page.evaluate(() => {
    const el = document.getElementById("toolbar-doc-title");
    if (!el) return null;
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    const nav = document.getElementById("toolbar-center");
    const right = document.getElementById("toolbar-right");
    return {
      text: el.textContent.trim(),
      truncated: el.scrollWidth > el.clientWidth + 1,
      hidden: cs.display === "none",
      maxWidth: cs.maxWidth,
      right: r.right,
      navLeft: nav ? nav.getBoundingClientRect().left : null,
      rightLeft: right ? right.getBoundingClientRect().left : null,
    };
  });
}

// Wide window, continuous mode (no centered nav): a 46-char name fits easily.
await page.setViewportSize({ width: 1500, height: 900 });
await page.waitForTimeout(700);
const wide = await nameMetrics();
if (wide) {
  check(!wide.truncated,
    "wide window: full name shown, NOT folded (the old hard max-w-40 bug)",
    `"${wide.text}" maxWidth=${wide.maxWidth}`);
  check(wide.maxWidth !== "160px", "no fixed 160px cap", `maxWidth=${wide.maxWidth}`);
}

// Never overlap the right-hand controls.
if (wide && wide.rightLeft !== null) {
  check(wide.right <= wide.rightLeft + 1,
    "name never overlaps the right-hand control group",
    `name right=${wide.right.toFixed(0)} < controls left=${wide.rightLeft.toFixed(0)}`);
}

// Switch to single-page mode: the centered page nav appears and becomes the
// binding constraint. The name must yield to it, not run underneath.
await page.evaluate(() => {
  const seg = document.querySelector('#toolbar-right button[title="Single page view"]');
  if (!seg) throw new Error("single-page segment not found (missing title?)");
  seg.click();
});
await page.waitForTimeout(900);
const single = await nameMetrics();
if (single && single.navLeft !== null) {
  check(single.right <= single.navLeft + 1,
    "single mode: name stops before the centered page nav",
    `name right=${single.right.toFixed(0)} <= nav left=${single.navLeft.toFixed(0)}`);
}

// Narrow the window progressively: the name must fold, then hide.
const shrink = [];
for (const w of [1100, 900, 760, 640, 520, 430]) {
  await page.setViewportSize({ width: w, height: 900 });
  await page.waitForTimeout(500);
  const m = await nameMetrics();
  shrink.push({ w, ...m });
}
const foldedSomewhere = shrink.some((s) => s.truncated || s.hidden);
check(foldedSomewhere, "narrow windows do fold / hide the name (width awareness)",
  shrink.map((s) => `${s.w}:${s.hidden ? "hidden" : s.truncated ? "folded" : "full"}`).join(" "));
const noOverlap = shrink.every((s) => s.hidden || s.navLeft === null || s.right <= s.navLeft + 1);
check(noOverlap, "at every width the name still clears the centered nav",
  shrink.map((s) => `${s.w}:${s.hidden ? "hidden" : Math.round(s.navLeft - s.right) + "px"}`).join(" "));
const shrinksMonotonically = shrink.every((s, i) =>
  i === 0 || s.hidden || shrink[i - 1].hidden ||
  parseFloat(s.maxWidth) <= parseFloat(shrink[i - 1].maxWidth) + 1);
check(shrinksMonotonically, "the budget shrinks as the window shrinks (no oscillation)",
  shrink.map((s) => `${s.w}:${s.maxWidth}`).join(" "));

await page.setViewportSize({ width: 1400, height: 900 });
await page.waitForTimeout(600);

// ---------------------------------------------------------------- C. thumbnails
console.log("\nC. thumbnail scrolling (flicker)");

await openViaApp(JUNK);
await page.waitForTimeout(1200);
// Open the thumbnails sidebar.
await page.evaluate(() => {
  const b = [...document.querySelectorAll("button")]
    .find((x) => x.getAttribute("title") === "Toggle sidebar");
  b && b.click();
});
await page.waitForTimeout(1200);

const grid = await page.$("#thumb-scroll");
if (!grid) {
  check(false, "thumbnail grid present", "#thumb-scroll not found");
} else {
  // Scroll down through several rows so they render and get cached...
  await page.evaluate(() => {
    const el = document.getElementById("thumb-scroll");
    el.scrollTop = 0;
  });
  await page.waitForTimeout(900);
  for (const top of [300, 600, 900, 1200]) {
    await page.evaluate((t) => { document.getElementById("thumb-scroll").scrollTop = t; }, top);
    await page.waitForTimeout(700);
  }

  // ...then scroll back UP, which is exactly the reported case. Sample the
  // skeleton covers of the mounted cells on every animation frame during the
  // scroll: a cached row must mount with its cover already transparent and
  // WITHOUT the pulse class. Any opaque cover over an already-rendered page is
  // the flicker.
  const flicker = await page.evaluate(async () => {
    const el = document.getElementById("thumb-scroll");
    const samples = [];
    let running = true;
    const sample = () => {
      if (!running) return;
      for (const cover of document.querySelectorAll(".thumb-skeleton")) {
        const cs = getComputedStyle(cover);
        samples.push({
          opacity: parseFloat(cs.opacity),
          pulsing: cover.classList.contains("thumb-skeleton-loading"),
          animated: cs.animationName !== "none",
        });
      }
      requestAnimationFrame(sample);
    };
    requestAnimationFrame(sample);

    // Smooth-ish scroll back up in steps.
    for (let t = 1200; t >= 0; t -= 100) {
      el.scrollTop = t;
      await new Promise((r) => setTimeout(r, 60));
    }
    await new Promise((r) => setTimeout(r, 300));
    running = false;

    const opaque = samples.filter((s) => s.opacity > 0.15);
    const pulsing = samples.filter((s) => s.pulsing);
    return {
      total: samples.length,
      opaque: opaque.length,
      pulsing: pulsing.length,
      maxOpacity: samples.reduce((m, s) => Math.max(m, s.opacity), 0),
    };
  });

  check(flicker.total > 0, "thumbnail covers sampled during the scroll-up",
    `${flicker.total} samples`);
  check(flicker.opaque === 0,
    "scrolling UP shows NO loading cover over already-rendered rows (the flicker)",
    `${flicker.opaque}/${flicker.total} opaque samples, peak opacity ${flicker.maxOpacity.toFixed(2)}`);
  check(flicker.pulsing === 0,
    "no skeleton pulse animation restarts on re-entering rows",
    `${flicker.pulsing} pulsing samples`);

  // Every visible thumbnail canvas should actually have pixels in it.
  const painted = await page.evaluate(() => {
    const out = { checked: 0, blank: 0 };
    for (const cv of document.querySelectorAll(".thumb-canvas")) {
      if (!cv.width || !cv.height) { out.blank++; continue; }
      out.checked++;
      const d = cv.getContext("2d").getImageData(0, 0, cv.width, Math.min(20, cv.height)).data;
      let any = false;
      for (let i = 3; i < d.length; i += 4) if (d[i] !== 0) { any = true; break; }
      if (!any) out.blank++;
    }
    return out;
  });
  check(painted.blank === 0, "every mounted thumbnail canvas is painted",
    `${painted.checked} checked, ${painted.blank} blank`);
}

// ---------------------------------------------------------------- D. selection
console.log("\nD. selection in the real viewer");

// Close the sidebar to get the page area back.
await page.evaluate(() => {
  const b = [...document.querySelectorAll("button")]
    .find((x) => x.getAttribute("title") === "Toggle sidebar");
  b && b.click();
});
await page.waitForTimeout(1000);

const host = await page.$(".pdf-page");
if (!host) {
  check(false, "a rendered page is present", ".pdf-page not found");
} else {
  const before = await host.screenshot();
  const selected = await page.evaluate(() => {
    const spans = [...document.querySelectorAll(".pdf-page .textLayer span")]
      .filter((s) => s.textContent.trim().length > 2);
    if (spans.length < 2) return 0;
    const r = document.createRange();
    r.setStartBefore(spans[0]);
    r.setEndAfter(spans[Math.min(3, spans.length - 1)]);
    const s = window.getSelection();
    s.removeAllRanges();
    s.addRange(r);
    return String(s).length;
  });
  await page.waitForTimeout(250);
  const after = await host.screenshot();

  check(selected > 0, "text can be selected in the viewer", `${selected} chars`);

  const diff = await page.evaluate(async ([a, b]) => {
    const load = (d) => new Promise((res) => {
      const i = new Image(); i.onload = () => res(i); i.src = "data:image/png;base64," + d;
    });
    const [ia, ib] = await Promise.all([load(a), load(b)]);
    const w = Math.min(ia.width, ib.width), h = Math.min(ia.height, ib.height);
    const px = (img) => {
      const c = document.createElement("canvas"); c.width = w; c.height = h;
      c.getContext("2d").drawImage(img, 0, 0);
      return c.getContext("2d").getImageData(0, 0, w, h).data;
    };
    const da = px(ia), db = px(ib);
    let changed = 0, newDark = 0;
    for (let i = 0; i < da.length; i += 4) {
      const la = (da[i] + da[i + 1] + da[i + 2]) / 3;
      const lb = (db[i] + db[i + 1] + db[i + 2]) / 3;
      if (Math.abs(lb - la) > 8) changed++;
      if (la > 170 && lb < 100) newDark++;
    }
    return { changed, newDark };
  }, [before.toString("base64"), after.toString("base64")]);

  check(diff.changed > 0, "selection is visible", `${diff.changed} px changed`);
  check(diff.newDark === 0,
    "selection draws NO second copy of the text (no doubling / misalignment)",
    `${diff.newDark} new dark px`);
}

check(errors.length === 0, "no console/page errors during the run",
  errors.slice(0, 3).join(" | ") || "clean");

// ---------------------------------------------------------------------------
// Outline panel — deep entries must render TEXT, not a bare ellipsis.
//
// The indent was an unbounded `8 + depth * 14` against a fixed 288px (`w-72`)
// sidebar, so from ~depth 12 the padding consumed the whole row and `truncate`
// collapsed the title to "...". Real TOCs nest 5-10 levels, so deep entries
// showed as dots. `Outlined Book.pdf` carries a 6-level branch for this.
{
  await openViaApp("/samples/Outlined Book.pdf");
  await page.evaluate(() => {
    const b = [...document.querySelectorAll("button")]
      .find((x) => /outline/i.test(x.title || "") || x.textContent.trim() === "Outline");
    if (b) b.click();
  });
  await page.waitForTimeout(600);

  const rows = await page.evaluate(() => {
    const out = [];
    for (const b of document.querySelectorAll("aside button")) {
      if (!b.className.includes("truncate")) continue;
      const cs = getComputedStyle(b);
      const padL = parseFloat(cs.paddingLeft) || 0;
      const padR = parseFloat(cs.paddingRight) || 0;
      out.push({
        text: b.textContent.trim(),
        h: +b.getBoundingClientRect().height.toFixed(1),
        // Space actually available to paint the label.
        textW: Math.round(b.getBoundingClientRect().width - padL - padR),
        title: b.getAttribute("title") || "",
      });
    }
    return out;
  });

  check(rows.length >= 6, "outline renders its entries", `${rows.length} rows`);
  // A title that is whitespace-only, newline-only or made of zero-width
  // characters used to render an 8px row instead of 28px — a sliver that reads
  // as a dot. The engine normalises those to "(untitled)" and `min-h-7` is the
  // backstop. The fixture carries all four pathological cases.
  const shortRows = rows.filter((r) => r.h < 24);
  check(shortRows.length === 0, "no outline row collapses to a sliver",
    shortRows.length ? `${shortRows.length} rows under 24px (min ${Math.min(...shortRows.map((s) => s.h))}px)` : "all rows >= 24px");
  const blank = rows.filter((r) => r.text.length === 0);
  check(blank.length === 0, "no outline row renders as blank text",
    blank.length ? `${blank.length} blank rows` : "every row has a visible label");
  // Every row must have real room for text, not just an ellipsis.
  const starved = rows.filter((r) => r.textW < 100);
  check(starved.length === 0, "no outline row is squeezed to the ellipsis",
    starved.length ? `${starved.length} rows under 100px (worst ${Math.min(...starved.map((s) => s.textW))}px)` : "all rows >= 100px");
  // And the deepest entry must still carry its real text.
  const deep = rows.find((r) => r.text.startsWith("7.1.1.1.1.1"));
  check(!!deep, "the 6-level-deep entry renders its title as text",
    deep ? `"${deep.text}" with ${deep.textW}px` : "not found");
  // Truncated titles stay recoverable.
  check(rows.every((r) => r.title.length > 0), "every outline row exposes its full title as a tooltip");
}

// ---------------------------------------------------------------------------
// Thumbnails — a freshly rendered cell must reveal at a CONSTANT tint.
//
// The skeleton pulse swings ~54 luminance units over a 1.6s cycle and used to
// keep running through the 300ms fade-out. A render can resolve at any phase,
// so each new cell started its reveal from a different brightness and kept
// oscillating while fading; row-mates resolve a few ms apart and shimmered
// against each other. `.thumb-skeleton-settling` pauses the animation at
// resolve (pausing, not removing, so nothing snaps) and the existing timer
// drops both classes once the cover is invisible.
{
  await openViaApp(JUNK);
  await page.evaluate(() => {
    const b = [...document.querySelectorAll("button")]
      .find((x) => /thumb/i.test(x.title || "") || x.textContent.trim() === "Thumbs");
    if (b) b.click();
  });
  await page.waitForTimeout(1200);

  const r = await page.evaluate(async () => {
    const sc = [...document.querySelectorAll("aside div")]
      .filter((d) => d.scrollHeight > d.clientHeight + 50)[0];
    if (!sc) return null;
    // Resolve any colour (oklab/color(srgb ...)) to real rgb via a canvas.
    const c1 = document.createElement("canvas");
    c1.width = c1.height = 1;
    const ctx = c1.getContext("2d", { willReadFrequently: true });
    const lum = (css) => {
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = css;
      ctx.fillRect(0, 0, 1, 1);
      const q = ctx.getImageData(0, 0, 1, 1).data;
      return 0.2126 * q[0] + 0.7152 * q[1] + 0.0722 * q[2];
    };
    const track = new Map();
    let stop = false;
    const obs = () => {
      for (const card of sc.querySelectorAll(".thumb-card")) {
        const cv = card.querySelector("canvas");
        const cover = card.querySelector(".thumb-skeleton");
        if (!cv || !cover) continue;
        const cs = getComputedStyle(cover);
        const op = +cs.opacity;
        if (op >= 0.999 || op <= 0.001) continue;   // sample only mid-reveal
        if (!track.has(cv.id)) track.set(cv.id, []);
        track.get(cv.id).push(lum(cs.backgroundColor));
      }
      if (!stop) requestAnimationFrame(obs);
    };
    requestAnimationFrame(obs);
    sc.scrollTop = sc.scrollHeight * 0.5;
    await new Promise((r) => setTimeout(r, 2000));
    sc.scrollTop = sc.scrollHeight * 0.85;
    await new Promise((r) => setTimeout(r, 2000));
    stop = true;

    let worst = 0, cells = 0;
    for (const [, Ls] of track) {
      if (Ls.length < 3) continue;
      cells++;
      worst = Math.max(worst, Math.max(...Ls) - Math.min(...Ls));
    }
    // Everything must settle: no cover left visible, no class left behind.
    await new Promise((r) => setTimeout(r, 900));
    let stuck = 0;
    for (const card of sc.querySelectorAll(".thumb-card")) {
      const cover = card.querySelector(".thumb-skeleton");
      if (!cover) continue;
      if (+getComputedStyle(cover).opacity > 0.01
        || cover.className.includes("thumb-skeleton-settling")) stuck++;
    }
    return { worst: +worst.toFixed(1), cells, stuck };
  });

  check(r !== null && r.cells >= 3, "observed several thumbnails revealing",
    r ? `${r.cells} cells` : "no scroller");
  // 1 unit of slack for sub-pixel colour rounding; it was ~54 before the fix.
  check(r !== null && r.worst <= 1,
    "a thumbnail's tint stays constant through its reveal (no pulse shimmer)",
    r ? `worst swing ${r.worst}` : "n/a");
  check(r !== null && r.stuck === 0,
    "no thumbnail is left covered or stuck paused",
    r ? `${r.stuck} stuck` : "n/a");
}

// ---------------------------------------------------------------------------
// Sidebar navigation: jumping from a panel must not close it, and the outline
// must show WHERE the reader is.
//
// Both panels used to call `sidebar.set(SidebarMode::None)` on click, so every
// jump slammed the panel shut — which makes browsing (jump, look, jump again)
// impossible and leaves the outline highlight with nowhere to show. The
// highlight itself is the "no clue which section I'm in" fix: the active entry
// is the last one at or before the current page, and it must track the reader
// as they scroll, not only when they click.
// ---------------------------------------------------------------------------
{
  await openViaApp("/samples/Outlined Book.pdf");
  await page.waitForTimeout(1500);

  const sidebarOpen = () =>
    page.evaluate(() => {
      const a = document.querySelector("aside");
      return a ? a.getBoundingClientRect().width > 50 : false;
    });
  const statusPage = () =>
    page.evaluate(() => {
      const m = document.body.innerText.match(/(\d+)\s*\/\s*(\d+)/);
      return m ? +m[1] : NaN;
    });
  const clickTitle = (t) =>
    page.evaluate((t) => {
      const b = [...document.querySelectorAll("button")].find((x) => x.title === t);
      b && b.click();
    }, t);
  // The rail buttons are TOGGLES: clicking the already-open panel's tab closes
  // it, so only click when it is not already showing.
  const showPanel = async (name) => {
    const showing = await page.evaluate((n) => {
      const rows = [...document.querySelectorAll("aside button")];
      const b = rows.find((x) => x.textContent.trim() === n);
      return b ? /bg-line|text-ink/.test(b.className) : null;
    }, name);
    if (showing === true) return;
    await page.evaluate((n) => {
      const b = [...document.querySelectorAll("aside button")].find(
        (x) => x.textContent.trim() === n
      );
      b && b.click();
    }, name);
  };

  if (!(await sidebarOpen())) {
    await clickTitle("Toggle sidebar");
    await page.waitForTimeout(800);
  }

  // --- thumbnails ---------------------------------------------------------
  await showPanel("Thumbs");
  await page.waitForTimeout(1500);
  const thumbsBefore = await statusPage();
  const clickedThumb = await page.evaluate(() => {
    const cells = [...document.querySelectorAll("aside button")].filter((b) =>
      b.querySelector("canvas")
    );
    if (cells.length < 4) return false;
    cells[3].click();
    return true;
  });
  await page.waitForTimeout(1400);
  check(clickedThumb, "found thumbnails to click");
  check(await sidebarOpen(), "clicking a thumbnail keeps the sidebar open");
  check(
    (await statusPage()) !== thumbsBefore,
    "clicking a thumbnail still navigates",
    `page ${thumbsBefore} -> ${await statusPage()}`
  );

  // --- outline ------------------------------------------------------------
  await showPanel("Outline");
  await page.waitForTimeout(1000);
  const rows = () =>
    page.evaluate(() =>
      [...document.querySelectorAll("aside button[aria-current]")].map((b) => ({
        title: b.textContent.trim().slice(0, 30),
        active: b.getAttribute("aria-current") === "true",
      }))
    );

  const r0 = await rows();
  check(r0.length > 0, "outline rows render", `${r0.length} rows`);
  check(
    r0.filter((r) => r.active).length === 1,
    "exactly one outline entry is highlighted",
    `${r0.filter((r) => r.active).length} active`
  );

  await page.evaluate(() => {
    const bs = [...document.querySelectorAll("aside button[aria-current]")];
    bs[2] && bs[2].click();
  });
  await page.waitForTimeout(1400);
  check(await sidebarOpen(), "clicking an outline entry keeps the sidebar open");
  const r1 = await rows();
  check(
    r1.findIndex((r) => r.active) === 2,
    "the clicked outline entry becomes the highlighted one",
    `active index ${r1.findIndex((r) => r.active)}`
  );

  // The highlight must be VISIBLE, not just an aria attribute.
  const style = await page.evaluate(() => {
    const on = document.querySelector("aside button[aria-current='true']");
    const off = document.querySelector("aside button[aria-current='false']");
    if (!on || !off) return null;
    const a = getComputedStyle(on);
    const b = getComputedStyle(off);
    return {
      accent: a.borderLeftColor !== b.borderLeftColor,
      ground: a.backgroundColor !== b.backgroundColor || a.fontWeight !== b.fontWeight,
    };
  });
  check(style !== null && style.accent, "the active outline row has a distinct left accent");
  check(style !== null && style.ground, "the active outline row is distinct in ground/weight");

  // ...and it follows the reader, not just clicks.
  const activeBefore = (await rows()).findIndex((r) => r.active);
  await page.evaluate(() => {
    const l = document.getElementById("page-list");
    l.scrollTop = l.scrollHeight * 0.75;
  });
  await page.waitForTimeout(1600);
  const activeAfter = (await rows()).findIndex((r) => r.active);
  check(
    activeAfter >= 0 && activeAfter !== activeBefore,
    "the outline highlight follows the reader while scrolling",
    `index ${activeBefore} -> ${activeAfter}`
  );
}

await browser.close();

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
if (failed.length) {
  console.log("FAILED:");
  for (const f of failed) console.log(`  - ${f.label} ${f.detail}`);
  process.exit(1);
}
