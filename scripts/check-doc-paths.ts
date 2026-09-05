// Doc-path check — the third piece of cheap insurance in this pipeline, after
// `check-versions.ts` and `check-formats.ts`.
//
// This repo's comments carry a lot of weight: module docs explain why a design
// is the way it is, and they name the other modules that make it work. Those
// names rot silently. A rename or a move leaves the prose pointing at a module
// that no longer exists, nothing fails, and the next reader follows the sign to
// an empty field — a `cargo doc` link would warn, but most of these are plain
// backticked prose, which no tool reads.
//
// So: every module path (`crate::a::b`, `super::x`, `ai_core::gloss::y`) and
// every file path with a slash in it (`effects/reader/zoom.rs`) that appears in
// a Rust comment must resolve. Resolution is deliberately shallow — it checks
// that the MODULE exists and that the last name is declared or re-exported
// there, which is all a comment promises — and deliberately conservative: a path
// whose first segment is not one of ours is assumed to be an external crate and
// skipped, and a module that re-exports with a glob passes whatever it is asked
// about.
//
// This is the TypeScript source; Trunk's pre-build hook compiles it to
// `scripts/check-doc-paths.js` so CI can run it with plain `node`.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function read(rel: string): string {
  return fs.readFileSync(path.join(root, rel), "utf8");
}

function isFile(rel: string): boolean {
  return fs.existsSync(path.join(root, rel)) && fs.statSync(path.join(root, rel)).isFile();
}

/** Every directory in the repo, minus the ones that are not source. */
const SKIP_DIRS = new Set([".git", "node_modules", "target", "dist", ".arena", "out", "build"]);

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of fs.readdirSync(path.join(root, dir), { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue;
      walk(path.posix.join(dir, entry.name), out);
    } else {
      out.push(path.posix.join(dir, entry.name));
    }
  }
  return out;
}

const ALL_FILES = walk(".");

/** The Rust files whose comments are checked. */
const RUST_FILES = ALL_FILES.filter(
  (file) =>
    file.endsWith(".rs") &&
    (file.startsWith("src/") || file.startsWith("crates/") || file.startsWith("src-tauri/")),
);

// ---------------------------------------------------------------------------
// Crate roots, so `ai_core::gloss` knows which `src/` it starts from.
// ---------------------------------------------------------------------------
const CRATE_ROOTS = new Map<string, string>();
for (const cargo of ALL_FILES.filter((file) => file.endsWith("Cargo.toml"))) {
  if (cargo.includes("/tests/") || cargo.includes("target/")) continue;
  const name = /^name\s*=\s*"([^"]+)"/m.exec(read(cargo))?.[1];
  if (!name) continue;
  const dir = path.posix.dirname(cargo);
  const src = path.posix.join(dir, "src");
  if (fs.existsSync(path.join(root, src))) CRATE_ROOTS.set(name.replace(/-/g, "_"), src);
}

/**
 * First segments that are never ours: Rust's own primitives and the external
 * crates this workspace builds against. A path starting with anything else
 * unknown is skipped too — the check only speaks for names it can resolve.
 */
const SKIP_FIRST = new Set(
  `i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 usize isize bool char str
   std core alloc serde serde_json leptos web_sys js_sys wasm_bindgen tauri
   tachys reactive_graph virtual_list virtual_list_leptos getrandom itertools
   thiserror futures objc2 cocoa`
    .split(/\s+/)
    .filter((name) => name.length > 0),
);

const DECL = /\b(?:fn|struct|enum|trait|const|static|type|mod|union)\s+([A-Za-z_][A-Za-z0-9_]*)/g;
const FIELD = /\b([A-Za-z_][A-Za-z0-9_]*)\s*:/g;
const USE_LINE = /^\s*(?:pub(?:\([^)]*\))?\s+)?use\s[^;]*;/gm;
const GLOB_USE = /pub\s+use\s[^;]*::\*;/;
const MODULE_PATH = /\b(crate|super|self|[a-z][a-z0-9_]*)((?:::)[A-Za-z_][A-Za-z0-9_]*)+/g;
const FILE_PATH = /[\w.\/-]*[\w-]\/[\w.\/-]+\.(?:rs|ts|css|json|toml|md|html|mjs)/g;

/** The file that makes `base` a module, if any. */
function moduleFile(base: string): string | null {
  if (isFile(`${base}.rs`)) return `${base}.rs`;
  const mod = path.posix.join(base, "mod.rs");
  if (isFile(mod)) return mod;
  return null;
}

/** The crate root a file's `crate::` refers to. */
function crateRootOf(file: string): string {
  if (file.startsWith("src-tauri/")) return "src-tauri/src";
  if (file.startsWith("crates/")) return file.slice(0, file.indexOf("/src/")) + "/src";
  return "src";
}

/** The longest prefix of `segs` under `base` that is a module. */
function resolveModules(base: string, segs: string[]): { count: number; file: string | null } {
  let current = base;
  let count = 0;
  let file: string | null = null;
  for (let i = 0; i < segs.length; i++) {
    current = path.posix.join(current, segs[i]!);
    const found = moduleFile(current);
    if (!found) break;
    file = found;
    count = i + 1;
  }
  return { count, file };
}

/** The crate-root module file (`lib.rs` / `main.rs`), for facade re-exports. */
function rootModule(dir: string): string | null {
  for (const name of ["lib.rs", "main.rs", "mod.rs"]) {
    if (isFile(path.posix.join(dir, name))) return path.posix.join(dir, name);
  }
  return moduleFile(dir);
}

/** Whether `item` is something `file` declares, re-exports, or waves through. */
function declaresItem(file: string, item: string): boolean {
  const body = read(file);
  // A glob re-export forwards names this script cannot enumerate.
  if (GLOB_USE.test(body)) return true;
  for (const m of body.matchAll(DECL)) if (m[1] === item) return true;
  for (const m of body.matchAll(FIELD)) if (m[1] === item) return true;
  for (const m of body.matchAll(USE_LINE)) if (m[0].includes(item)) return true;
  return false;
}

const problems: string[] = [];
let checked = 0;

for (const file of RUST_FILES) {
  const crateRoot = crateRootOf(file);
  const fileDir = path.posix.dirname(file);
  // The module a file's own items live in: its directory, except for a `mod.rs`,
  // which IS its directory's module.
  const ownModule = path.posix.basename(file) === "mod.rs" ? path.posix.dirname(fileDir) : fileDir;
  // Where a comment's unprefixed module name (`page_host::block_row_id`) is
  // looked for: the crate root, then the file's own directory and its parents.
  // The file's DIRECTORY, not its module — prose in a `mod.rs` usually means a
  // sibling of the file it is written in.
  const ancestors = [crateRoot, fileDir];
  const top = crateRoot.split("/")[0]!;
  for (
    let up = path.posix.dirname(fileDir);
    up !== "." && up.startsWith(top);
    up = path.posix.dirname(up)
  ) {
    ancestors.push(up);
  }

  const lines = read(file).split("\n");
  lines.forEach((line, index) => {
    const at = line.indexOf("//");
    if (at < 0) return;
    const comment = line.slice(at);
    const lineNo = index + 1;

    // A URL's `://` would otherwise be read as a comment starting mid-string.
    const withoutUrl = comment.includes("://") ? comment.slice(0, comment.indexOf("://")) : comment;

    for (const m of withoutUrl.matchAll(MODULE_PATH)) {
      const whole = m[0];
      const first = m[1]!;
      let segs = whole.split("::").slice(1);
      if (SKIP_FIRST.has(first)) continue;

      let base: string;
      if (first === "crate") {
        base = crateRoot;
      } else if (first === "self") {
        base = ownModule;
      } else if (first === "super") {
        const all = whole.split("::");
        let supers = 0;
        while (supers < all.length && all[supers] === "super") supers++;
        segs = all.slice(supers);
        base = ownModule;
        for (let i = 1; i < supers; i++) base = path.posix.dirname(base);
      } else if (CRATE_ROOTS.has(first)) {
        base = CRATE_ROOTS.get(first)!;
      } else {
        // Prose that names a module without its crate: try the crate root and
        // the file's own neighbourhood. If nothing there has that name, it is
        // somebody else's crate and none of this script's business.
        const known = ancestors.find((dir) => moduleFile(path.posix.join(dir, first)));
        if (!known) continue;
        base = known;
        segs = [first, ...segs];
      }

      checked++;
      let { count, file: modFile } = resolveModules(base, segs);
      if (count === 0) {
        // Nothing below the root is a module, but the root itself may re-export
        // the name (a crate's facade `pub use`).
        const rootFile = rootModule(base);
        if (!rootFile) {
          problems.push(`${file}:${lineNo}: no module at \`${whole}\` (root ${base})`);
          continue;
        }
        modFile = rootFile;
      }
      if (!modFile) continue;
      if (count === segs.length) continue;
      const item = segs[count]!;
      if (!declaresItem(modFile, item)) {
        problems.push(
          `${file}:${lineNo}: \`${item}\` is not declared or re-exported by ${modFile} (from \`${whole}\`)`,
        );
      }
    }

    for (const m of comment.matchAll(FILE_PATH)) {
      const token = m[0].replace(/^[./]+/, "");
      // An elided path ("readest/.../traffic_light.rs") is prose, not a claim.
      if (token.includes("...")) continue;
      // Comments name a file by whatever tail is unambiguous, so match on the
      // end of a real path rather than guessing its prefix.
      const exists = ALL_FILES.some((candidate) => candidate === token || candidate.endsWith(`/${token}`));
      if (!exists) problems.push(`${file}:${lineNo}: no such file: \`${token}\``);
    }
  });
}

if (problems.length > 0) {
  console.error(`::error::${problems.length} comment path(s) do not resolve:`);
  for (const problem of problems) console.error(`  ${problem}`);
  console.error("");
  console.error(
    "  A comment that names a module is a claim about the tree; rename the path with the code.",
  );
  process.exit(1);
}

console.log(`doc paths resolve: ${checked} module paths across ${RUST_FILES.length} Rust files`);
