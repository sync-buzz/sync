import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

/*
 * There was a rule here, and its absence is the point.
 *
 * It restricted what `src/extensions/**` could import — the best a repository
 * can do about code it contains. No extension is contained here now: they are
 * built in `sync-extensions` against the published declarations of one module,
 * where there is no Sync source to reach into and an import past the contract
 * does not resolve. The rule became a fact about where the code lives, and CI
 * keeps it that way by refusing a tree that has `src/extensions` in it at all.
 */
const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
    // Rust build artifacts and Tauri generated schemas, for both applications.
    // The phone's build directory holds JavaScript too — the API script Tauri
    // generates for its webview — and it is not ours to lint.
    "src-tauri/target/**",
    "src-tauri/gen/**",
    "src-mobile/target/**",
    "src-mobile/gen/**",
    // Declarations emitted for the API surface report. Generated, not written.
    "temp/**",
  ]),
]);

export default eslintConfig;
