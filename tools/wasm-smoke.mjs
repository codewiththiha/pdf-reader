// Mount the real wasm bundle in Node, the way the webview does.
//
// CI compiles the frontend and runs the host tests, but nothing ever MOUNTED
// the app: a panic on the library page's first frame is invisible to both —
// the bundle builds, the tests pass, and the reader gets a blank window whose
// only evidence is a line in a webview console nobody is watching. This is
// the missing check: jsdom stands in for the webview, localStorage is seeded
// with a library that resembles a real one (books, a book whose file is
// gone, a link row, a watched folder, a cover), the bundle is required — and
// for a bin crate, requiring runs `main` — and the script asserts that the
// shelf actually painted, with no panic on the way.
//
// Two passes, because the app has two environments:
//   node tools/wasm-smoke.mjs            plain browser: no window.__TAURI__,
//                                        so every shell call is skipped.
//   SMOKE_TAURI=1 node tools/wasm-smoke.mjs
//                                        a stubbed shell: measure_paths
//                                        answers honestly (the gone book is
//                                        gone), scan_folder walks an empty
//                                        folder, so the startup passes —
//                                        verify, then rescan — run for real.
//
// Prereqs (CI provides them): the bundle bound for Node in ./smoke
// (wasm-bindgen --target nodejs) and jsdom installed.

import { existsSync, readdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { JSDOM, VirtualConsole } from 'jsdom';

const WITH_TAURI = process.env.SMOKE_TAURI === '1';
const SMOKE_DIR = path.resolve('smoke');

// ---------------------------------------------------------------------------
// The seed: a library a reader could actually have. Shapes must match the
// serde wire format exactly (LibraryBlob/Row/Book/Fingerprint/Origin/Shelf/
// WatchedFolder are all camelCase; Row and Origin are tagged on "kind").
// ---------------------------------------------------------------------------

const fp = { size: 1000, mtimeMs: 1_700_000_000_000, headHash: 7 };

const book = (id, title, src, extra = {}) => ({
  kind: 'book',
  id,
  fp,
  title,
  format: 'pdf',
  origin: { kind: 'linked', src },
  addedMs: 1_700_000_000_000,
  lastReadMs: 0,
  page: 1,
  numPages: 12,
  ...extra,
});

const library = {
  books: [
    book('b1', 'Dune', '/home/demo/books/dune.pdf'),
    book('b2', 'Neuromancer', '/home/demo/books/neuromancer.pdf'),
    // The case the Find-again flow exists for: the row stays, the file is
    // gone, and the card paints the missing badge on the first frame.
    book('b3', 'Gone Book', '/home/demo/books/gone.pdf', { missing: true }),
    { kind: 'link', id: 'l1', name: 'Dune (alias)', target: 'b1', addedMs: 1_700_000_000_000 },
  ],
  shelves: [{ id: 's1', name: 'Favourites', books: ['b1', 'b3'] }],
  folders: [{ id: 'f1', root: '/home/demo/books', opts: { watch: true } }],
};

const covers = {
  '/home/demo/books/dune.pdf': {
    dataUrl: 'data:image/gif;base64,R0lGODlhAQABAAAAACw=',
    width: 120,
    height: 160,
  },
};

// Settings are deliberately NOT seeded: the defaults path is the one a fresh
// install takes, and every added field must survive an old stored blob —
// both are serde defaults, and a missing default is a mount the reader never
// gets past.

// ---------------------------------------------------------------------------
// The environment. jsdom is the webview; the shims below are only for APIs
// jsdom lacks that every real webview has — a shim here must never stand in
// for something the bundle should find missing.
// ---------------------------------------------------------------------------

const errors = [];
const notes = [];

// Collect on NODE's console, not jsdom's: console_error_panic_hook — where a
// Rust panic lands — writes through the wasm-bindgen glue straight to the
// host console, and so does everything the virtual console forwards. The
// hook has to be on before the JSDOM is built, so sendTo binds the hooked
// methods and page logs and wasm panics arrive through the one door.
const hook = (level, sink) => {
  const original = console[level].bind(console);
  console[level] = (...args) => {
    sink.push(
      args
        .map((a) => (typeof a === 'string' ? a : safeStringify(a)))
        .join(' '),
    );
    original(...args);
  };
};
hook('error', errors);
hook('warn', notes);

const virtualConsole = new VirtualConsole();
virtualConsole.on('jsdomError', (e) => {
  errors.push(`jsdomError: ${e && (e.stack || e.message)}`);
});
virtualConsole.forwardTo(console); // jsdom >= 27; sendTo on older

function safeStringify(value) {
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'http://localhost:1420/',
  pretendToBeVisual: true, // requestAnimationFrame, like a visible window
  virtualConsole,
});
const { window } = dom;

// Present in every webview the app ships in, absent from jsdom.
class NoopObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return [];
  }
}
window.ResizeObserver ??= NoopObserver;
window.IntersectionObserver ??= NoopObserver;
window.Element.prototype.scrollIntoView ??= function scrollIntoView() {};
window.HTMLElement.prototype.scrollIntoView ??= function scrollIntoView() {};
window.fetch ??= () =>
  Promise.resolve(new Response('', { status: 404, statusText: 'Not Found' }));
window.Response ??= Response;

if (WITH_TAURI) {
  // The shell, stubbed at the contract the frontend declares in
  // crates/tauri-bridge: measure answers honestly for the seed (the gone
  // book stays gone, the rest measure to the fingerprint they carry, so the
  // startup pass changes nothing), a walk finds an empty folder, and every
  // window method resolves to a benign answer.
  const ANY_METHOD = () => Promise.resolve(null);
  window.__TAURI__ = {
    core: {
      invoke: (cmd, args) => {
        notes.push(`invoke: ${cmd}`);
        switch (cmd) {
          case 'verify_paths':
            return Promise.resolve(
              (args?.paths ?? []).map((p) => ({
                path: p,
                exists: !p.endsWith('gone.pdf'),
                size: fp.size,
                mtimeMs: fp.mtimeMs,
                headHash: fp.headHash,
              })),
            );
          case 'scan_folder':
            return Promise.resolve([]);
          default:
            return Promise.resolve(null);
        }
      },
    },
    event: { listen: () => Promise.resolve(() => {}) },
    window: {
      getCurrentWindow: () =>
        new Proxy(
          {},
          {
            get: () => ANY_METHOD,
          },
        ),
    },
    dialog: { open: () => Promise.resolve(null) },
  };
}

// The bundle reaches for these as bare globals, the way it would off
// window.* in the webview.
globalThis.window = window;
globalThis.document = window.document;
globalThis.navigator = window.navigator;
globalThis.location = window.location;
globalThis.localStorage = window.localStorage;
globalThis.sessionStorage = window.sessionStorage;
globalThis.history = window.history;
for (const name of [
  'Node',
  'Element',
  'HTMLElement',
  'HTMLInputElement',
  'HTMLCanvasElement',
  'Event',
  'CustomEvent',
  'MouseEvent',
  'KeyboardEvent',
  'DragEvent',
  'FocusEvent',
  'WheelEvent',
  'PointerEvent',
  'InputEvent',
  'ResizeObserver',
  'IntersectionObserver',
  'MutationObserver',
  'DOMParser',
  'getComputedStyle',
  'requestAnimationFrame',
  'cancelAnimationFrame',
  'matchMedia',
  'Image',
  'Response',
  'fetch',
  'Headers',
  'Request',
  'AbortController',
  'File',
  'FileReader',
  'Blob',
  'URL',
  'URLSearchParams',
]) {
  if (name in window) globalThis[name] = window[name];
}

window.localStorage.setItem('pdfreader.library.v3', JSON.stringify(library));
window.localStorage.setItem('pdfreader.covers.v1', JSON.stringify(covers));

// An async panic after a successful mount is still a bug the reader meets;
// keep the process alive to collect it instead of dying on the spot.
process.on('unhandledRejection', (reason) => {
  errors.push(`unhandledRejection: ${reason && (reason.stack || reason.message || reason)}`);
});

// ---------------------------------------------------------------------------
// The mount. For a bin crate the bound module runs `main` on require, so this
// call IS the app starting: if the first frame panics, it throws here, and
// console_error_panic_hook has already put the message in `errors`.
// ---------------------------------------------------------------------------

const glueName = existsSync(SMOKE_DIR)
  ? readdirSync(SMOKE_DIR).find((f) => f.endsWith('.js') && !f.endsWith('.d.ts'))
  : undefined;
if (!glueName) {
  console.error(`no bound module in ${SMOKE_DIR} — run wasm-bindgen --target nodejs first`);
  process.exit(2);
}

let mountThrew = null;
try {
  createRequire(import.meta.url)(path.join(SMOKE_DIR, glueName));
} catch (e) {
  mountThrew = e;
}

// Let spawned futures, effects and the (stubbed) shell calls settle, then
// read what the webview would be showing.
await new Promise((r) => setTimeout(r, 1000));

const body = window.document.body;
const html = body.innerHTML;
const panicked = errors.some((e) => /panicked at|RuntimeError: unreachable/.test(e));
const painted = html.includes('Dune') && html.includes('data-tauri-drag-region');

console.log(`\n=== wasm smoke (${WITH_TAURI ? 'stubbed shell' : 'plain browser'}) ===`);
console.log(`mount threw:      ${mountThrew ? mountThrew.message : 'no'}`);
console.log(`body children:    ${body.children.length}`);
console.log(`body html bytes:  ${html.length}`);
console.log(`shelf painted:    ${painted ? 'yes' : 'NO'}`);
console.log(`panics:           ${panicked ? 'YES' : 'none'}`);
if (notes.length) console.log(`notes:\n  ${notes.join('\n  ')}`);
if (errors.length) console.log(`errors:\n  ${errors.join('\n  ')}`);
console.log(`body snippet:     ${html.slice(0, 300)}`);

const ok = !mountThrew && !panicked && painted;
console.log(ok ? 'SMOKE PASS' : 'SMOKE FAIL');
process.exit(ok ? 0 : 1);
