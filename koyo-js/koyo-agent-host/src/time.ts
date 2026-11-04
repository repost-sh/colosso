// Waiting a limited time.

/** Waits for `promise`, if there's one, for `ms` at most. */
export async function within(promise: Promise<unknown> | undefined, ms: number): Promise<void> {
  const late = Promise.withResolvers<undefined>();
  const timer = setTimeout(late.resolve, ms, undefined);
  await Promise.race([promise ?? Promise.resolve(), late.promise]);
  clearTimeout(timer);
}
