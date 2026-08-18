// Canvas backing-store helpers. A single shared scratch pad is recycled for
// theme-bake intermediates so we never hold 3–4 full-page RGBA buffers.

import type { MaybeCanvas, Raster } from "./types";

/** Force the browser to drop a canvas backing store. */
export function releaseCanvas(canvas: MaybeCanvas): void {
  if (!canvas) return;
  const c = canvas as HTMLCanvasElement;
  try {
    if (c.width !== 0) c.width = 0;
    if (c.height !== 0) c.height = 0;
  } catch (_) {
    /* detached / already gone */
  }
}

let scratch: HTMLCanvasElement | null = null;
let scratchInUse = false;

/** Borrow the shared bake scratchpad. Concurrent callers get a throwaway canvas. */
export function acquireScratch(w: number, h: number): HTMLCanvasElement {
  if (scratchInUse) {
    const tmp = document.createElement("canvas");
    tmp.width = Math.max(1, w);
    tmp.height = Math.max(1, h);
    return tmp;
  }
  if (!scratch) scratch = document.createElement("canvas");
  scratch.width = Math.max(1, w);
  scratch.height = Math.max(1, h);
  scratchInUse = true;
  return scratch;
}

export function releaseScratch(owned?: HTMLCanvasElement | null): void {
  if (owned && owned !== scratch) {
    releaseCanvas(owned);
    return;
  }
  scratchInUse = false;
}

export function isSharedScratch(c: HTMLCanvasElement | null | undefined): boolean {
  return !!c && c === scratch;
}

/** Drop the scratch backing store entirely (document teardown). */
export function disposeScratch(): void {
  if (scratch && !scratchInUse) {
    releaseCanvas(scratch);
    scratch = null;
  }
}

export function blitInto(
  dst: HTMLCanvasElement | null,
  src: Raster | null
): boolean {
  if (!dst || !src) return false;
  const srcW = (src as ImageBitmap).width ?? (src as HTMLCanvasElement).width;
  const srcH = (src as ImageBitmap).height ?? (src as HTMLCanvasElement).height;
  dst.width = srcW;
  dst.height = srcH;
  const ctx = dst.getContext("2d", { alpha: false });
  if (!ctx) return false;
  ctx.drawImage(src as CanvasImageSource, 0, 0);
  return true;
}

export function el(id: string): HTMLElement | null {
  if (typeof id !== "string" || !id) return null;
  return document.getElementById(id);
}

export function fail(name: string, message: string): { ok: false; error: { name: string; message: string } } {
  return { ok: false, error: { name, message } };
}

export function errorInfo(e: unknown): { name: string; message: string } {
  const er = e as { name?: string; message?: string } | null;
  const name = (er && er.name) || "Error";
  const message = (er && er.message) || String(e);
  return { name, message };
}
