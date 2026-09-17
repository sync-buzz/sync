/**
 * The one thing about a typed line that a character class cannot say.
 *
 * Inside `!` the line belongs to a shell, so its own marks have to go on
 * working: `!git log | head` is one command with a pipe in it and
 * `!cargo test 2>&1` redirects. What ends the run is `->`, which is two
 * characters — and deciding on the first of them whether it is a mark or part
 * of a word is exactly the look-ahead a regular token cannot do.
 *
 * Lezer asks this only where the parse state allows a `ShellText`, which is
 * after the mark and nowhere else. A path in `!cat /etc/hosts` is therefore
 * never read as an address into memory, and that is the one exception the
 * language has, kept by construction rather than by a check.
 */

import { ExternalTokenizer } from "@lezer/lr";

import { ShellText } from "@/lib/console/console.parser.terms";

const NEWLINE = 10;
const DASH = 45;
const GREATER = 62;

export const shellText = new ExternalTokenizer((input) => {
  let length = 0;
  for (;;) {
    const here = input.peek(length);
    // `-1` is the end of the input. A line is one line, so a newline ends the
    // run for the same reason the end of the input does.
    if (here < 0 || here === NEWLINE) break;
    if (here === DASH && input.peek(length + 1) === GREATER) break;
    length += 1;
  }
  // Nothing to take is not an empty token: `!` on its own is a mark somebody
  // has typed and not yet finished, and a zero-length token would be accepted
  // for ever at the same position.
  if (length > 0) input.acceptToken(ShellText, length);
});
