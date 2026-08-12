// Regression gate for the appearance system + clickable links.
//
//   A. presets   — gallery renders grouped, swatches carry their OWN look,
//                  selecting one applies it, Sepia/Green/Night still exist
//   B. tint      — hue + strength drive a computed canvas filter and the UI
//                  tokens, and 0 strength means literally no filter
//   C. texture   — opacity and scale sliders reach the page, and the texture
//                  still tracks zoom (appendix 7 must not regress)
//   D. grain     — off / static / animated, and animated actually moves
//   E. saving    — a custom preset persists, groups, and reloads
//   F. links     — internal links jump, external open in a new tab,
//                  javascript: URLs are inert
//   G. migration — a pre-refactor settings blob lands on the right preset
//
// Run: node scripts/verify/verify-appearance.mjs   (dev server on :1420)

import { chromium } from "playwright";

const BASE = "http://127.0.0.1:1420";
const DOC = "/samples/Outlined Book.pdf";
const LINKED = "/samples/Linked Book.pdf";

const results = [];
const check = (ok, label, detail = "") => {
  results.push({ ok, label, detail });
  console.log(`  ${ok ? "PASS" : "FAIL"}  ${label}${detail ? `  — ${detail}` : ""}`);
};

const browser = await chromium.launch();

async function newPage(ctx) {
  const page = await ctx.newPage();
  page.on("pageerror", (e) => {
    const m = String(e);
    if (!/startCleanup/.test(m)) console.log("    PAGEERROR", m);
  });
  return page;
}

/** Seed last_path, reload, click "Open last", wait for a rendered page. */
async function openViaApp(page, path, settings = {}) {
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.evaluate(
    ([p, extra]) => {
      const base = { default_zoom: 1, last_path: p, ...extra };
      localStorage.setItem("pdfreader.settings.v1", JSON.stringify(base));
    },
    [path, settings]
  );
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(500);
  const btn = page.locator('button:has-text("Open last")').first();
  if (await btn.count()) await btn.click();
  await page.waitForSelector(".pdf-page", { timeout: 30000 });
  await page.waitForTimeout(2000);
}

const openMenu = async (page) => {
  const trigger = page.locator('button[title="Appearance"]');
  if (!(await page.locator(".menu-popover").count())) await trigger.click();
  await page.waitForSelector(".menu-popover", { timeout: 5000 });
  await page.waitForTimeout(300);
};

/** Computed value of a custom property on <html>. */
const cssVar = (page, name) =>
  page.evaluate(
    (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim(),
    name
  );

const pageFilter = (page) =>
  page.evaluate(() => {
    const c = document.querySelector(".pdf-page canvas");
    return c ? getComputedStyle(c).filter : null;
  });

const statusPage = (page) =>
  page.evaluate(() => {
    const m = document.body.innerText.match(/(\d+)\s*\/\s*(\d+)/);
    return m ? +m[1] : NaN;
  });

// ===========================================================================
// A + B + C + D + E — the appearance popover
// ===========================================================================
{
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await newPage(ctx);
  await openViaApp(page, DOC);
  await openMenu(page);

  // --- A. preset gallery ---------------------------------------------------
  const gallery = await page.evaluate(() => {
    const pop = document.querySelector(".menu-popover");
    const heads = [...pop.querySelectorAll("div")]
      .filter((d) => /text-\[10px\]/.test(d.className) && d.textContent.trim())
      .map((d) => d.textContent.trim());
    const swatches = [...pop.querySelectorAll(".preset-swatch")];
    return {
      groups: heads,
      swatches: swatches.length,
      names: [...pop.querySelectorAll("button[aria-pressed] .truncate, button[aria-pressed] span")]
        .map((s) => s.textContent.trim())
        .filter(Boolean),
    };
  });
  check(gallery.swatches >= 8, "preset gallery renders thumbnails", `${gallery.swatches} swatches`);
  check(
    gallery.groups.some((g) => /basic/i.test(g)) && gallery.groups.some((g) => /classic/i.test(g)),
    "presets are split into named groups",
    gallery.groups.join(" | ")
  );
  for (const want of ["Sepia", "Green", "Night"]) {
    check(
      gallery.names.some((n) => n === want),
      `${want} survives as a built-in preset`,
    );
  }

  // Each swatch must carry its OWN look, not the applied one — that is the
  // difference between a real preview and a coloured square.
  const swatchLooks = await page.evaluate(() =>
    [...document.querySelectorAll(".preset-swatch")].map((s) => {
      const page = s.querySelector(".preset-page");
      // The filter lives on the stand-in CANVAS, not the page backdrop —
      // mirroring the real DOM, where .pdf-page is unfiltered themed paper and
      // only the canvas carries --canvas-filter.
      const cv = s.querySelector(".preset-canvas");
      return {
        paper: getComputedStyle(page).backgroundColor,
        filter: cv ? getComputedStyle(cv).filter : "MISSING",
      };
    })
  );
  const distinctPapers = new Set(swatchLooks.map((s) => s.paper)).size;
  const distinctFilters = new Set(swatchLooks.map((s) => s.filter)).size;
  check(distinctPapers >= 3, "swatches show different paper colours", `${distinctPapers} distinct`);
  check(
    distinctFilters >= 3,
    "swatches render their own canvas filter (real previews)",
    `${distinctFilters} distinct filters`
  );

  // --- B. applying a preset + the tint maths -------------------------------
  const applyPreset = async (name) => {
    await page.evaluate((n) => {
      const btn = [...document.querySelectorAll(".menu-popover button[aria-pressed]")].find(
        (b) => b.title === n
      );
      btn && btn.click();
    }, name);
    await page.waitForTimeout(700);
  };

  await applyPreset("Sepia");
  const sepiaFilter = await pageFilter(page);
  const sepiaPaper = await cssVar(page, "--color-paper");
  check(
    /sepia\(/.test(sepiaFilter || ""),
    "Sepia applies a computed tint to the page canvas",
    sepiaFilter
  );
  check(
    /color-mix|oklch|rgb/.test(sepiaPaper),
    "the tint also reaches the UI tokens",
    sepiaPaper.slice(0, 60)
  );

  await applyPreset("Night");
  const nightFilter = await pageFilter(page);
  check(
    /invert\(/.test(nightFilter || "") && /sepia\(/.test(nightFilter || ""),
    "Night is an inverted base WITH a tint",
    nightFilter
  );
  const invertBeforeSepia =
    (nightFilter || "").indexOf("invert") < (nightFilter || "").indexOf("sepia");
  check(invertBeforeSepia, "the tint is applied after the inversion, not before");

  await applyPreset("Light");
  const lightFilter = await pageFilter(page);
  check(
    lightFilter === "none",
    "a plain Light preset has NO canvas filter at all",
    String(lightFilter)
  );
  const lightPaper = await cssVar(page, "--color-paper");
  check(
    !/color-mix/.test(lightPaper),
    "clearing the tint removes the UI overrides too",
    lightPaper
  );

  // --- hue + strength sliders ----------------------------------------------
  const setSlider = async (label, value) => {
    await page.evaluate(
      ([l, v]) => {
        const labels = [...document.querySelectorAll(".menu-popover label")];
        const target =
          labels.find((x) => x.textContent.includes(l)) ||
          [...document.querySelectorAll(".menu-popover input[type=range]")].find(
            (i) => (i.getAttribute("aria-label") || "") === l
          )?.closest("label");
        const input = target
          ? target.querySelector("input[type=range]")
          : [...document.querySelectorAll(".menu-popover input[type=range]")].find(
              (i) => i.getAttribute("aria-label") === l
            );
        if (!input) return;
        const setter = Object.getOwnPropertyDescriptor(
          window.HTMLInputElement.prototype,
          "value"
        ).set;
        setter.call(input, String(v));
        input.dispatchEvent(new Event("input", { bubbles: true }));
      },
      [label, value]
    );
    await page.waitForTimeout(500);
  };

  await setSlider("Tint strength", 70);
  const strongFilter = await pageFilter(page);
  check(
    /sepia\(/.test(strongFilter || ""),
    "the tint-strength slider drives the canvas filter",
    strongFilter
  );

  await page.evaluate(() => {
    const input = [...document.querySelectorAll(".menu-popover input[type=range]")].find(
      (i) => i.getAttribute("aria-label") === "Tint colour"
    );
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value"
    ).set;
    setter.call(input, "220");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await page.waitForTimeout(600);
  const blueFilter = await pageFilter(page);
  const hueOf = (f) => {
    const m = /hue-rotate\((-?[\d.]+)deg\)/.exec(f || "");
    return m ? +m[1] : null;
  };
  check(
    hueOf(blueFilter) !== hueOf(strongFilter),
    "the hue picker changes the rotation",
    `${hueOf(strongFilter)}deg -> ${hueOf(blueFilter)}deg`
  );
  // 220 is measured from sepia's own 34deg output.
  check(
    Math.abs((hueOf(blueFilter) ?? 0) - (220 - 34)) < 1.5,
    "hue is absolute (measured from sepia's 34deg output)",
    `${hueOf(blueFilter)}deg for hue 220`
  );

  // Editing a preset must detach from it rather than lie about the selection.
  const activeCount = await page.evaluate(
    () =>
      [...document.querySelectorAll('.menu-popover button[aria-pressed="true"]')].filter((b) =>
        b.querySelector(".preset-swatch")
      ).length
  );
  check(activeCount === 0, "editing a preset deselects it (menu shows no active preset)");

  // --- C. texture opacity + scale ------------------------------------------
  await page.evaluate(() => {
    const b = [...document.querySelectorAll(".menu-popover button")].find(
      (x) => x.textContent.trim() === "Lined"
    );
    b && b.click();
  });
  await page.waitForTimeout(600);
  const texApplied = await page.evaluate(() =>
    document.querySelector(".pdf-page")?.className.includes("texture-lined")
  );
  check(texApplied, "choosing a texture applies it to the page");

  await setSlider("Texture opacity", 30);
  const op30 = await cssVar(page, "--texture-opacity");
  await setSlider("Texture opacity", 95);
  const op95 = await cssVar(page, "--texture-opacity");
  check(
    parseFloat(op30) < parseFloat(op95) && Math.abs(parseFloat(op95) - 0.95) < 0.02,
    "the texture-opacity slider reaches the page",
    `${op30} -> ${op95}`
  );
  const beforeOpacity = await page.evaluate(() => {
    const el = document.querySelector(".pdf-page");
    return getComputedStyle(el, "::before").opacity;
  });
  check(
    Math.abs(parseFloat(beforeOpacity) - 0.95) < 0.03,
    "the texture layer actually honours the opacity",
    beforeOpacity
  );

  await setSlider("Texture scale", 200);
  const sc200 = await cssVar(page, "--texture-scale-user");
  check(Math.abs(parseFloat(sc200) - 2) < 0.02, "the texture-scale slider reaches the page", sc200);

  // Appendix 7 must not regress: the texture is part of the PAGE, so its pitch
  // must still track zoom even with a user scale applied.
  // Read the pitch out of the resolved background-image, the way verify-zoom
  // does: custom properties on a ::before do not reliably read back through
  // getPropertyValue, and the gradient is the value that actually paints.
  const pitchAt = () =>
    page.evaluate(() => {
      const el = document.querySelector(".pdf-page");
      const bg = getComputedStyle(el, "::before").backgroundImage;
      // getComputedStyle serialises `transparent` as rgba(0,0,0,0), so match
      // the trailing pitch the way verify-zoom.mjs does.
      const m = /([\d.]+)px\)\s*$/.exec(bg);
      return { pitch: m ? +m[1] : null, w: el.getBoundingClientRect().width };
    });
  const p1 = await pitchAt();
  await page.click('button[title="Zoom in (+)"]');
  await page.waitForTimeout(1400);
  const p2 = await pitchAt();
  check(
    p1.pitch !== null && p2.pitch !== null,
    "the lined texture exposes a readable pitch",
    `${p1.pitch} -> ${p2.pitch}`
  );
  const pageGrew = p2.w / p1.w;
  const pitchGrew = p1.pitch ? p2.pitch / p1.pitch : 0;
  check(
    Math.abs(pageGrew - pitchGrew) < 0.05,
    "the texture still zooms WITH the page (appendix 7 holds)",
    `page x${pageGrew.toFixed(3)}, pitch x${pitchGrew.toFixed(3)}`
  );

  // The zoom button is OUTSIDE the popover, so that click dismissed it (by
  // design — pointerdown anywhere else closes the menu). Reopen before
  // continuing, or every later control lookup silently finds nothing.
  await openMenu(page);

  // --- D. film grain --------------------------------------------------------
  const grainMode = async (name) => {
    await page.evaluate((n) => {
      const b = [...document.querySelectorAll(".menu-popover button")].find(
        (x) => x.textContent.trim() === n
      );
      b && b.click();
    }, name);
    await page.waitForTimeout(500);
  };

  await grainMode("Static");
  let body = await page.evaluate(() => document.body.className);
  check(/noise-enabled/.test(body) && !/noise-animated/.test(body), "static grain turns on", body);

  await grainMode("Animated");
  body = await page.evaluate(() => document.body.className);
  check(/noise-animated/.test(body), "animated grain sets its own class", body);

  // ...and it must actually MOVE, not merely carry a class.
  const moved = await page.evaluate(async () => {
    const ov = document.querySelector(".noise-overlay");
    if (!ov) return null;
    const read = () => getComputedStyle(ov, "::after").transform;
    const seen = new Set();
    for (let i = 0; i < 14; i += 1) {
      seen.add(read());
      await new Promise((r) => setTimeout(r, 120));
    }
    return seen.size;
  });
  check(moved !== null && moved > 1, "animated grain genuinely moves over time", `${moved} distinct transforms`);

  // ...and it must be VISIBLE, which is NOT the same as being enabled.
  // The overlay used `mix-blend-mode: overlay`, which is nearly a no-op on both
  // white paper and a near-black dark theme: 80% grain rendered identically to
  // no grain on exactly the two backgrounds people read on. Prove visibility by
  // screenshotting a blank patch of page with grain off and on and requiring
  // the PIXELS to differ — a class-name assertion cannot catch this.
  const patchOf = async () => {
    const clip = await page.evaluate(() => {
      const r = document.querySelector(".pdf-page").getBoundingClientRect();
      return {
        x: Math.round(r.x + r.width * 0.5),
        y: Math.round(Math.max(r.y, 0) + 40),
        width: 140,
        height: 90,
      };
    });
    return page.screenshot({ clip });
  };

  await grainMode("Off");
  await page.waitForTimeout(500);
  const patchOff = await patchOf();

  await grainMode("Static");
  await page.evaluate(() => {
    const inputs = [...document.querySelectorAll(".menu-popover input[type=range]")];
    const i = inputs[inputs.length - 1]; // grain intensity is the last slider
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value"
    ).set;
    setter.call(i, "100");
    i.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await page.waitForTimeout(700);
  const patchOn = await patchOf();

  const blend = await page.evaluate(
    () => getComputedStyle(document.querySelector(".noise-overlay")).mixBlendMode
  );
  check(
    blend === "multiply" || blend === "screen",
    "grain uses a blend direction that can actually change the paper",
    blend
  );
  check(
    !patchOff.equals(patchOn),
    "grain is VISIBLY rendered on the page (pixels change)",
    `${patchOff.length}B vs ${patchOn.length}B`
  );

  await grainMode("Off");
  body = await page.evaluate(() => document.body.className);
  check(!/noise-enabled/.test(body), "grain turns back off", body || "(none)");

  // --- E. saving a custom preset -------------------------------------------
  await applyPreset("Sepia");
  await setSlider("Tint strength", 62);
  await page.evaluate(() => {
    const b = [...document.querySelectorAll(".menu-popover button")].find((x) =>
      /Save current look/.test(x.textContent)
    );
    b && b.click();
  });
  await page.waitForTimeout(400);
  await page.evaluate(() => {
    const set = (sel, v) => {
      const i = document.querySelector(sel);
      if (!i) throw new Error("missing input: " + sel);
      const setter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        "value"
      ).set;
      setter.call(i, v);
      i.dispatchEvent(new Event("input", { bubbles: true }));
    };
    set('.menu-popover input[aria-label="Preset name"]', "My Warm");
    set('.menu-popover input[aria-label="Preset section"]', "Evening");
  });
  await page.waitForTimeout(200);
  await page.evaluate(() => {
    const b = [...document.querySelectorAll(".menu-popover button")].find(
      (x) => x.textContent.trim() === "Save"
    );
    b && b.click();
  });
  await page.waitForTimeout(700);

  const saved = await page.evaluate(() => {
    const s = JSON.parse(localStorage.getItem("pdfreader.settings.v1") || "{}");
    return { presets: s.user_presets || [], active: s.active_preset };
  });
  check(saved.presets.length === 1, "the custom preset is persisted", JSON.stringify(saved.presets.map((p) => p.name)));
  check(saved.presets[0]?.group === "Evening", "it lands in the named section", saved.presets[0]?.group);
  check(saved.active === saved.presets[0]?.id, "saving selects the new preset", String(saved.active));
  check(
    saved.presets[0]?.appearance?.tint_strength === 62,
    "the preset captured the whole look, sliders included",
    `strength ${saved.presets[0]?.appearance?.tint_strength}`
  );

  // It must come back after a reload, in its own group.
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1200);
  const btn = page.locator('button:has-text("Open last")').first();
  if (await btn.count()) await btn.click();
  await page.waitForSelector(".pdf-page", { timeout: 30000 });
  await page.waitForTimeout(1500);
  await openMenu(page);
  const afterReload = await page.evaluate(() => {
    const pop = document.querySelector(".menu-popover");
    const heads = [...pop.querySelectorAll("div")]
      .filter((d) => /text-\[10px\]/.test(d.className))
      .map((d) => d.textContent.trim());
    const mine = [...pop.querySelectorAll("button[title]")].some((b) => b.title === "My Warm");
    return { heads, mine };
  });
  check(afterReload.mine, "the custom preset survives a reload");
  check(
    afterReload.heads.some((h) => /evening/i.test(h)),
    "its user-named section renders as a group header",
    afterReload.heads.join(" | ")
  );

  await ctx.close();
}

// ===========================================================================
// F. clickable links
// ===========================================================================
{
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await newPage(ctx);
  await openViaApp(page, LINKED);

  const links = await page.evaluate(() =>
    [...document.querySelectorAll(".pdf-page .linkLayer .pdf-link")].map((a) => ({
      href: a.getAttribute("href"),
      target: a.getAttribute("target"),
      rel: a.getAttribute("rel"),
      page: a.dataset.page || null,
      w: Math.round(a.getBoundingClientRect().width),
      h: Math.round(a.getBoundingClientRect().height),
    }))
  );
  check(links.length >= 5, "a link layer is built over the page", `${links.length} links`);
  check(
    links.every((l) => l.w > 0 && l.h > 0),
    "every link has a real hit area"
  );

  const internal = links.filter((l) => l.page);
  const external = links.filter((l) => l.href && l.href !== "#");
  check(internal.length >= 3, "internal destinations are resolved to page numbers", `${internal.length}`);
  check(external.length >= 2, "external URLs are kept as hrefs", `${external.length}`);
  check(
    external.every((l) => l.target === "_blank" && /noopener/.test(l.rel || "")),
    "external links open in a new tab with noopener"
  );
  check(
    !links.some((l) => /^javascript:/i.test(l.href || "")),
    "javascript: URLs are refused (not clickable)"
  );

  // The whole point: clicking an internal link must navigate.
  const before = await statusPage(page);
  await page.evaluate(() => {
    const a = [...document.querySelectorAll(".pdf-link")].find((x) => x.dataset.page);
    a && a.click();
  });
  await page.waitForTimeout(1600);
  const after = await statusPage(page);
  check(after !== before, "clicking an internal link jumps to its page", `page ${before} -> ${after}`);

  // Links must sit ABOVE the text layer or the transparent spans eat the click.
  const stack = await page.evaluate(() => {
    const t = document.querySelector(".pdf-page .textLayer");
    const l = document.querySelector(".pdf-page .linkLayer");
    return {
      text: t ? +getComputedStyle(t).zIndex : null,
      link: l ? +getComputedStyle(l).zIndex : null,
      linkPointer: l ? getComputedStyle(l).pointerEvents : null,
      anchorPointer: getComputedStyle(document.querySelector(".pdf-link")).pointerEvents,
    };
  });
  check(stack.link > stack.text, "the link layer sits above the text layer", `${stack.link} > ${stack.text}`);
  check(
    stack.linkPointer === "none" && stack.anchorPointer === "auto",
    "only the anchors take the pointer, so text selection still works",
    `${stack.linkPointer} / ${stack.anchorPointer}`
  );

  // A hit test at the centre of a link must reach the anchor, not one of the
  // text layer's transparent spans. Only links actually inside the viewport
  // can be tested — elementFromPoint returns null for off-screen coordinates,
  // and an earlier jump may have scrolled some of them away.
  const hit = await page.evaluate(() => {
    const inView = [...document.querySelectorAll(".pdf-link")].filter((a) => {
      const r = a.getBoundingClientRect();
      return r.width > 0 && r.y > 0 && r.y + r.height < innerHeight && r.x > 0;
    });
    if (!inView.length) return { tested: 0, onLink: 0 };
    let onLink = 0;
    for (const a of inView) {
      const r = a.getBoundingClientRect();
      const el = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      if (el && String(el.className).includes("pdf-link")) onLink += 1;
    }
    return { tested: inView.length, onLink };
  });
  check(
    hit.tested > 0 && hit.onLink === hit.tested,
    "a click at the centre of a link actually lands on the link",
    `${hit.onLink}/${hit.tested} in-view links hit-test to the anchor`
  );

  // ...and a REAL mouse click (not a synthetic .click()) navigates, which is
  // what the user actually does.
  const realBefore = await statusPage(page);
  const target = await page.evaluate(() => {
    const a = [...document.querySelectorAll(".pdf-link")].find((x) => {
      const r = x.getBoundingClientRect();
      return x.dataset.page && r.y > 0 && r.y + r.height < innerHeight;
    });
    if (!a) return null;
    const r = a.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  });
  if (target) {
    await page.mouse.click(target.x, target.y);
    await page.waitForTimeout(1600);
  }
  const realAfter = await statusPage(page);
  check(
    target !== null && realAfter !== realBefore,
    "a real mouse click on a link navigates",
    `page ${realBefore} -> ${realAfter}`
  );

  // Links must survive a zoom, at the new geometry.
  const boxBefore = await page.evaluate(() => {
    const a = document.querySelector(".pdf-link");
    return Math.round(a.getBoundingClientRect().width);
  });
  await page.click('button[title="Zoom in (+)"]');
  await page.waitForTimeout(1800);
  const boxAfter = await page.evaluate(() => {
    const a = document.querySelector(".pdf-link");
    return a ? Math.round(a.getBoundingClientRect().width) : null;
  });
  check(
    boxAfter !== null && boxAfter > boxBefore,
    "links are rebuilt at the new scale after a zoom",
    `${boxBefore}px -> ${boxAfter}px`
  );

  await ctx.close();
}

// ===========================================================================
// G. migration from the pre-refactor settings schema
// ===========================================================================
{
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await newPage(ctx);
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  // Exactly what an existing install has on disk today.
  await page.evaluate(() => {
    localStorage.setItem(
      "pdfreader.settings.v1",
      JSON.stringify({
        theme_id: "green",
        texture: "grid",
        noise_enabled: true,
        noise_intensity: 55,
        default_zoom: 1,
        last_path: "/samples/Outlined Book.pdf",
      })
    );
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1500);

  const migrated = await page.evaluate(() =>
    JSON.parse(localStorage.getItem("pdfreader.settings.v1") || "{}")
  );
  check(
    migrated.appearance?.base === "light" && migrated.appearance?.tint_strength > 0,
    "an old Green install migrates to a tinted Light base",
    JSON.stringify(migrated.appearance || {}).slice(0, 90)
  );
  check(migrated.appearance?.texture === "grid", "the old texture choice is carried across");
  check(
    migrated.appearance?.noise === "static" && migrated.appearance?.noise_intensity === 55,
    "the old grain settings are carried across",
    `${migrated.appearance?.noise} @ ${migrated.appearance?.noise_intensity}`
  );
  check(migrated.theme_id === undefined, "the legacy fields are dropped after migrating");

  await ctx.close();
}

await browser.close();

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
if (failed.length) {
  console.log("FAILED:");
  for (const f of failed) console.log(`  - ${f.label} ${f.detail}`);
  process.exit(1);
}
