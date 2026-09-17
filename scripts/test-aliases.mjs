/**
 * Teaches `node --test` the one import shape this repository writes.
 *
 * Every file here reaches its neighbours as `@/lib/…`, which is a fact of
 * `tsconfig.json` and nothing else: `paths` is read by the compiler and by the
 * bundler, and Node has never heard of it. Without this the test runner fails
 * on the first import of the module under test, with a message about a missing
 * package rather than about a mapping.
 *
 * Rewriting the tests to reach for `../resolve` instead would need no hook at
 * all, and is the reason this file is short rather than absent: a test that
 * imports differently from the code is a test standing next to the code rather
 * than on it. The day `@/lib/console/resolve` stops resolving in the
 * application, a test written that way goes on passing.
 *
 * The `imports` field of `package.json` is the other way Node offers, and it
 * cannot be used: it only maps specifiers beginning with `#`, so taking it
 * would mean the same two spellings of one path.
 */

import { existsSync } from "node:fs";
import { registerHooks } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const src = new URL("../src/", pathToFileURL(fileURLToPath(import.meta.url)));

/**
 * What the bundler tries, in the order it tries it. A directory with an
 * `index` is included because the alias is a whole-repository mapping rather
 * than one for `src/lib/console`, and the first caller outside it would find
 * that out by a failure with no explanation in it.
 */
const ENDINGS = [".ts", ".tsx", "/index.ts", "/index.tsx"];

/** The first spelling of this specifier that is a file, or nothing. */
function found(specifier, base) {
  for (const ending of ENDINGS) {
    const candidate = new URL(specifier + ending, base);
    if (existsSync(candidate)) return candidate.href;
  }
  return null;
}

registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith("@/")) {
      const at = found(specifier.slice("@/".length), src);
      // Nothing matched. Handed on unchanged rather than refused here, so the
      // failure is Node's own about a specifier that does not resolve — which
      // names the importer, where a message invented here would not.
      return next(at ?? specifier, context);
    }

    // A relative import with no extension on it, which is what a bundler
    // resolves and Node does not. Generated code is the reason this is here
    // rather than a convenience: `lezer-generator` writes `./console-tokens`,
    // and a file nobody types cannot be asked to spell its imports our way.
    if (specifier.startsWith(".") && context.parentURL !== undefined) {
      const at = found(specifier, context.parentURL);
      if (at !== null) return next(at, context);
    }

    return next(specifier, context);
  },
});
