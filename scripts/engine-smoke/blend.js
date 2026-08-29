import { PDFReader, fakeLocalStorage, getEl, setFakePageColors, } from "./harness.js";
/** The --pdf-paper the engine currently has published. */
function paper() {
    const root = getEl("documentElement");
    return root.style.getPropertyValue("--pdf-paper");
}
/** Poll until `want` is the published paper (or fail after ~1s). */
async function untilPaperIs(want, what) {
    for (let i = 0; i < 100; i += 1) {
        if (paper() === want)
            return;
        await new Promise((r) => setTimeout(r, 10));
    }
    throw new Error(`${what}: expected --pdf-paper ${want}, got ${paper() || "(none)"}`);
}
export async function run() {
    // A book whose first page is dark grey and whose other four are white:
    // the single scope must find the dark page, the document scan must decide
    // the book as a whole is white, and the continuous scope has two colours
    // to blend between.
    setFakePageColors({ 1: "#404040", 2: "#ffffff", 3: "#ffffff", 4: "#ffffff", 5: "#ffffff" });
    // --- single scope: the first rendered page stands in for the book ------
    const opened = await PDFReader.open("/fake/blend-book.pdf");
    if (!opened.ok)
        throw new Error("open failed: " + JSON.stringify(opened));
    PDFReader.registerPage({ canvasId: "blend-1-cv", hostId: "blend-1-pg", page: 1 });
    const rendered = await PDFReader.renderPage("blend-1-cv", 1.0, true);
    if (!rendered.ok)
        throw new Error("render failed: " + JSON.stringify(rendered));
    if (paper() !== "#404040") {
        throw new Error("single scope should publish page 1's colour, got " + paper());
    }
    const cached = fakeLocalStorage.get("pdfreader.blend-paper.v1");
    if (!cached || !cached.includes("\"single\":\"#404040\"")) {
        throw new Error("single colour was not cached per document: " + String(cached));
    }
    console.log("blend single ok: #404040 detected + cached");
    // --- document scope: pooled buckets across every page -------------------
    PDFReader.setBlendScope("document");
    // The interim colour stays until the scan lands…
    if (paper() !== "#404040") {
        throw new Error("document scope must keep the interim colour, got " + paper());
    }
    // …then four white pages outvote the one dark page.
    await untilPaperIs("#ffffff", "document scan");
    const cachedDoc = fakeLocalStorage.get("pdfreader.blend-paper.v1");
    if (!cachedDoc || !cachedDoc.includes("\"document\":\"#ffffff\"")) {
        throw new Error("document colour was not cached per document: " + String(cachedDoc));
    }
    console.log("blend document ok: pooled scan found #ffffff + cached");
    // --- persistence: a reopen publishes from the cache, before any render --
    const reopened = await PDFReader.open("/fake/blend-book.pdf");
    if (!reopened.ok)
        throw new Error("reopen failed: " + JSON.stringify(reopened));
    if (paper() !== "#ffffff") {
        throw new Error("cached document colour should publish on open, got " + paper());
    }
    console.log("blend cache ok: #ffffff restored with zero renders");
    // --- continuous scope: per-page colours interpolated by progress -------
    PDFReader.setBlendScope("continuous");
    PDFReader.setBlendPages(1, 2); // look-ahead samples page 1 (#404040) and 2 (#ffffff)
    await untilPaperIs("#404040", "continuous pair at rest");
    PDFReader.setBlendProgress(0.5);
    if (paper() !== "#a0a0a0") { // (64 + 255) / 2 = 159.5 → 160
        throw new Error("continuous mid-blend should be #a0a0a0, got " + paper());
    }
    PDFReader.setBlendProgress(1.0);
    if (paper() !== "#ffffff") {
        throw new Error("continuous at progress 1 should be page 2's colour, got " + paper());
    }
    // A page rendered in continuous mode feeds the palette too: page 3 renders
    // (white), becomes the current page, and pairs with page 4 (white).
    PDFReader.registerPage({ canvasId: "blend-3-cv", hostId: "blend-3-pg", page: 3 });
    const r3 = await PDFReader.renderPage("blend-3-cv", 1.0, true);
    if (!r3.ok)
        throw new Error("render page 3 failed: " + JSON.stringify(r3));
    PDFReader.setBlendPages(3, 4);
    PDFReader.setBlendProgress(0.25);
    await untilPaperIs("#ffffff", "white pair blend");
    console.log("blend continuous ok: #404040 → #a0a0a0 → #ffffff by progress");
    // Back to single: the scope the engine held all along is still cached.
    PDFReader.setBlendScope("single");
    if (paper() !== "#404040") {
        throw new Error("returning to single should republish its colour, got " + paper());
    }
    console.log("blend scope switch back ok");
}
