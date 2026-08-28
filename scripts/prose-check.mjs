#!/usr/bin/env node
/**
 * The prose rules `AGENTS.md` states, for the ones a machine can hold.
 *
 * Prose here is comments and documents — never code and never a string
 * literal. That distinction is the whole reason this is a script rather than a
 * grep: `"key": "d-3ad25f"` in a test is a fixture and has to stay, while the
 * same six characters in the comment above it is a footnote into a database
 * the reader cannot open. A check that cannot tell them apart is a check
 * somebody switches off in its first week.
 *
 * Run it with no arguments over the tracked tree. `--self-test` runs it over
 * inputs that are known to be bad, which is the only way to find out that a
 * check finds nothing because there is nothing to find rather than because it
 * never matches anything.
 */

import { execFileSync } from "node:child_process";
import { readFileSync, existsSync, statSync } from "node:fs";
import path from "node:path";

const ROOT = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();

/* -------------------------------------------------------------------------
 * Prose out of a file
 * ---------------------------------------------------------------------- */

/** A run of prose: the text, and the line it starts on. */
function commentsOf(source, ext) {
  const out = [];
  const rustRaw = ext === ".rs";
  let i = 0;
  let line = 1;
  const push = (text, at) => {
    if (text.trim()) out.push({ line: at, text });
  };

  while (i < source.length) {
    const c = source[i];

    if (c === "\n") {
      line += 1;
      i += 1;
      continue;
    }

    // A raw string in Rust holds anything, including `//`. Skipping it whole is
    // what keeps a fixture out of the prose.
    if (rustRaw && c === "r" && (source[i + 1] === '"' || source[i + 1] === "#")) {
      let hashes = 0;
      let j = i + 1;
      while (source[j] === "#") {
        hashes += 1;
        j += 1;
      }
      if (source[j] === '"') {
        const close = '"' + "#".repeat(hashes);
        const end = source.indexOf(close, j + 1);
        const body = source.slice(i, end === -1 ? source.length : end + close.length);
        line += (body.match(/\n/g) ?? []).length;
        i = end === -1 ? source.length : end + close.length;
        continue;
      }
    }

    // A lifetime is not a string, and reading `'a` as one swallows the rest
    // of the file: every comment after it becomes invisible and the check
    // reports a clean tree. This is the bug that made it report one.
    if (c === "'" && rustRaw) {
      const literal =
        source[i + 1] === "\\" || (source[i + 2] === "'" && source[i + 1] !== undefined);
      if (!literal) {
        i += 1;
        continue;
      }
    }

    if (c === '"' || c === "'" || c === "`") {
      i += 1;
      while (i < source.length && source[i] !== c) {
        if (source[i] === "\\") i += 1;
        else if (source[i] === "\n") line += 1;
        i += 1;
      }
      i += 1;
      continue;
    }

    if (c === "/" && source[i + 1] === "/") {
      const end = source.indexOf("\n", i);
      const stop = end === -1 ? source.length : end;
      push(source.slice(i + 2, stop), line);
      i = stop;
      continue;
    }

    if (c === "/" && source[i + 1] === "*") {
      const end = source.indexOf("*/", i + 2);
      const stop = end === -1 ? source.length : end;
      const body = source.slice(i + 2, stop);
      // One run per line, so a report can name the line rather than the block.
      body.split("\n").forEach((text, offset) => {
        push(text.replace(/^\s*\*/, ""), line + offset);
      });
      line += (body.match(/\n/g) ?? []).length;
      i = stop + 2;
      continue;
    }

    i += 1;
  }

  return out;
}

/** Markdown prose is everything outside a fence. */
function markdownProse(source) {
  const out = [];
  let fence = null;
  source.split("\n").forEach((text, index) => {
    const opening = text.match(/^\s*(`{3,}|~{3,})/);
    if (fence) {
      if (opening && opening[1][0] === fence[0] && opening[1].length >= fence.length) {
        fence = null;
      }
      return;
    }
    if (opening) {
      fence = opening[1];
      return;
    }
    out.push({ line: index + 1, text });
  });
  return out;
}

/* -------------------------------------------------------------------------
 * Sections of a document
 * ---------------------------------------------------------------------- */

const headingCache = new Map();

/** The numbers and the titles a document actually has. */
function headingsOf(file) {
  if (headingCache.has(file)) return headingCache.get(file);
  const numbers = new Set();
  const titles = new Set();
  for (const raw of readFileSync(file, "utf8").split("\n")) {
    const heading = raw.match(/^#{1,6}\s+(.*)$/);
    if (!heading) continue;
    const title = heading[1].trim().replace(/\s*#+\s*$/, "");
    titles.add(title.replace(/[*`]/g, ""));
    const numbered = title.match(/^([0-9]+[0-9a-z.]*?)\.?(?:\s|$)/);
    if (numbered) {
      const number = numbered[1].replace(/\.$/, "");
      numbers.add(number);
      // `§6` is a fair way to name the part `6.3` is in, so every prefix of a
      // number that exists is a number that resolves.
      const parts = number.split(".");
      for (let n = 1; n < parts.length; n += 1) numbers.add(parts.slice(0, n).join("."));
    }
  }
  const found = { numbers, titles };
  headingCache.set(file, found);
  return found;
}

/* -------------------------------------------------------------------------
 * The rules
 * ---------------------------------------------------------------------- */

/**
 * A key in the corpus: the kind, then six of a digest.
 *
 * At least one digit is required of those six, which is what keeps an ordinary
 * hyphenated word out of it — no English word ends in six characters that are
 * all hex and one of them a numeral.
 */
const RECORD_KEY = /\b[A-Za-z][A-Za-z0-9]*(?::[a-z][a-z0-9]*)?(?:-[a-z0-9]+)*-(?=[0-9a-f]{6}\b)(?=[0-9a-f]*[0-9])[0-9a-f]{6}\b/g;

/** A decision numbered somewhere else. */
const FOREIGN_DECISION = /(?<![A-Za-z0-9])D[0-9]{1,3}(?![A-Za-z0-9-])/g;

/** A branch, which is a thing that stops existing. */
const BRANCH = /\b(?:feat|fix|chore|refactor|perf|test|spike|hotfix|release|wip)\/[a-z0-9]+(?:-[a-z0-9]+)+\b/g;

/** Somebody's disk. */
const MACHINE_PATH = /(?:\/Users\/|\/home\/|[A-Z]:\\Users\\)[A-Za-z0-9_.-]+\//g;

/**
 * A document of this repository, as opposed to one in an example.
 *
 * Only the forms this repository uses to name its own are read: a Markdown
 * link, a path under `docs/`, or one of the documents that live at the root.
 * A comment about somebody editing `setup.md` in their own project is naming a
 * file that is none of Sync's business, and a check that demanded it resolve
 * would be demanding the impossible of an accurate sentence.
 */
const DOC_REFERENCE =
  /(?<![A-Za-z0-9_.\\/-])(docs\/[A-Za-z0-9_-]+\.md)\b|(?<![A-Za-z0-9_.\\/-])(README\.md|AGENTS\.md|SECURITY\.md|CHANGELOG\.md|CONTRIBUTING\.md)\b/g;

/** How a document names its neighbour: `background.md`, from inside `docs/`. */
const SIBLING_DOC = /(?<![A-Za-z0-9_.\\/-])([a-z][a-z0-9-]*\.md)\b/g;

/** A section. */
const SECTION = /§\s*(?:"([^"]+)"|“([^”]+)”|([0-9]+(?:\.[0-9]+)*[a-z]?))/g;

/** Documentation that reports its own build status instead of the product. */
const STATUS_WORD = /\b(?:planned|unimplemented|coming soon)\b/gi;
const STATUS_MARK = /\*(?:Built|Unbuilt|Planned|Unplanned|Not built)\b[^*]*\*/g;

/**
 * Packages the shell may not name.
 *
 * Split in two because the words are. `slang` and `routines` are never
 * anything else in this repository, so a comment carrying one is naming a
 * package. `issues`, `chat`, `records` and `tasks` are ordinary English, and a
 * check that failed on every one of them would be switched off before it ever
 * caught a real breach — so those are caught only in the forms that are
 * unmistakably a name: quoted, or capitalised in the middle of a sentence.
 */
const HARD_PACKAGE = /\b(?:slang|routines|project-memory)\b/gi;
const SOFT_PACKAGE = ["issues", "chat", "records", "tasks"];
/** The words that turn an ordinary noun into the name of a thing installed. */
const NAMING = "extension|extensions|package|packages|area|section";

function ruleCitations(file, prose, report) {
  for (const { line, text } of prose) {
    for (const [match] of text.matchAll(RECORD_KEY)) {
      report(file, line, `cites the record \`${match}\`, which no reader of this repository can open`);
    }
    for (const [match] of text.matchAll(FOREIGN_DECISION)) {
      report(file, line, `cites \`${match}\`, a decision numbered in a document that is not here`);
    }
    for (const [match] of text.matchAll(BRANCH)) {
      report(file, line, `cites the branch \`${match}\`, which is a thing that stops existing`);
    }
    for (const [match] of text.matchAll(MACHINE_PATH)) {
      report(file, line, `cites \`${match}\`, a path on somebody's machine`);
    }
  }
}

function resolveDoc(file, reference) {
  const seen = [path.resolve(ROOT, reference)];
  // Walking up is how a reader resolves it: a comment in a crate that names
  // `tests/fixtures/README.md` means the crate's, not the workspace's.
  let dir = path.dirname(path.resolve(ROOT, file));
  while (dir.startsWith(ROOT)) {
    seen.push(path.resolve(dir, reference));
    if (dir === ROOT) break;
    dir = path.dirname(dir);
  }
  return seen.find((c) => existsSync(c) && statSync(c).isFile()) ?? null;
}

function ruleReferences(file, prose, report) {
  const isMarkdown = file.endsWith(".md");
  // What a bare `§` is a section of. A document's own sections are its own
  // unless a line says otherwise; a source file has to have said so somewhere
  // above, and if it never did, the reference opens nothing.
  let carried = null;
  let wrapped = null;

  for (const { line, text } of prose) {
    const named = [];
    const references = [...text.matchAll(DOC_REFERENCE)].map(
      (m) => m[1] ?? m[2],
    );
    if (isMarkdown) {
      for (const [, sibling] of text.matchAll(SIBLING_DOC)) {
        if (!references.includes(sibling)) references.push(sibling);
      }
    }
    for (const reference of references) {
      const resolved = resolveDoc(file, reference);
      if (!resolved) {
        report(file, line, `points at \`${reference}\`, which is not in this repository`);
        continue;
      }
      named.push(resolved);
    }
    const previous = wrapped;
    if (named.length > 0) carried = named[named.length - 1];
    wrapped = named.length > 0 ? named[named.length - 1] : null;

    const sections = [...text.matchAll(SECTION)];
    if (sections.length === 0) continue;
    const leading = previous !== null && text.trimStart().startsWith("§");

    // The same line wins: `docs/background.md §4.1` inside the voice document
    // is background's section, and the sentence after it is voice's again.
    const owner =
      named.length > 0
        ? named[named.length - 1]
        : leading
          ? previous
          : isMarkdown
            ? path.resolve(ROOT, file)
            : carried;
    if (!owner) {
      report(file, line, "names a section with no document, so there is nothing to open");
      continue;
    }
    const { numbers, titles } = headingsOf(owner);
    const relative = path.relative(ROOT, owner);
    for (const [, quoted, curly, number] of sections) {
      const title = quoted ?? curly;
      if (title !== undefined) {
        if (!titles.has(title)) {
          report(file, line, `points at §"${title}" of ${relative}, which has no such section`);
        }
      } else if (!numbers.has(number)) {
        report(file, line, `points at §${number} of ${relative}, which has no such section`);
      }
    }
  }
}

function ruleCurrentVersion(file, prose, report) {
  for (const { line, text } of prose) {
    for (const [match] of text.matchAll(STATUS_WORD)) {
      report(file, line, `says \`${match}\`: documentation describes the version that exists, and what does not goes to "Deliberately absent" in one line`);
    }
    for (const [match] of text.matchAll(STATUS_MARK)) {
      report(file, line, `carries the status mark \`${match}\`: a reader wants the product, not the build log`);
    }
  }
}

function rulePackageNames(file, prose, report) {
  const named = (name) =>
    `names the package \`${name}\`: the shell knows no subject matter, and a ` +
    "comment that names a package announces it on the package's behalf";

  for (const { line, text } of prose) {
    for (const [match] of text.matchAll(HARD_PACKAGE)) {
      report(file, line, named(match));
    }
    for (const name of SOFT_PACKAGE) {
      // Capitalised where a sentence did not begin: `Records` there is a
      // proper noun, and the only proper noun it can be is the package.
      const proper = new RegExp(
        "(\\S)\\s+(" + name[0].toUpperCase() + name.slice(1) + ")\\b",
        "g",
      );
      for (const [, before, match] of text.matchAll(proper)) {
        if (/[.!?:;*]$/.test(before)) continue;
        report(file, line, named(match));
      }
      // Or said to be one, in as many words.
      const qualified = new RegExp(
        "(?:\\b(?:" + NAMING + ")\\s+[`\"']?" + name + "\\b" +
          "|[`\"']?\\b" + name + "[`\"']?\\s+(?:" + NAMING + ")\\b)",
        "gi",
      );
      for (const [match] of text.matchAll(qualified)) {
        report(file, line, named(match.trim()));
      }
    }
  }
}

/* -------------------------------------------------------------------------
 * Walking the tree
 * ---------------------------------------------------------------------- */

const CODE = new Set([".rs", ".ts", ".tsx", ".mjs", ".js"]);

function proseOf(relative, source) {
  const ext = path.extname(relative);
  if (ext === ".md") return markdownProse(source);
  if (CODE.has(ext)) return commentsOf(source, ext);
  return null;
}

function checkFile(relative, source, report) {
  const prose = proseOf(relative, source);
  if (prose === null) return;

  ruleCitations(relative, prose, report);
  ruleReferences(relative, prose, report);
  // Rule three is about what this repository publishes about the product.
  // `AGENTS.md` is not that — it is where the rule is written, and stating a
  // rule means spelling the word it forbids. The same allowance
  // `extensions-outside` makes: prose about a rule is not a breach of it.
  if (relative.startsWith("docs/") || relative === "README.md") {
    // Status words hide in the file listings as readily as in the sentences,
    // so this one rule reads the fences too.
    const everything = relative.endsWith(".md")
      ? source.split("\n").map((text, index) => ({ line: index + 1, text }))
      : prose;
    ruleCurrentVersion(relative, everything, report);
  }
  // Rule four is about the shell, which is the half a package plugs into.
  if (relative.startsWith("src/")) rulePackageNames(relative, prose, report);
}

function tracked() {
  return execFileSync("git", ["ls-files", "-z"], { cwd: ROOT, encoding: "utf8" })
    .split("\0")
    .filter(Boolean)
    .filter((f) => f !== "scripts/prose-check.mjs");
}

function run() {
  const failures = [];
  const report = (file, line, why) => failures.push(`${file}:${line}  ${why}`);
  for (const relative of tracked()) {
    const ext = path.extname(relative);
    if (ext !== ".md" && !CODE.has(ext)) continue;
    checkFile(relative, readFileSync(path.join(ROOT, relative), "utf8"), report);
  }
  return failures;
}

/* -------------------------------------------------------------------------
 * The check on the check
 * ---------------------------------------------------------------------- */

const SELF_TEST = [
  ["src/a.rs", "// see `d-3ad25f` for why\n", true],
  ["src/a.rs", "//! ported from `observation-3f9d21`\n", true],
  ["src/a.rs", "/// as `[[d-3ad25f]]` has it\n", true],
  ["src/a.rs", "/// settled by `SYNC:o-efbfb8`\n", true],
  ["src/a.rs", 'let x = json!({ "key": "d-3ad25f" });\n', false],
  ["src/a.rs", 'assert_eq!(answer, json!({ "title": "d-3ad25f" }));\n', false],
  ["docs/a.md", "As D14 decided, the loader waits.\n", true],
  ["src/a.rs", "// spiked on `chore/acp-live-spike`\n", true],
  ["src/a.rs", "// /Users/nikolai/Projects/sync/x.rs has it\n", true],
  ["src/a.rs", "// `docs/background.md` §11 says so\n", true],
  ["src/a.rs", "// `docs/handoff-background.md` is where this went\n", true],
  ["src/a.rs", "// `docs/background.md` §6.3 says so\n", true, "resolves"],
  ["src/a.rs", "// §7 says so\n", true],
  ["docs/a.md", "The registry is *Planned*.\n", true],
  ["AGENTS.md", "No `Planned`, no `*Built*`: that is the rule.\n", false],
  ["docs/a.md", "| **Palette commands** | ⌘K | planned |\n", true],
  ["docs/a.md", "```\nservice/index.js  the handlers — planned\n```\n", true],
  ["docs/a.md", "- **Declared.** *Built 2026-08-24.* The manifest carries\n", true],
  ["docs/a.md", "The handler joins `OFFERED`. *Unbuilt.*\n", true],
  ["src/a.rs", "struct S<'a> { name: &'a str }\n// then `docs/background.md` §11\n", true],
  ["src/a.ts", "// the slang module compiles to wasm\n", true],
  ["src/a.ts", "// routines runs it on a clock\n", true],
  ["src/a.ts", "// project-memory asks for a second mark\n", true],
  ["src/a.ts", "// The first shape of Issues asked for `owner/name`\n", true],
  ["src/a.ts", "// the issues extension reads a forge\n", true],
  ["src/a.ts", "// the extension `issues` publishes one kind\n", true],
  ["src/a.ts", "// a Chat area that has never run\n", true],
  ["src/a.ts", "// tells *this repository has no issues* apart from\n", false],
  ["src/a.ts", "// letting one project be pointed at another project's issues\n", false],
  ["src-tauri/x.rs", "// slang is fine outside the shell\n", false],
];

function selfTest() {
  let bad = 0;
  for (const [file, source, shouldFail, note] of SELF_TEST) {
    const failures = [];
    checkFile(file, source, (f, l, why) => failures.push(`${f}:${l} ${why}`));
    const failed = failures.length > 0;
    const expected = note === "resolves" ? false : shouldFail;
    if (failed !== expected) {
      bad += 1;
      console.error(
        `self-test: expected ${expected ? "a failure" : "a pass"} for ${JSON.stringify(source)}` +
          (failures.length ? `\n  got: ${failures.join("\n  got: ")}` : "\n  got: nothing"),
      );
    }
  }
  if (bad > 0) {
    console.error(`\n${bad} of the checks does not do what it says. Fix the check before trusting it.`);
    process.exit(1);
  }
  console.log(`prose-check: ${SELF_TEST.length} self-tests pass — every rule fails on a known-bad line.`);
}

if (process.argv.includes("--self-test")) {
  selfTest();
} else {
  selfTest();
  const failures = run();
  if (failures.length > 0) {
    console.error("\nProse that cites what a reader cannot open, or describes what is not here:\n");
    for (const failure of failures) console.error(`  ${failure}`);
    console.error(`\n${failures.length} in all. The rules are in AGENTS.md, "Prose in this repository".`);
    process.exit(1);
  }
  console.log("prose-check: every citation resolves.");
}
