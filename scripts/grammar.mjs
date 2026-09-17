/**
 * Builds the console's parser from its grammar, or says it is out of date.
 *
 * The generated parser is committed rather than built on the way past, and the
 * reason is the same one the API surface report is committed for: a file that
 * only exists after a build step is a file nobody reviews. Here it is in the
 * diff, next to the grammar that produced it, and a change to one without the
 * other is a red build rather than a parser that disagrees with its own
 * grammar until somebody happens to regenerate it.
 *
 * `--update` writes the files. With no argument it compares and fails, which
 * is what CI runs.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { buildParserFile } from "@lezer/generator";

const here = (path) => fileURLToPath(new URL(`../${path}`, import.meta.url));

const GRAMMAR = "src/lib/console/console.grammar";
const PARSER = "src/lib/console/console.parser.ts";
const TERMS = "src/lib/console/console.parser.terms.ts";

const built = buildParserFile(readFileSync(here(GRAMMAR), "utf8"), {
  fileName: here(GRAMMAR),
  typeScript: true,
});

const written = [
  [PARSER, built.parser],
  [TERMS, built.terms],
];

const updating = process.argv.includes("--update");

if (updating) {
  for (const [path, text] of written) writeFileSync(here(path), text);
  console.log(`grammar: wrote ${PARSER} and ${TERMS}`);
  process.exit(0);
}

const stale = written.filter(([path, text]) => {
  let held = "";
  try {
    held = readFileSync(here(path), "utf8");
  } catch {
    // Absent counts as out of date, which is the answer somebody who has just
    // deleted it is looking for.
  }
  return held !== text;
});

if (stale.length > 0) {
  console.error(
    `grammar: ${stale
      .map(([path]) => path)
      .join(" and ")} ${stale.length === 1 ? "does" : "do"} not match ${GRAMMAR}.`,
  );
  console.error("Run `pnpm grammar` and commit what it writes.");
  process.exit(1);
}

console.log(`grammar: ${PARSER} matches ${GRAMMAR}.`);
