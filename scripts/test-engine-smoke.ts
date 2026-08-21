// Entry point: runs every engine-smoke scenario in document order.
// Compiled by the Trunk pre-build hook to scripts/test-engine-smoke.js;
// CI runs it with plain `node`.

(async () => {
  await (await import("./engine-smoke/open.js")).run();
  await (await import("./engine-smoke/render.js")).run();
  await (await import("./engine-smoke/theme.js")).run();
  await (await import("./engine-smoke/thumbnail.js")).run();
  await (await import("./engine-smoke/teardown.js")).run();
  console.log("ALL ENGINE TESTS PASSED");
})().catch((e: unknown) => {
  console.error("TEST FAILURE:", e);
  process.exit(1);
});
