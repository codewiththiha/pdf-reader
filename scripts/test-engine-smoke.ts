// Entry point: runs every engine-smoke scenario in document order.
// Compiled by the Trunk pre-build hook to scripts/test-engine-smoke.js;
// CI runs it with plain `node`.
//
// The `./engine-smoke/*.js` imports below are GENERATED, not committed:
// `npm run build:ts` (which CI runs before this step) compiles them from the
// `.ts` sources, and `.gitignore` excludes them. They are imported as `.js`
// because this runs under plain Node with no TypeScript loader — the
// scenario files stay the source of truth.

(async () => {
  await (await import("./engine-smoke/open.js")).run();
  await (await import("./engine-smoke/render.js")).run();
  await (await import("./engine-smoke/theme.js")).run();
  await (await import("./engine-smoke/thumbnail.js")).run();
  await (await import("./engine-smoke/blend.js")).run();
  await (await import("./engine-smoke/teardown.js")).run();
  console.log("ALL ENGINE TESTS PASSED");
})().catch((e: unknown) => {
  console.error("TEST FAILURE:", e);
  process.exit(1);
});
