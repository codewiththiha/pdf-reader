// The appearance filter, as numbers.
//
// The definition of record is `pdf_core::appearance::filter`: Rust composes
// the pipeline and hands the result over through `PDFReader.setFilterMatrix`,
// because it is also what produced the `--canvas-filter` string in the first
// place. That used to be a round trip — Rust formatted `sepia(0.300)`, this
// engine regex-parsed it back into the same numbers.
//
// The parser below still exists, but only as a FALLBACK for a consumer with
// no Rust on the other end (the engine's smoke harness is exactly that: it
// evaluates the bundle in a vm sandbox and never calls `setFilterMatrix`).
// Nothing on the bake path reaches it in the real app.

import type { FilterMatrix } from "../types";

/** Validate a matrix arriving over the bridge. Shape errors are treated as
 *  "no matrix" so a malformed push degrades to the string fallback instead of
 *  producing a black page. */
export function coerceFilterMatrix(value: unknown): FilterMatrix | null {
  if (!value || typeof value !== "object") return null;
  const v = value as { m?: unknown; o?: unknown };
  if (!Array.isArray(v.m) || v.m.length !== 9) return null;
  if (!Array.isArray(v.o) || v.o.length !== 3) return null;
  if (!v.m.every((n) => typeof n === "number" && Number.isFinite(n))) return null;
  if (!v.o.every((n) => typeof n === "number" && Number.isFinite(n))) return null;
  return { m: [...(v.m as number[])], o: [...(v.o as number[])] };
}

function tokenToMatrix(tok: string): FilterMatrix | null {
  const m = /^([a-z-]+)\(([^)]*)\)$/.exec(String(tok).trim());
  if (!m || !m[1] || !m[2]) return null;
  const name = m[1];
  const arg = parseFloat(m[2]);
  if (!Number.isFinite(arg)) return null;
  switch (name) {
    case "invert": {
      const k = 1 - 2 * arg;
      return { m: [k, 0, 0, 0, k, 0, 0, 0, k], o: [arg, arg, arg] };
    }
    case "brightness":
      return { m: [arg, 0, 0, 0, arg, 0, 0, 0, arg], o: [0, 0, 0] };
    case "contrast": {
      const off = 0.5 * (1 - arg);
      return { m: [arg, 0, 0, 0, arg, 0, 0, 0, arg], o: [off, off, off] };
    }
    case "saturate": {
      const t = 1 - arg;
      const a = 0.213 * t;
      const b = 0.715 * t;
      const c = 0.072 * t;
      return {
        m: [a + arg, b, c, a, b + arg, c, a, b, c + arg],
        o: [0, 0, 0],
      };
    }
    case "sepia": {
      const S = [0.393, 0.769, 0.189, 0.349, 0.686, 0.168, 0.272, 0.534, 0.131];
      const out: number[] = [];
      for (let i = 0; i < 9; i += 1) {
        const ident = i === 0 || i === 4 || i === 8 ? 1 : 0;
        out.push((1 - arg) * ident + arg * (S[i] ?? 0));
      }
      return { m: out, o: [0, 0, 0] };
    }
    case "hue-rotate": {
      const th = (arg * Math.PI) / 180;
      const c = Math.cos(th);
      const s = Math.sin(th);
      return {
        m: [
          0.213 + 0.787 * c - 0.213 * s,
          0.715 - 0.715 * c - 0.715 * s,
          0.072 - 0.072 * c + 0.928 * s,
          0.213 - 0.213 * c + 0.143 * s,
          0.715 + 0.285 * c + 0.140 * s,
          0.072 - 0.072 * c - 0.283 * s,
          0.213 - 0.213 * c - 0.787 * s,
          0.715 - 0.715 * c + 0.715 * s,
          0.072 + 0.928 * c + 0.072 * s,
        ],
        o: [0, 0, 0],
      };
    }
    default:
      return null;
  }
}

/** Compose a CSS filter string. Fallback only — see the module header. */
export function parseFilter(filterString: string): FilterMatrix | null {
  if (!filterString || filterString === "none") return null;
  let m = [1, 0, 0, 0, 1, 0, 0, 0, 1];
  let o = [0, 0, 0];
  let seen = 0;
  for (const tok of String(filterString).split(/\s+/)) {
    if (!tok) continue;
    const op = tokenToMatrix(tok);
    if (!op) continue;
    seen += 1;
    const nm: number[] = [];
    const no: number[] = [];
    for (let r = 0; r < 3; r += 1) {
      nm[r * 3] =
        op.m[r * 3] * m[0] + op.m[r * 3 + 1] * m[3] + op.m[r * 3 + 2] * m[6];
      nm[r * 3 + 1] =
        op.m[r * 3] * m[1] + op.m[r * 3 + 1] * m[4] + op.m[r * 3 + 2] * m[7];
      nm[r * 3 + 2] =
        op.m[r * 3] * m[2] + op.m[r * 3 + 1] * m[5] + op.m[r * 3 + 2] * m[8];
      no[r] =
        op.m[r * 3] * o[0] + op.m[r * 3 + 1] * o[1] + op.m[r * 3 + 2] * o[2] + op.o[r];
    }
    m = nm;
    o = no;
  }
  // A string that named no filter we recognise is not a matrix, and treating
  // it as the identity would bake nothing where the caller expected
  // something. Leave it null so the bake falls back to the raw raster.
  return seen > 0 ? { m, o } : null;
}
