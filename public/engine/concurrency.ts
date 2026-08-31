// Bounded-concurrency runner: run async jobs with at most `limit` in
// flight, collecting results in job order.
//
// Lifted here from renderer.ts (where it lived privately for the re-render
// lanes) so the search scan and the outline resolver — the two other
// one-round-trip-per-item loops the codebase flags as slow — share one
// implementation instead of each growing its own.

export async function runLimited<T>(
  jobs: Array<() => Promise<T>>,
  limit = 2,
): Promise<T[]> {
  const out: T[] = [];
  let i = 0;
  const workers = Array.from(
    { length: Math.min(Math.max(limit, 1), Math.max(jobs.length, 1)) },
    async () => {
      while (i < jobs.length) {
        const idx = i;
        i += 1;
        const job = jobs[idx];
        if (job) out[idx] = await job();
      }
    },
  );
  await Promise.all(workers);
  return out;
}
