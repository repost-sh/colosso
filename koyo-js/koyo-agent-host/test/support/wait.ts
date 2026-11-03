// Waiting in tests: for what's queued to run, or for something that has no event to wait on.

/** Lets the steps and frames queued so far run, `rounds` turns of the event loop. */
export async function settle(rounds = 1): Promise<void> {
  for (let round = 0; round < rounds; round += 1) {
    await Bun.sleep(0);
  }
}

/** Waits for `done`, for `ms` at most; whether it came. */
export async function until(done: () => boolean, ms: number): Promise<boolean> {
  const by = Date.now() + ms;
  while (!done() && Date.now() < by) {
    await Bun.sleep(5);
  }
  return done();
}
