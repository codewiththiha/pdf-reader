// Constant sync check — the same cheap insurance as `check-formats.ts`, for
// the three facts this repo writes down twice where no compiler connects the
// copies:
//
//   - crates/reader-core/src/appearance/base.rs        `base_tokens` — the
//     seven raw colours of each base mode                    <- Rust copy
//     styles/tokens.css  `:root` / `:root[data-base=...]`   <- CSS copy
//
//   - crates/pdf-core/src/layout.rs            `TOOLBAR_H: f64`       <- the
//     height the search reveal and the traffic lights clear (Rust)
//     crates/app-chrome/src/titlebar/root.rs   the bar's `h-N`        <- the
//     class that actually sizes the bar (Tailwind: N * 0.25rem = N * 4 CSS px)
//     crates/app-chrome/src/window/traffic_lights.rs `DEFAULT_HEADER_HEIGHT`
//                                                            <- the observer's
//                                                            Rust fallback
//
//   - src/components/shell/sidebar/panels/thumbnails/geometry.rs
//     `THUMB_SCALE` — the scale the rail renders and caches at
//     src/services/document/open/warmup.rs     `THUMB_SCALE` — the scale the
//     post-open warm-up prefetches at
//
// All three drift one-sided: a palette edited only in Rust leaves the chrome
// a different dark than the paper, a bar re-sized only in the class leaves the
// search reveal clearing the old band, and a warm-up that renders at a scale
// the rail never requests is a cache miss with extra steps. Each site's own
// comment already asks the next reader to keep the pair in step — this script
// makes CI do the asking instead.
//
// This is the TypeScript source; Trunk's pre-build hook compiles it to
// `scripts/check-constants.js` so CI can run it with plain `node`.

import { read } from "./repo.js";

const BASE_RS = "crates/reader-core/src/appearance/base.rs";
const TOKENS_CSS = "styles/tokens.css";
const PDF_LAYOUT_RS = "crates/pdf-core/src/layout.rs";
const TITLEBAR_ROOT_RS = "crates/app-chrome/src/titlebar/root.rs";
const TRAFFIC_LIGHTS_RS = "crates/app-chrome/src/window/traffic_lights.rs";
const THUMB_GEOMETRY_RS = "src/components/shell/sidebar/panels/thumbnails/geometry.rs";
const THUMB_WARMUP_RS = "src/services/document/open/warmup.rs";

const problems: string[] = [];

// ---------------------------------------------------------------------------
// The base palettes: Rust match arms vs the CSS `:root[data-base=...]` blocks.
// ---------------------------------------------------------------------------

/** `(token, value)` pairs of one mode, keyed by token name (`--base-ink`). */
type Palette = Map<string, string>;

function parseRustBases(): Map<string, Palette> {
  const text = read(BASE_RS);
  const modes = new Map<string, Palette>();
  for (const mode of ["Light", "Dark", "Dim"]) {
    const block = new RegExp(`BaseMode::${mode}\\s*=>\\s*BaseTokens\\s*\\{([\\s\\S]*?)\\}`).exec(text);
    if (!block || !block[1]) {
      throw new Error(`${BASE_RS}: cannot find the BaseTokens literal for ${mode}`);
    }
    const tokens: Palette = new Map();
    for (const field of block[1].matchAll(/(\w+)\s*:\s*"([^"]+)"/g)) {
      tokens.set(`--base-${field[1]!.replace(/_/g, "-")}`, field[2]!);
    }
    // Seven tokens is the palette's contract; parsing fewer means the literal
    // changed shape under the pattern, which this check must scream about.
    if (tokens.size !== 7) {
      throw new Error(`${BASE_RS}: ${mode} parsed as ${tokens.size} tokens, expected 7`);
    }
    modes.set(mode.toLowerCase(), tokens);
  }
  return modes;
}

function parseCssBases(): Map<string, Palette> {
  const text = read(TOKENS_CSS);
  const modes = new Map<string, Palette>();
  const rule = /:root(?:\[data-base="([^"]+)"])?\s*\{([^}]*)\}/g;
  for (let m = rule.exec(text); m; m = rule.exec(text)) {
    const body = m[2]!;
    // Only the palette blocks define `--base-*`; the alias blocks below them
    // reference the tokens with `var(--base-...)`, which the colon test keeps
    // out. A bare `:root` without a mode is the light palette.
    if (!body.includes("--base-paper:")) continue;
    const mode = (m[1] ?? "light").toLowerCase();
    const tokens: Palette = new Map();
    for (const declaration of body.matchAll(/(--base-[\w-]+)\s*:\s*([^;]+);/g)) {
      tokens.set(declaration[1]!, declaration[2]!.trim());
    }
    if (tokens.size !== 7) {
      throw new Error(`${TOKENS_CSS}: :root[data-base=${mode}] parsed as ${tokens.size} tokens, expected 7`);
    }
    modes.set(mode, tokens);
  }
  if (modes.size !== 3) {
    throw new Error(`${TOKENS_CSS}: found ${modes.size} base-palette blocks, expected 3`);
  }
  return modes;
}

const rustBases = parseRustBases();
const cssBases = parseCssBases();
for (const [mode, rust] of rustBases) {
  const css = cssBases.get(mode)!;
  for (const [token, value] of rust) {
    if (css.get(token) !== value) {
      problems.push(
        `${TOKENS_CSS} :root[data-base=${mode}] ${token} is ${css.get(token)} but ${BASE_RS} ${mode} is ${value}`,
      );
    }
  }
}

// ---------------------------------------------------------------------------
// A `const NAME: f64 = <number>;` anywhere in a Rust file.
// ---------------------------------------------------------------------------
function rustConst(path: string, name: string): number {
  const text = read(path);
  const m = new RegExp(`(?:pub\\s+)?const\\s+${name}\\s*:\\s*f64\\s*=\\s*([\\d.]+)`).exec(text);
  if (!m || !m[1]) throw new Error(`${path}: cannot find const ${name}: f64`);
  return parseFloat(m[1]);
}

// ---------------------------------------------------------------------------
// The toolbar height: Rust reference vs the Tailwind class vs the fallback.
// Tailwind `h-N` is N * 0.25rem, and the bar is measured in CSS px, so the
// Rust height must be N * 4.
// ---------------------------------------------------------------------------
const toolbarH = rustConst(PDF_LAYOUT_RS, "TOOLBAR_H");
const barClass = /\{BAR\} h-(\d+)/.exec(read(TITLEBAR_ROOT_RS));
if (!barClass || !barClass[1]) {
  throw new Error(`${TITLEBAR_ROOT_RS}: cannot find the title bar's height class ({BAR} h-N)`);
}
const barClassPx = parseInt(barClass[1], 10) * 4;
if (barClassPx !== toolbarH) {
  problems.push(
    `${TITLEBAR_ROOT_RS} styles the bar h-${barClass[1]} (${barClassPx}px) but ${PDF_LAYOUT_RS} TOOLBAR_H is ${toolbarH}`,
  );
}
const headerFallback = rustConst(TRAFFIC_LIGHTS_RS, "DEFAULT_HEADER_HEIGHT");
if (headerFallback !== toolbarH) {
  problems.push(
    `${TRAFFIC_LIGHTS_RS} DEFAULT_HEADER_HEIGHT is ${headerFallback} but ${PDF_LAYOUT_RS} TOOLBAR_H is ${toolbarH}`,
  );
}

// ---------------------------------------------------------------------------
// The thumbnail scale: what the rail caches at vs what the warm-up renders.
// ---------------------------------------------------------------------------
const railScale = rustConst(THUMB_GEOMETRY_RS, "THUMB_SCALE");
const warmScale = rustConst(THUMB_WARMUP_RS, "THUMB_SCALE");
if (railScale !== warmScale) {
  problems.push(
    `${THUMB_WARMUP_RS} THUMB_SCALE is ${warmScale} but ${THUMB_GEOMETRY_RS} is ${railScale} ` +
      `— the warm-up would render at a scale the rail never requests`,
  );
}

// ---------------------------------------------------------------------------
if (problems.length > 0) {
  console.error("::error::Mirrored constants disagree:");
  for (const problem of problems) console.error(`  ${problem}`);
  console.error("");
  console.error("  Each pair names its source of truth in a comment at the site; edit it there.");
  process.exit(1);
}

console.log(
  `constants agree: ${rustBases.size} base palettes, toolbar ${toolbarH}px (h-${barClass[1]}), thumbnail scale ${railScale}`,
);
