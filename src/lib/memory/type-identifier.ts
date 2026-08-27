/**
 * The identifier a new type will be stored under.
 *
 * Two answers, and which one is given depends entirely on what the name is made
 * of. Kinds live in one narrow alphabet — lower case, digits and underscores —
 * because they travel through record envelopes, agent instructions, the
 * engine's own schema and the keys definitions live under, and every one of
 * those reads them as a bare word.
 *
 * A name written in that alphabet becomes an identifier that looks like itself:
 * "Open question" is stored as `open_question`, which is the word somebody will
 * later type into an instruction for an agent. That is worth more than any
 * uniformity, so it is the first thing tried.
 *
 * A name that alphabet cannot carry — Russian, Chinese, Japanese, Arabic,
 * Hebrew, emoji — gets a generated one. The alternative is transliteration, and
 * that is a guess about a language rather than a fact about a string: the same
 * Cyrillic letter romanises differently for Russian and Serbian, and a window
 * that guessed would put its guess inside every record of the type for ever.
 * A generated identifier says plainly that it is a name for machines, and the
 * name a person reads is stored beside it and is free to be anything.
 *
 * Accents are not a different alphabet. "Décision" and "Fürsorge" are Latin
 * with marks on top, so the marks are removed and the word survives — that is a
 * normalisation, not a translation, and it is the one case where dropping
 * characters still leaves the person's own word.
 */

/** The alphabet a kind is spelled in. */
const GENERATED_LENGTH = 6;
const ALPHABET = "abcdefghijklmnopqrstuvwxyz0123456789";

/**
 * What a name reduces to, or an empty string when nothing of it survives.
 *
 * Empty is a real answer and the caller has to have one ready: it means the
 * name is written in a script this alphabet cannot hold, not that the person
 * typed nothing.
 */
export function identifierFrom(name: string): string {
  return (
    name
      .trim()
      .toLowerCase()
      // Decompose, then drop the combining marks: `é` becomes `e` rather than
      // being deleted with everything else the alphabet does not hold.
      .normalize("NFKD")
      .replace(/\p{Diacritic}/gu, "")
      // Every kind of dash, not only the one on the keyboard: a name typed with
      // an en dash has a word break in it just the same.
      .replace(/[\s\p{Pd}]+/gu, "_")
      .replace(/[^a-z0-9_]/g, "")
      .replace(/_+/g, "_")
      .replace(/^_+|_+$/g, "")
  );
}

/**
 * An identifier for a name that has none, avoiding the ones the project holds.
 *
 * `type_` leads it so that it reads as what it is — an identifier nobody chose
 * — rather than as six characters that might mean something. The suffix is
 * random rather than a hash of the name: a hash would promise that the same
 * name always produces the same identifier, and two types are allowed to share
 * a name.
 */
export function generatedIdentifier(taken: readonly string[]): string {
  // A collision needs two of 36⁶, and the loop still costs nothing. The bound
  // is there so that a caller passing an impossible `taken` cannot hang the
  // window; the last attempt is returned whether it collides or not, and the
  // store's own uniqueness check is what the form reports.
  for (let attempt = 0; attempt < 10; attempt += 1) {
    const candidate = `type_${randomWord(GENERATED_LENGTH)}`;
    if (!taken.includes(candidate)) return candidate;
  }
  return `type_${randomWord(GENERATED_LENGTH)}`;
}

/**
 * Random characters from the kind alphabet.
 *
 * From the platform's cryptographic source where there is one — not because an
 * identifier is a secret, but because it is the generator every runtime has and
 * the one that does not repeat a sequence across two windows opened in the same
 * millisecond.
 */
function randomWord(length: number): string {
  const values = new Uint8Array(length);
  if (typeof crypto !== "undefined" && "getRandomValues" in crypto) {
    crypto.getRandomValues(values);
  } else {
    for (let index = 0; index < length; index += 1) {
      values[index] = Math.floor(Math.random() * 256);
    }
  }
  return Array.from(values, (value) => ALPHABET[value % ALPHABET.length]).join(
    "",
  );
}
