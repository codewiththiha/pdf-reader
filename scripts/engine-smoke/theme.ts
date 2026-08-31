import {
  EngineResult,
  FakeCtx,
  PDFReader,
  RenderPayload,
  assertClose,
  created,
  expectedBakePixel,
  fakeComputed,
  setFakeComputed,
  getEl,
} from "./harness.js";

export async function run(): Promise<void> {
  // 3. DARK MODE REGRESSION TEST.
  const beforeDark = created.length;
  setFakeComputed({
    "--canvas-filter": "invert(0.92) hue-rotate(180deg) saturate(0.85) brightness(1.02)",
    "--canvas-blend": "screen",
    paper: "#131316",
  });
  await PDFReader.refreshTheme();
  const darkExpect = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "screen", [19, 19, 22]);
  const cv0 = getEl("cont-0-cv") as unknown as { _ctx: FakeCtx };
  const darkPx = cv0._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(darkPx, darkExpect, "dark refreshTheme bake");
  console.log("refreshTheme (dark) ok: page pixel", Array.from(darkPx).slice(0, 3), "expected", darkExpect);

  // 4. render another page while dark.
  PDFReader.registerPage({ canvasId: "cont-1-cv", hostId: "cont-1-pg", page: 2 });
  const r2 = await PDFReader.renderPage("cont-1-cv", 1.5, true);
  if (!r2.ok) throw new Error("render2 failed: " + JSON.stringify(r2));
  const darkAllocs = created.length - beforeDark;
  if (darkAllocs < 1) throw new Error("dark bake should allocate at least one intermediate canvas, got " + darkAllocs);
  const cv1 = getEl("cont-1-cv") as unknown as { _ctx: FakeCtx };
  const darkPx2 = cv1._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(darkPx2, darkExpect, "dark render bake");
  console.log("render ok (dark/baked):", r2.width, "x", r2.height, `(${darkAllocs} canvases)`);

  // 5. scrub mode must restore UNBAKED (near-white) pixels, not leave the
  // baked Dark raster under the live CSS filter (that double-inverts to light).
  await PDFReader.setScrubMode(true);
  const scrubPx = cv0._ctx.getImageData(0, 0, 1, 1).data;
  if (scrubPx[0]! < 200 || scrubPx[1]! < 200 || scrubPx[2]! < 200) {
    throw new Error(
      "scrub should show raw page pixels, got [" +
        Array.from(scrubPx).slice(0, 3).join(",") +
        "]",
    );
  }
  console.log("scrub on ok (raw pixels)", Array.from(scrubPx).slice(0, 3));
  await PDFReader.setScrubMode(false);
  const afterScrub = cv0._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(afterScrub, darkExpect, "scrub off rebake");
  console.log("scrub off ok (rebaked)", Array.from(afterScrub).slice(0, 3));

  // 11. DIM check.
  setFakeComputed({
    "--canvas-filter": "brightness(0.8) saturate(0.75) contrast(0.9)",
    "--canvas-blend": "soft-light",
    paper: "#1a1c1f",
  });
  await PDFReader.refreshTheme();
  PDFReader.registerPage({ canvasId: "cont-2-cv", hostId: "cont-2-pg", page: 3 });
  const r3 = await PDFReader.renderPage("cont-2-cv", 1.5, true);
  if (!r3.ok) throw new Error("render3 failed: " + JSON.stringify(r3));
  const dimExpect = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "soft-light", [26, 28, 31]);
  const cv2 = getEl("cont-2-cv") as unknown as { _ctx: FakeCtx };
  const dimPx = cv2._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(dimPx, dimExpect, "dim bake");
  console.log("dim bake ok: page pixel", Array.from(dimPx).slice(0, 3), "expected", dimExpect);

  // 12. A DARK PRESET WITH A TINT.
  setFakeComputed({
    "--canvas-filter": "invert(0.92) hue-rotate(180deg) saturate(0.85) brightness(1.02) sepia(0.193) saturate(1.21) hue-rotate(76deg)",
    "--canvas-blend": "screen",
    paper: "#131316",
  });
  await PDFReader.refreshTheme();
  PDFReader.registerPage({ canvasId: "cont-3-cv", hostId: "cont-3-pg", page: 4 });
  const r4 = await PDFReader.renderPage("cont-3-cv", 1.5, true);
  if (!r4.ok) throw new Error("render4 failed: " + JSON.stringify(r4));
  const nightExpect = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "screen", [19, 19, 22]);
  const cv3 = getEl("cont-3-cv") as unknown as { _ctx: FakeCtx };
  const nightPx = cv3._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(nightPx, nightExpect, "dark+tint bake");
  console.log("dark+tint bake ok: page pixel", Array.from(nightPx).slice(0, 3), "expected", nightExpect);

  // 13. STRUCTURED FILTER MATRIX: the Rust theme applier hands the composed
  // matrix over directly; a hand-computed invert(0.5) matrix
  // (m = diag(1 - 2·0.5) = 0, o = 0.5) must bake exactly like the string.
  // Fresh render each time so the scenarios never depend on a page raw
  // surviving the 10s idle drop.
  const cv0b = getEl("cont-0-cv") as unknown as { _ctx: FakeCtx };
  const invExpect = expectedBakePixel([255, 255, 255], "invert(0.5)", "normal", [255, 255, 255]);
  setFakeComputed({ "--canvas-filter": "invert(0.5)", "--canvas-blend": "normal" });
  PDFReader.setFilterMatrix({ m: [0, 0, 0, 0, 0, 0, 0, 0, 0], o: [0.5, 0.5, 0.5] });
  PDFReader.registerPage({ canvasId: "cont-0-cv", hostId: "cont-0-pg", page: 1 });
  const r13 = await PDFReader.renderPage("cont-0-cv", 1.5, true);
  if (!r13.ok) throw new Error("render13 failed: " + JSON.stringify(r13));
  assertClose(cv0b._ctx.getImageData(0, 0, 1, 1).data, invExpect, "structured matrix bake");
  console.log("structured matrix bake ok:", Array.from(cv0b._ctx.getImageData(0, 0, 1, 1).data).slice(0, 3), "expected", invExpect);

  // Clearing the matrix returns the pipeline to the CSS-string fallback.
  PDFReader.setFilterMatrix(null);
  setFakeComputed({
    "--canvas-filter": "brightness(0.8) saturate(0.75) contrast(0.9)",
    "--canvas-blend": "soft-light",
    paper: "#1a1c1f",
  });
  PDFReader.registerPage({ canvasId: "cont-0-cv", hostId: "cont-0-pg", page: 1 });
  const r13b = await PDFReader.renderPage("cont-0-cv", 1.5, true);
  if (!r13b.ok) throw new Error("render13b failed: " + JSON.stringify(r13b));
  const dimExpect2 = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "soft-light", [26, 28, 31]);
  assertClose(cv0b._ctx.getImageData(0, 0, 1, 1).data, dimExpect2, "cleared matrix falls back to the CSS string");
  console.log("cleared matrix fallback ok");

  // 14. WASM BAKER DELEGATION: a registered baker owns the pixel transform.
  let delegated = 0;
  PDFReader.setWasmBaker((data) => {
    delegated += 1;
    for (let i = 0; i < data.length; i += 4) {
      data[i] = 10;
      data[i + 1] = 20;
      data[i + 2] = 30;
    }
    return data;
  });
  setFakeComputed({ "--canvas-filter": "invert(0.92)", "--canvas-blend": "normal" });
  PDFReader.registerPage({ canvasId: "cont-0-cv", hostId: "cont-0-pg", page: 1 });
  const r14 = await PDFReader.renderPage("cont-0-cv", 1.5, true);
  if (!r14.ok) throw new Error("render14 failed: " + JSON.stringify(r14));
  if (delegated < 1) throw new Error("the registered baker was never invoked");
  const delPx = cv0b._ctx.getImageData(0, 0, 1, 1).data;
  if (delPx[0]! !== 10 || delPx[1]! !== 20 || delPx[2]! !== 30) {
    throw new Error(
      "wasm baker output was not painted: got [" + Array.from(delPx).slice(0, 3).join(",") + "]",
    );
  }
  console.log("wasm baker delegation ok (invocations:", delegated + ")");

  // A baker that throws is dropped and the local loop bakes instead.
  PDFReader.setWasmBaker(() => {
    throw new Error("busted baker");
  });
  setFakeComputed({ "--canvas-filter": "invert(0.5)", "--canvas-blend": "normal" });
  PDFReader.registerPage({ canvasId: "cont-0-cv", hostId: "cont-0-pg", page: 1 });
  const r14b = await PDFReader.renderPage("cont-0-cv", 1.5, true);
  if (!r14b.ok) throw new Error("render14b failed: " + JSON.stringify(r14b));
  assertClose(cv0b._ctx.getImageData(0, 0, 1, 1).data, invExpect, "throwing baker falls back");
  PDFReader.setWasmBaker(null);
  console.log("wasm baker fallback ok");
}
