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
 * `an hour` in the next. That is also why it is on the extension surface. A
 * package holds moments of two kinds — the engine's, and the ones it wrote
 * itself — and had no way to word either, so every one of them would have
 * arrived at its own ladder and the window would have spoken several dialects
 * of the same sentence.
 *
 * **Two spellings of a moment, because a package has both.** The engine answers
 * in seconds since the epoch, and a package's own field holds a moment as
 * ISO 8601 text — there is no moment among the field types, so a type that
 * records when something was copied records a string. Taking only the number
 * would leave every such package converting, and the conversion has two silent
 * ways to go wrong: milliseconds passed as seconds read as the far future and
 * say *Just now* about everything, and text that will not parse becomes `NaN`
 * and reads as *NaN days ago* on somebody's screen. Both are wrong in a way
 * nothing reports, so both are decided here instead.
 *
 * Anything this cannot read as a moment is *Never*, which is also what nothing
 * at all answers. That is the honest reading of the only case it happens in: a
 * record carrying no moment this can make sense of is a record with nothing to
 * say about when, and a sentence invented for it would be a guess in the one
 * place a person came to for a fact.
 */
export function elapsed(when: number | string | null | undefined): string {
  const seconds = moment(when);
  if (seconds === null) return "Never";
  const ago = Math.max(0, Math.floor(Date.now() / 1000) - seconds);
  if (ago < 60) return "Just now";
  if (ago < 3600) return `${Math.floor(ago / 60)} min ago`;
  if (ago < 86_400) return `${Math.floor(ago / 3600)} h ago`;
  const days = Math.floor(ago / 86_400);
  return days === 1 ? "Yesterday" : `${days} days ago`;
}

/**
 * Seconds since the epoch, from either spelling, or `null` when there is no
 * moment in it.
 *
 * A number is already seconds — the engine's own unit, and the reason this
 * cannot simply parse everything. `Date.parse` answers in milliseconds, so the
 * string branch divides and the number branch must not.
 */
function moment(when: number | string | null | undefined): number | null {
  if (typeof when === "number") return Number.isFinite(when) ? when : null;
  if (typeof when === "string") {
    const parsed = Date.parse(when);
    return Number.isNaN(parsed) ? null : parsed / 1000;
  }
  return null;
}
