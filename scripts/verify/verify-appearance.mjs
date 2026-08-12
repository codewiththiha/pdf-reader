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

// ===========================================================================
// H. tint keeps the UI hierarchy (light mode, high strength)
//
// REGRESSION: the tint mixed every token TOWARD the tint colour, which dragged
// paper (L=1.00), surface (0.97) and line (0.93) to a common lightness. At 90%
// green the page, sidebar, toolbar and thumbnail cards became one flat slab
// with no edges, and the accent stayed BLUE because mixing a saturated blue
// 31% toward green barely moves its hue. Dark mode hid it: its bases start low,
// so being pulled up still left them dark enough to read as chrome.
// ===========================================================================
{
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 820 } });
  const page = await newPage(ctx);
  await openViaApp(page, DOC, {
    appearance: {
      base: "light", tint_hue: 104, tint_strength: 90, texture: "none",
      texture_opacity: 90, texture_scale: 100, noise: "off", noise_intensity: 25,
    },
  });

  const lch = async (name) =>
    page.evaluate((n) => {
      const el = document.createElement("div");
      el.style.color = getComputedStyle(document.documentElement).getPropertyValue(n).trim();
      document.body.appendChild(el);
      const v = getComputedStyle(el).color;
      el.remove();
      const m = /oklch\(([\d.]+)\s+([\d.]+)\s+([\d.]+)/.exec(v);
      return m ? { l: +m[1], c: +m[2], h: +m[3] } : null;
    }, name);

  const paper = await lch("--color-paper");
  const surface = await lch("--color-surface");
  const line = await lch("--color-line");
  const accent = await lch("--color-accent");

  check(
    paper && surface && paper.l > surface.l + 0.01,
    "light+90% tint: the page stays brighter than the chrome",
    paper && surface ? `paper L=${paper.l.toFixed(3)} vs surface L=${surface.l.toFixed(3)}` : "n/a"
  );
  check(
    surface && line && surface.l > line.l + 0.01,
    "light+90% tint: the chrome stays brighter than its borders",
    surface && line ? `surface L=${surface.l.toFixed(3)} vs line L=${line.l.toFixed(3)}` : "n/a"
  );
  check(
    paper && Math.abs(paper.l - 1.0) < 0.01,
    "light+90% tint: paper keeps its own lightness (does not darken)",
    paper ? `L=${paper.l.toFixed(3)}` : "n/a"
  );
  // The accent must follow the tint, not stay blue on a green page.
  const hueDist = (a, b) => Math.min(Math.abs(a - b), 360 - Math.abs(a - b));
  check(
    accent && paper && hueDist(accent.h, paper.h) < 25,
    "light+90% tint: the accent follows the tint hue (no stranded blue)",
    accent && paper ? `accent h=${accent.h.toFixed(0)} vs paper h=${paper.h.toFixed(0)}` : "n/a"
  );

  // The panels must be visually distinguishable from the page, not merged.
  const bgs = await page.evaluate(() => {
    const rgb = (s) => {
      const el = document.querySelector(s);
      return el ? getComputedStyle(el).backgroundColor : null;
    };
    return { aside: rgb("aside"), page: rgb(".pdf-page"), card: rgb(".thumb-card") };
  });
  check(
    bgs.aside && bgs.page && bgs.aside !== bgs.page,
    "light+90% tint: the sidebar does not merge into the page",
    `${bgs.aside} vs ${bgs.page}`
  );

  await ctx.close();
}

// ===========================================================================
// I. toolbar readouts stay legible
//
// REGRESSION: every toolbar span was white + mix-blend-difference. That works
// for near-black/near-white ink but fails for mid-grey: `--color-muted`
// differenced against the light glass resolved to near-white, and the "/ 12"
// total rendered at a measured luminance spread of 7/255 — invisible.
// ===========================================================================
for (const base of ["light", "dark"]) {
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 820 } });
  const page = await newPage(ctx);
  await openViaApp(page, DOC, {
    appearance: {
      base, tint_hue: 34, tint_strength: 0, texture: "none",
      texture_opacity: 90, texture_scale: 100, noise: "off", noise_intensity: 25,
    },
  });
  // Single-page mode is where the x/y navigator lives.
  await page.evaluate(() =>
    [...document.querySelectorAll("button")].find((b) => b.title === "Single page view")?.click()
  );
  await page.waitForTimeout(1200);

  // Measure the PAINTED pixels of each readout: the glyphs must actually
  // contrast with what is behind them.
  const spread = async (text) => {
    const clip = await page.evaluate((t) => {
      const s = [...document.querySelectorAll("header span")].find(
        (x) => x.textContent.trim() === t
      );
      if (!s) return null;
      const r = s.getBoundingClientRect();
      return { x: Math.round(r.x), y: Math.round(r.y), width: Math.max(4, Math.round(r.width)), height: Math.max(4, Math.round(r.height)) };
    }, text);
    if (!clip) return null;
    const buf = await page.screenshot({ clip });
    return page.evaluate(async (b64) => {
      const img = new Image();
      img.src = "data:image/png;base64," + b64;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = img.width;
      c.height = img.height;
      const g = c.getContext("2d");
      g.drawImage(img, 0, 0);
      const d = g.getImageData(0, 0, c.width, c.height).data;
      let min = 999, max = -1;
      for (let i = 0; i < d.length; i += 4) {
        const lum = 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];
        if (lum < min) min = lum;
        if (lum > max) max = lum;
      }
      return Math.round(max - min);
    }, buf.toString("base64"));
  };

  const total = await spread("12");
  check(
    total !== null && total > 60,
    `${base}: the page-total readout is legible (not white-on-white)`,
    `luminance spread ${total}`
  );

  await ctx.close();
}

// ===========================================================================
// J. the sidebar follows the reader
//
// The panels always rendered from the top, so on a long document the reader
// had to hunt for their position — the outline highlight was useless until you
// scrolled to it. Uses a 40-chapter fixture whose outline is taller than the
// panel; `Outlined Book.pdf` fits entirely on screen and cannot show this.
// ===========================================================================
{
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 820 } });
  const page = await newPage(ctx);
  await openViaApp(page, "/samples/Deep Outline.pdf");

  await page.evaluate(() => {
    const a = document.querySelector("aside");
    if (!a || a.getBoundingClientRect().width < 50)
      [...document.querySelectorAll("button")].find((b) => b.title === "Toggle sidebar")?.click();
  });
  await page.waitForTimeout(800);

  // The rail buttons are TOGGLES and their active state is not a stable class
  // token, so drive them and then VERIFY — guessing from className is what made
  // an earlier harness click twice and silently close the panel again.
  // Both panels stay MOUNTED (the inactive one is `visibility:hidden`), so
  // "are there outline rows in the DOM" is true even when Thumbs is showing.
  // Ask whether the outline panel is actually VISIBLE instead — checking the
  // wrong thing here is what made an earlier harness click the tab twice and
  // silently toggle straight back to Thumbs.
  const outlineShowing = () =>
    page.evaluate(() => {
      const row = document.querySelector("aside button[data-outline-index]");
      if (!row) return false;
      let el = row;
      while (el && el !== document.body) {
        if (getComputedStyle(el).visibility === "hidden") return false;
        el = el.parentElement;
      }
      return true;
    });
  const showOutline = async () => {
    if (await outlineShowing()) return true;
    await page.evaluate(() =>
      [...document.querySelectorAll("aside button")]
        .find((x) => x.textContent.trim() === "Outline")
        ?.click()
    );
    await page.waitForTimeout(1300);
    return outlineShowing();
  };
  check(await showOutline(), "the outline panel is showing (precondition)");

  const outlineState = () =>
    page.evaluate(() => {
      const sc = document.querySelector("aside .overflow-y-auto");
      const act = document.querySelector('aside button[aria-current="true"]');
      if (!sc) return null;
      const sr = sc.getBoundingClientRect();
      const ar = act ? act.getBoundingClientRect() : null;
      return {
        scrollTop: Math.round(sc.scrollTop),
        overflows: sc.scrollHeight > sc.clientHeight + 5,
        activeVisible: ar ? ar.top >= sr.top - 1 && ar.bottom <= sr.bottom + 1 : null,
        active: act ? act.textContent.trim().slice(0, 20) : null,
      };
    });

  const s0 = await outlineState();
  check(s0 && s0.overflows, "the fixture's outline is taller than the panel (precondition)");

  // Jump deep into the document; the active entry must be brought into view.
  await page.evaluate(() => {
    const l = document.getElementById("page-list");
    l.scrollTop = l.scrollHeight * 0.9;
  });
  await page.waitForTimeout(2500);
  const s1 = await outlineState();
  check(
    s1 && s1.scrollTop > 0,
    "the outline scrolls to follow the reader",
    s1 ? `scrollTop ${s0.scrollTop} -> ${s1.scrollTop} (${s1.active})` : "n/a"
  );
  check(s1 && s1.activeVisible === true, "the active outline entry is on screen");

  // Scroll the panel away by hand, then re-click the ACTIVE tab: that is the
  // explicit "take me back to where I am" gesture, and it must not close the
  // panel.
  await page.evaluate(() => {
    document.querySelector("aside .overflow-y-auto").scrollTop = 0;
  });
  await page.waitForTimeout(400);
  await page.evaluate(() =>
    [...document.querySelectorAll("aside button")]
      .find((x) => x.textContent.trim() === "Outline")
      ?.click()
  );
  await page.waitForTimeout(1000);
  const s2 = await outlineState();
  check(
    s2 && s2.scrollTop > 0 && s2.activeVisible === true,
    "re-clicking the active Outline tab jumps back to the reader's position",
    s2 ? `scrollTop 0 -> ${s2.scrollTop}` : "n/a"
  );
  check(
    await page.evaluate(() => document.querySelector("aside").getBoundingClientRect().width > 50),
    "re-clicking the active tab does NOT close the sidebar"
  );

  // Same gesture for thumbnails.
  await page.evaluate(() =>
    [...document.querySelectorAll("aside button")]
      .find((x) => x.textContent.trim() === "Thumbs")
      ?.click()
  );
  await page.waitForTimeout(2200);
  // The aside contains TWO scrollers (the outline list and the thumb grid,
  // both mounted). Identify the grid by its content, not by being the first
  // overflowing div.
  const thumbScroller = () =>
    page.evaluate(() => {
      const sc = [...document.querySelectorAll("aside div")].find(
        (d) =>
          d.scrollHeight > d.clientHeight + 20 &&
          d.clientHeight > 100 &&
          d.querySelector("canvas")
      );
      return sc ? Math.round(sc.scrollTop) : null;
    });
  const t1 = await thumbScroller();
  check(t1 !== null && t1 > 0, "the thumbnail grid follows the reader too", `scrollTop ${t1}`);

  await page.evaluate(() => {
    const sc = [...document.querySelectorAll("aside div")].find(
      (d) =>
        d.scrollHeight > d.clientHeight + 20 &&
        d.clientHeight > 100 &&
        d.querySelector("canvas")
    );
    if (sc) sc.scrollTop = 0;
  });
  await page.waitForTimeout(400);
  await page.evaluate(() =>
    [...document.querySelectorAll("aside button")]
      .find((x) => x.textContent.trim() === "Thumbs")
      ?.click()
  );
  await page.waitForTimeout(1500);
  const t2 = await thumbScroller();
  check(
    t2 !== null && t2 > 0,
    "re-clicking the active Thumbs tab jumps back to the current page",
    `scrollTop 0 -> ${t2}`
  );

  await ctx.close();
}

// ===========================================================================
// K. the animated-grain thumbnail is not a mess
//
// `noise-crawl` translates by up to 14px; the swatch is ~49x65px. At `inset: 0`
// the tile's own EDGE was dragged into frame, giving the Cinema thumbnail a
// hard vertical seam and a bare corner.
// ===========================================================================
{
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 880 } });
  const page = await newPage(ctx);
  await openViaApp(page, DOC);
  await openMenu(page);

  const geom = await page.evaluate(() => {
    const sw = document.querySelector('button[title="Cinema"] .preset-swatch');
    if (!sw) return null;
    const pg = sw.querySelector(".preset-page");
    const af = getComputedStyle(pg, "::after");
    const r = pg.getBoundingClientRect();
    return {
      clipped: getComputedStyle(sw).overflow === "hidden",
      overhangX: (parseFloat(af.width) - r.width) / 2,
      overhangY: (parseFloat(af.height) - r.height) / 2,
      anim: af.animationName,
    };
  });
  check(geom !== null, "the Cinema swatch exists");
  check(
    geom && geom.anim === "noise-crawl",
    "the animated preset's thumbnail is animated",
    geom ? geom.anim : "n/a"
  );
  // The crawl peaks at 14px, so the tile must overhang by more than that on
  // both axes or an edge shows.
  check(
    geom && geom.overhangX > 14 && geom.overhangY > 14,
    "the grain tile overhangs the swatch, so no tile edge can slide into view",
    geom ? `overhang ${geom.overhangX.toFixed(0)}x${geom.overhangY.toFixed(0)}px` : "n/a"
  );
  check(geom && geom.clipped, "the swatch clips the oversized tile");

  await ctx.close();
}

// ===========================================================================
// L. a thumbnail depicts the page, so it must be the SAME colour as the page
//
// REGRESSION: `--thumb-bg` was `--color-surface` while the real page host is
// `--color-paper`. A thumbnail canvas is `multiply`-blended over that backdrop,
// and multiplying a white page pixel by the backdrop yields THE BACKDROP — so
// the backdrop IS the paper the reader sees. The two variables were close
// enough in the old fixed themes to hide it, but the computed tint moves them
// apart (paper keeps L=1.00, surface sits at 0.967 with more chroma), so every
// thumbnail rendered darker and more saturated than the page it depicts.
//
// Diagnostic worth keeping: the CORRECT colour flashed for one frame during a
// re-render, because the skeleton cover is unblended. A one-frame flash of the
// right colour means the final composite is wrong, not the render.
// ===========================================================================
{
  // ONE context for every combination: each newContext is a fresh browser
  // process, and this gate already runs a dozen of them — spawning five more
  // was enough to exhaust the sandbox and crash the run mid-suite.
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 820 } });
  const page = await newPage(ctx);
  for (const [base, hue, strength] of [
  ["light", 34, 0],
  ["light", 104, 90],
  ["dark", 104, 90],
  ["dim", 220, 40],
]) {
  await openViaApp(page, DOC, {
    appearance: {
      base, tint_hue: hue, tint_strength: strength, texture: "none",
      texture_opacity: 90, texture_scale: 100, noise: "off", noise_intensity: 25,
    },
  });
  await page.evaluate(() => {
    const a = document.querySelector("aside");
    if (!a || a.getBoundingClientRect().width < 50)
      [...document.querySelectorAll("button")].find((b) => b.title === "Toggle sidebar")?.click();
  });
  await page.waitForTimeout(2500);

  // Sample REAL PIXELS from a blank region of the page and the same relative
  // region of a thumbnail. Comparing CSS variables would not catch this: the
  // filter and blend were already identical, only the backdrop differed.
  const sample = async (sel, fx, fy) => {
    const clip = await page.evaluate(
      ([s, x, y]) => {
        const el = document.querySelector(s);
        if (!el) return null;
        const r = el.getBoundingClientRect();
        return { x: Math.round(r.x + r.width * x), y: Math.round(r.y + r.height * y), width: 6, height: 6 };
      },
      [sel, fx, fy]
    );
    if (!clip) return null;
    const buf = await page.screenshot({ clip });
    return page.evaluate(async (b64) => {
      const img = new Image();
      img.src = "data:image/png;base64," + b64;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = img.width;
      c.height = img.height;
      const g = c.getContext("2d");
      g.drawImage(img, 0, 0);
      const d = g.getImageData(0, 0, c.width, c.height).data;
      return [d[0], d[1], d[2]];
    }, buf.toString("base64"));
  };

  const pagePx = await sample(".pdf-page", 0.5, 0.6);
  const thumbPx = await sample(".thumb-card", 0.5, 0.6);
  const dist =
    pagePx && thumbPx ? Math.max(...pagePx.map((v, i) => Math.abs(v - thumbPx[i]))) : 999;
  check(
    dist <= 6,
    `${base}/${strength}%: thumbnails are the same colour as the page`,
    `page rgb(${pagePx}) vs thumb rgb(${thumbPx}), max channel delta ${dist}`
  );
  }
  await ctx.close();
}

// The backdrop must track the PAGE, not the chrome — asserted directly so the
// intent survives a future refactor of the variables.
{
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 820 } });
  const page = await newPage(ctx);
  await openViaApp(page, DOC, {
    appearance: {
      base: "light", tint_hue: 104, tint_strength: 90, texture: "none",
      texture_opacity: 90, texture_scale: 100, noise: "off", noise_intensity: 25,
    },
  });
  const vars = await page.evaluate(() => {
    const cs = getComputedStyle(document.documentElement);
    const resolve = (v) => {
      const el = document.createElement("div");
      el.style.color = cs.getPropertyValue(v).trim();
      document.body.appendChild(el);
      const out = getComputedStyle(el).color;
      el.remove();
      return out;
    };
    return { thumb: resolve("--thumb-bg"), paper: resolve("--color-paper"), surface: resolve("--color-surface") };
  });
  check(
    vars.thumb === vars.paper,
    "--thumb-bg resolves to the page's paper, not the chrome's surface",
    `thumb=${vars.thumb} paper=${vars.paper} surface=${vars.surface}`
  );
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
