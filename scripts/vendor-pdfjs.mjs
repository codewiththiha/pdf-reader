// Vendors the pdf.js files we need from node_modules/pdfjs-dist into
// public/vendor/pdfjs so Trunk's `copy-dir` serves them offline at /vendor/pdfjs/*.
//
// We use the LEGACY build (ES2015-transpiled): it runs on older WKWebView
// (macOS < 14.4) whereas the modern build requires Promise.withResolvers
// (Safari 17.4+). The main lib and worker MUST be the same variant + version.
import { cpSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const src = join(root, "node_modules", "pdfjs-dist");
const out = join(root, "public", "vendor", "pdfjs");

if (!existsSync(src)) {
  console.error("pdfjs-dist not found. Run `npm install` first.");
  process.exit(1);
}

mkdirSync(out, { recursive: true });
mkdirSync(join(out, "cmaps"), { recursive: true });

const files = [
  ["legacy/build/pdf.min.mjs", "pdf.min.mjs"],
  ["legacy/build/pdf.worker.min.mjs", "pdf.worker.min.mjs"],
  ["web/pdf_viewer.css", "pdf_viewer.css"],
];

for (const [from, to] of files) {
  const srcPath = join(src, from);
  if (!existsSync(srcPath)) {
    console.error(`missing in pdfjs-dist: ${from}`);
    process.exit(1);
  }
  cpSync(srcPath, join(out, to));
}

// cmaps (CJK + some Western Type0/CID) — required for those PDFs.
cpSync(join(src, "cmaps"), join(out, "cmaps"), { recursive: true });

console.log("vendored pdfjs-dist ->", out);
