// Document loading. Prefer a streaming URL (pdf.js range requests) so a
// 50 MB file is not cloned through Rust Vec → IPC → JS ArrayBuffer → worker.
// Falls back to `read_file_bytes` because the Tauri asset protocol is
// intermittently broken on Windows (`https://asset.localhost`).

import type {
  CoverResult,
  OpenResult,
  OutlineItem,
  PDFDocumentProxy,
} from "./types";
import { errorInfo, fail, releaseCanvas } from "./canvas";
import {
  currentPath,
  loadingTask,
  numPages,
  pdf,
  setCurrentPath,
  setLoadingTask,
  setNumPages,
  setPdf,
} from "./state";

type PdfjsLib = {
  getDocument: (params: Record<string, unknown>) => {
    promise: Promise<PDFDocumentProxy>;
    destroy: () => Promise<void>;
  };
  GlobalWorkerOptions: { workerSrc: string };
  TextLayer: unknown;
};

export const pdfjsLib = globalThis.pdfjsLib as unknown as PdfjsLib;
export const { getDocument, GlobalWorkerOptions } = pdfjsLib;
export const TextLayer = pdfjsLib.TextLayer as {
  new (opts: {
    textContentSource: { items: unknown[] };
    container: HTMLElement;
    viewport: { width: number; height: number };
  }): { render: () => Promise<void>; cancel: () => void };
};

GlobalWorkerOptions.workerSrc = "/vendor/pdfjs/pdf.worker.min.mjs";

const CMAP = { cMapUrl: "/vendor/pdfjs/cmaps/", cMapPacked: true };

async function doFetch(src: string): Promise<Uint8Array> {
  const res = await fetch(src);
  if (!res.ok) {
    throw Object.assign(new Error("HTTP " + res.status), {
      name: "UnexpectedResponseException",
    });
  }
  return new Uint8Array(await res.arrayBuffer());
}

export async function fetchBytes(path: string): Promise<Uint8Array> {
  if (/^https?:\/\//i.test(path)) return doFetch(path);
  const tauri = globalThis.__TAURI__;
  if (tauri && tauri.core && typeof tauri.core.invoke === "function") {
    try {
      const bytes = await tauri.core.invoke("read_file_bytes", { path });
      return new Uint8Array(bytes as ArrayBufferLike);
    } catch (_) {
      /* fall through */
    }
  }
  return doFetch(path);
}

function fileUrlFor(path: string): string | null {
  if (/^https?:\/\//i.test(path) || /^asset:/i.test(path) || path.startsWith("/")) {
    return path;
  }
  const tauri = globalThis.__TAURI__;
  if (tauri && tauri.core && typeof tauri.core.convertFileSrc === "function") {
    try {
      return tauri.core.convertFileSrc(path);
    } catch (_) {
      return null;
    }
  }
  return null;
}

export async function openDocument(path: string): Promise<PDFDocumentProxy> {
  const url = fileUrlFor(path);
  if (url) {
    try {
      const task = getDocument({
        url,
        ...CMAP,
        disableAutoFetch: false,
        disableStream: false,
        rangeChunkSize: 65536,
      });
      setLoadingTask(task);
      return await task.promise;
    } catch (_) {
      /* Windows asset-protocol flake, or a relative path the worker cannot fetch. */
    }
  }
  const bytes = await fetchBytes(path);
  const task = getDocument({
    data: bytes,
    ...CMAP,
    disableAutoFetch: true,
    disableStream: true,
  });
  setLoadingTask(task);
  return await task.promise;
}

function outlineTitle(raw: string | null | undefined): string {
  return String(raw == null ? "" : raw).trim() || "(untitled)";
}

async function flattenOutline(
  items: OutlineItem[] | null | undefined,
  depth: number,
  acc: { title: string; page: number; depth: number }[]
): Promise<typeof acc> {
  for (const it of items || []) {
    let page: number | null = null;
    try {
      if (Array.isArray(it.dest)) {
        const ref = it.dest[0];
        if (ref && typeof ref === "object" && "num" in ref) {
          const idx = await pdf!.getPageIndex(ref);
          page = idx + 1;
        } else if (typeof ref === "number") {
          page = ref + 1;
        }
      } else if (typeof it.dest === "string") {
        const d = await pdf!.getDestination(it.dest);
        if (d && d[0]) {
          const ref = d[0];
          if (ref && typeof ref === "object" && "num" in ref) {
            const idx = await pdf!.getPageIndex(ref);
            page = idx + 1;
          }
        }
      }
    } catch (_) {
      page = null;
    }
    if (page) acc.push({ title: outlineTitle(it.title), page, depth });
    await flattenOutline(it.items, depth + 1, acc);
  }
  return acc;
}

export async function open(path: string): Promise<OpenResult> {
  try {
    // destroy is wired from the facade so we don't import the whole engine here.
    const destroy = (globalThis as unknown as { __pdfDestroy?: () => Promise<void> }).__pdfDestroy;
    if (destroy) await destroy();
    const doc = await openDocument(path);
    setPdf(doc);
    setNumPages(doc.numPages);
    setCurrentPath(path);

    let title: string | null = null;
    let author: string | null = null;
    try {
      const meta = await doc.getMetadata();
      title = (meta && meta.info && meta.info.Title) || null;
      author = (meta && meta.info && meta.info.Author) || null;
    } catch (_) {
      /* exotic docs */
    }

    let outline: { title: string; page: number; depth: number }[] = [];
    try {
      outline = await flattenOutline(await doc.getOutline(), 0, []);
    } catch (_) {
      /* ignore */
    }

    const page1 = await doc.getPage(1);
    const vp = page1.getViewport({ scale: 1 });

    const pageHeights: number[] = new Array(numPages);
    const pageWidths: number[] = new Array(numPages);
    pageHeights[0] = vp.height;
    pageWidths[0] = vp.width;
    for (let n = 2; n <= numPages; n += 1) {
      try {
        const pg = await doc.getPage(n);
        const v = pg.getViewport({ scale: 1 });
        pageHeights[n - 1] = v.height;
        pageWidths[n - 1] = v.width;
        pg.cleanup();
      } catch (_) {
        pageHeights[n - 1] = vp.height;
        pageWidths[n - 1] = vp.width;
      }
    }
    try { page1.cleanup(); } catch (_) { /* ignore */ }

    return {
      ok: true,
      numPages,
      title,
      author,
      outline,
      page1Size: { width: vp.width, height: vp.height },
      pageHeights,
      pageWidths,
    };
  } catch (e) {
    const er = e as { name?: string };
    if (er && er.name === "PasswordException") {
      return fail("encrypted", "This PDF is password-protected.");
    }
    if (
      er &&
      (er.name === "InvalidPDFException" ||
        er.name === "MissingPDFException" ||
        er.name === "UnexpectedResponseException")
    ) {
      const d = errorInfo(e);
      return fail("corrupt", `Could not read this PDF. (${d.name}: ${d.message})`);
    }
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

function renderCoverFromPdf(
  doc: PDFDocumentProxy,
  maxWidth: number
): Promise<{ dataUrl: string; width: number; height: number }> {
  return doc.getPage(1).then((page) => {
    const vp1 = page.getViewport({ scale: 1 });
    const scale = Math.min((maxWidth || 240) / (vp1.width || 1), 2);
    const viewport = page.getViewport({ scale });

    const off = document.createElement("canvas");
    off.width = Math.max(1, Math.floor(viewport.width));
    off.height = Math.max(1, Math.floor(viewport.height));
    const ctx = off.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("no_context");
    return page
      .render({ canvasContext: ctx, viewport })
      .promise.then(() => {
        try { page.cleanup(); } catch (_) { /* ignore */ }
        const dataUrl = off.toDataURL("image/jpeg", 0.82);
        releaseCanvas(off);
        return { dataUrl, width: viewport.width, height: viewport.height };
      });
  });
}

export async function coverDataUrl(path: string, maxWidth = 240): Promise<CoverResult> {
  try {
    if (!path) return fail("no_path", "No path");
    let result: { dataUrl: string; width: number; height: number };
    if (pdf && currentPath === path) {
      result = await renderCoverFromPdf(pdf, maxWidth);
    } else {
      let task: ReturnType<typeof getDocument>;
      const url = fileUrlFor(path);
      if (url) {
        try {
          task = getDocument({
            url,
            ...CMAP,
            disableAutoFetch: true,
            disableStream: false,
            rangeChunkSize: 65536,
          });
        } catch {
          task = getDocument({
            data: await fetchBytes(path),
            ...CMAP,
            disableAutoFetch: true,
            disableStream: true,
          });
        }
      } else {
        task = getDocument({
          data: await fetchBytes(path),
          ...CMAP,
          disableAutoFetch: true,
          disableStream: true,
        });
      }
      try {
        const doc = await task.promise;
        result = await renderCoverFromPdf(doc, maxWidth);
      } finally {
        try { await task.destroy(); } catch (_) { /* ignore */ }
      }
    }
    return { ok: true, dataUrl: result.dataUrl, width: result.width, height: result.height };
  } catch (e) {
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

export async function takePendingFile(): Promise<string | null> {
  const tauri = globalThis.__TAURI__;
  if (!tauri || !tauri.core || typeof tauri.core.invoke !== "function") {
    return null;
  }
  try {
    const path = await tauri.core.invoke("take_pending_file");
    return typeof path === "string" && path ? path : null;
  } catch (_) {
    return null;
  }
}

export { loadingTask };
