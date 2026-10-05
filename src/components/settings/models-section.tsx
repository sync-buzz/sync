"use client";

import { useCallback, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { FIELD, messageOf, Setting } from "@/components/settings/shared";
import {
  assignModel,
  humanSize,
  installCatalogueModel,
  loadModels,
  removeModel,
  testModel,
  type ModelsStatus,
} from "@/lib/settings/models";
import { cn } from "@/lib/utils";

/**
 * Local auxiliary models: what this machine has downloaded, and which serves
 * which task.
 *
 * A model is a machine-level concern, so this is a settings section rather than
 * a project's marketplace. A model installed here is available to every
 * project's extensions; an extension asks by task (`text.classify`) and the
 * shell dispatches to whatever this machine holds for it.
 *
 * The catalogue is curated and bundled with the build; install-by-URL is the
 * path for anything not in it. Either way the bytes land in one place and are
 * shared by every project.
 */
export function ModelsSection() {
  const [status, setStatus] = useState<ModelsStatus | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [testPrompt, setTestPrompt] = useState("Say hello in one sentence.");
  const [testResult, setTestResult] = useState<string | null>(null);

  const settle = useCallback((answered: ModelsStatus) => {
    setStatus(answered);
  }, []);

  useEffect(() => {
    let live = true;
    void loadModels()
      .then((answered) => {
        if (live) settle(answered);
      })
      .catch((error: unknown) => {
        if (live) setFailure(messageOf(error, "Models could not be read."));
      });
    return () => {
      live = false;
    };
  }, [settle]);

  const remove = useCallback(
    (id: string) => {
      setBusy(true);
      setFailure(null);
      void removeModel(id)
        .then(settle, (error: unknown) =>
          setFailure(messageOf(error, "The model could not be removed.")),
        )
        .finally(() => setBusy(false));
    },
    [settle],
  );

  const assign = useCallback(
    (task: string, modelId: string | null) => {
      setBusy(true);
      setFailure(null);
      void assignModel({ task, modelId })
        .then(settle, (error: unknown) =>
          setFailure(messageOf(error, "The assignment could not be written.")),
        )
        .finally(() => setBusy(false));
    },
    [settle],
  );

  const downloadCatalogue = useCallback(
    (id: string) => {
      setBusy(true);
      setFailure(null);
      void installCatalogueModel({ id })
        .then(settle, (error: unknown) =>
          setFailure(messageOf(error, "The model could not be downloaded.")),
        )
        .finally(() => setBusy(false));
    },
    [settle],
  );

  const test = useCallback(() => {
    setBusy(true);
    setFailure(null);
    setTestResult(null);
    void testModel({ prompt: testPrompt })
      .then(
        (text) => setTestResult(text),
        (error: unknown) => setFailure(messageOf(error, "The test failed.")),
      )
      .finally(() => setBusy(false));
  }, [testPrompt]);

  if (status === null) {
    return (
      <p className="text-xs text-fg-tertiary">{failure ?? "Reading what this machine holds…"}</p>
    );
  }

  // Every task any installed model declares, in the order the catalogue's
  // purposes imply. Used to draw the assignment rows.
  const tasks = new Set<string>();
  for (const model of status.installed) {
    for (const task of model.purposes) tasks.add(task);
  }

  return (
    <section className="flex flex-col gap-5">
      <Setting
        label="Installed"
        detail="Models downloaded to this machine. Any project's extension may ask one through the model surface; the shell dispatches by task."
      >
        {status.installed.length === 0 ? (
          <p className="text-xs text-fg-tertiary">
            No model installed yet. Install one from a file, or from the catalogue below.
          </p>
        ) : (
          <ul className="flex flex-col gap-1">
            {status.installed.map((model) => (
              <li
                key={model.id}
                className="flex items-center justify-between gap-3 rounded-(--radius-control) border border-separator px-3 py-2"
              >
                <div className="min-w-0">
                  <p className="truncate text-sm text-fg">{model.name}</p>
                  <p className="text-xs text-fg-tertiary">
                    {model.runtime} · {humanSize(model.size)} · {model.purposes.join(", ")}
                  </p>
                </div>
                <Button variant="ghost" size="sm" disabled={busy} onClick={() => remove(model.id)}>
                  Remove
                </Button>
              </li>
            ))}
          </ul>
        )}
      </Setting>

      {tasks.size > 0 ? (
        <Setting
          label="Assignments"
          detail="Which model answers which task. Clear a row to let the shell auto-pick the first installed model that declares it."
        >
          <ul className="flex flex-col gap-1">
            {[...tasks].sort().map((task) => (
              <li key={task} className="flex items-center gap-2">
                <code className="min-w-[18ch] text-xs text-fg-tertiary">{task}</code>
                <select
                  aria-label={`Model for ${task}`}
                  disabled={busy}
                  value={status.assignments[task] ?? ""}
                  onChange={(event) => assign(task, event.target.value || null)}
                  className={cn(FIELD, "flex-1")}
                >
                  <option value="">Auto</option>
                  {status.installed
                    .filter((m) => m.purposes.includes(task))
                    .map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.name}
                      </option>
                    ))}
                </select>
              </li>
            ))}
          </ul>
        </Setting>
      ) : null}

      <Setting
        label="Test"
        detail="Run a completion through the full pipeline — the same path a handler's model.run takes. Verifies the sidecar starts, the model loads, and inference answers."
      >
        <div className="flex flex-wrap items-center gap-2">
          <input
            aria-label="Test prompt"
            value={testPrompt}
            disabled={busy || status.installed.length === 0}
            onChange={(event) => setTestPrompt(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") test();
            }}
            className={cn(FIELD, "w-full max-w-[42ch] flex-1")}
          />
          <Button
            variant="outline"
            size="sm"
            disabled={busy || status.installed.length === 0 || testPrompt.trim().length === 0}
            onClick={test}
          >
            Run
          </Button>
        </div>
        {testResult !== null ? <p className="max-w-[64ch] text-sm text-fg">{testResult}</p> : null}
      </Setting>

      <Setting
        label="Catalogue"
        detail="A curated list, bundled with this build. Entries this machine cannot run are shown disabled with the reason, not hidden — you may override."
      >
        {status.catalogue.length === 0 ? (
          <p className="text-xs text-fg-tertiary">
            The curated catalogue is empty in this build. Install by URL above.
          </p>
        ) : (
          <ul className="flex flex-col gap-1">
            {status.catalogue.map((entry) => (
              <li
                key={entry.id}
                className="flex items-center justify-between gap-3 rounded-(--radius-control) border border-separator px-3 py-2"
              >
                <div className="min-w-0">
                  <p className="truncate text-sm text-fg">{entry.name}</p>
                  <p className="text-xs text-fg-tertiary">
                    {entry.runtime} · {humanSize(entry.size)}
                    {entry.refusal ? ` · ${entry.refusal}` : ""}
                  </p>
                </div>
                {entry.installed ? (
                  <span className="text-xs text-fg-tertiary">Installed</span>
                ) : (
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={busy || !entry.eligible}
                    title={entry.refusal ?? undefined}
                    onClick={() => downloadCatalogue(entry.id)}
                  >
                    Download
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}
      </Setting>

      <p className="text-xs text-fg-tertiary">
        This machine: {status.capabilities.arch}, {status.capabilities.os},{" "}
        {status.capabilities.gpu}.
      </p>

      {failure ? <p className="text-sm text-danger">{failure}</p> : null}
    </section>
  );
}
