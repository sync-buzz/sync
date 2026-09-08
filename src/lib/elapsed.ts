/**
 * How long ago something was, for a list somebody is scanning.
 *
 * *Just now* and *3 days ago* are what a column of events is for — is this
 * still in use, has this been sitting here since Friday — and an exact
 * timestamp answers neither without arithmetic. A date is the other question
 * and a different function: *which one did I set up at the office* is looked
 * up, not scanned.
 *
 * Shared rather than kept beside the first screen that wanted it, because the
 * second one wants exactly the same sentences: two copies of this drift the
 * first time somebody decides an hour should read as `1 h` in one column and
 * `an hour` in the next.
 *
 * The engine answers in seconds, so this takes seconds. Milliseconds passed to
 * it would read as the far future and say *Just now* about all of them, which
 * is the failure that looks like it is working.
 */
export function elapsed(seconds: number | null): string {
  if (seconds === null) return "Never";
  const ago = Math.max(0, Math.floor(Date.now() / 1000) - seconds);
  if (ago < 60) return "Just now";
  if (ago < 3600) return `${Math.floor(ago / 60)} min ago`;
  if (ago < 86_400) return `${Math.floor(ago / 3600)} h ago`;
  const days = Math.floor(ago / 86_400);
  return days === 1 ? "Yesterday" : `${days} days ago`;
}
