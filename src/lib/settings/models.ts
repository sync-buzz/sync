/**
 * Local auxiliary models: what this machine has installed, and what an
 * extension may ask of them.
 *
 * The client half of `src-tauri/src/models.rs`. The shell decides everything
 * about a model — which is installed, which serves which task — in Rust, and
 * this is a typed read of its answers. A list built here would be a second
 * answer to a question the file system already answers, and it would look right
 * while disagreeing.
 *
 * A model is a machine-level concern, not a project's, which is why this lives
 * beside `model-choice.ts` and `voice.ts` rather than beside an extension's
 * settings: it is true of this machine whatever project is open.
 */

import { command } from "@/lib/command";

/** One model installed on this machine. */
export interface InstalledModel {
  readonly id: string;
  readonly name: string;
  readonly runtime: string;
  readonly purposes: readonly string[];
  readonly size: number;
  readonly sha256: string;
}

/** One curated catalogue entry, with the hardware refusal beside it. */
export interface CatalogueEntry {
  readonly id: string;
  readonly version: string;
  readonly name: string;
  readonly modality: string;
  readonly runtime: string;
  readonly purposes: readonly string[];
  readonly size: number;
  readonly minMemory: number;
  readonly requires: { readonly arch: readonly string[]; readonly gpu: string };
  readonly weights: { readonly url: string; readonly sha256: string; readonly format?: string };
  readonly source?: string;
  readonly license?: string;
  /** Whether this machine can run it. */
  readonly eligible: boolean;
  /** Why it cannot, in a sentence, or `null` when it can. */
  readonly refusal: string | null;
  /** Whether the bytes are already on this machine. */
  readonly installed: boolean;
}

/** What this build can run a model on. */
export interface ModelCapabilities {
  readonly arch: string;
  readonly os: string;
  readonly gpu: string;
}

/** The whole of what the settings page draws, in one answer. */
export interface ModelsStatus {
  readonly installed: readonly InstalledModel[];
  /** Which model serves which task, keyed by task string. */
  readonly assignments: Record<string, string>;
  readonly catalogue: readonly CatalogueEntry[];
  readonly capabilities: ModelCapabilities;
}

/** What `model_assign` writes. */
export type AssignRequest = { readonly task: string; readonly modelId: string | null };

/** What `model_install_catalogue` was asked to install. */
export type CatalogueInstallRequest = { readonly id: string };

/** What `model_test` runs. */
export type TestRequest = { readonly prompt: string };

/** The whole status, for the settings page. */
export function loadModels(): Promise<ModelsStatus> {
  return command<ModelsStatus>("models_status", {});
}

/** Install a model from the curated catalogue by its id. */
export function installCatalogueModel(request: CatalogueInstallRequest): Promise<ModelsStatus> {
  return command<ModelsStatus>("model_install_catalogue", { request: { id: request.id } });
}

/** Remove a model's registration. */
export function removeModel(id: string): Promise<ModelsStatus> {
  return command<ModelsStatus>("model_remove", { id });
}

/** Assign a task to a model, or clear the assignment. */
export function assignModel(request: AssignRequest): Promise<ModelsStatus> {
  return command<ModelsStatus>("model_assign", {
    request: { task: request.task, modelId: request.modelId },
  });
}

/** Run a test completion through the full pipeline. Returns the generated text. */
export function testModel(request: TestRequest): Promise<string> {
  return command<string>("model_test", { request: { prompt: request.prompt } });
}

/**
 * A size, in the words this window already uses for one elsewhere.
 *
 * Kept here rather than imported from a shared util because the only other
 * place a byte count is shown is the extensions area, which formats its own; a
 * shared helper would be one more thing to keep aligned for two call sites that
 * disagree about nothing yet.
 */
export function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}
