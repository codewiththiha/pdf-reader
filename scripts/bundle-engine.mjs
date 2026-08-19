// Flatten public/engine/*.js + public/pdfEngine.js into a SINGLE
// public/pdfEngine.js with no import/export. Tauri's custom protocol does
// not reliably resolve relative ES-module specifiers, so a multi-file engine
// left window.PDFReader unset and froze open / theme / menus.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function stripModuleSyntax(src) {
  return src
    .replace(/^\s*import\s+type\s+[^;]+;\s*$/gm, "")
    .replace(/^\s*import\s+[^;]+;\s*$/gm, "")
    .replace(/^\s*export\s+type\s+[^;]+;\s*$/gm, "")
    .replace(/^\s*export\s+\{\s*\}[;\s]*$/gm, "")
    .replace(/^\s*export\s+\{[^}]*\}[;\s]*$/gm, "")
    .replace(/^export\s+async\s+function\s+/gm, "async function ")
    .replace(/^export\s+function\s+/gm, "function ")
    .replace(/^export\s+const\s+/gm, "const ")
    .replace(/^export\s+let\s+/gm, "let ")
    .replace(/^export\s+class\s+/gm, "class ")
    .replace(/^\s*export\s+\{\s*\}\s*;?\s*$/gm, "");
}

const files = [
  "public/engine/state.js",
  "public/engine/canvas.js",
  "public/engine/theme.js",
  "public/engine/loader.js",
  "public/engine/renderer.js",
  "public/engine/thumbnails.js",
  "public/engine/search.js",
  "public/engine/selection.js",
  "public/pdfEngine.js",
];

const bundled = files
  .map((rel) => stripModuleSyntax(readFileSync(join(root, rel), "utf8")))
  .join("\n;\n");

writeFileSync(join(root, "public/pdfEngine.js"), bundled);
console.log("bundled engine -> public/pdfEngine.js (" + bundled.length + " bytes)");
