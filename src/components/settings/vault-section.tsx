"use client";

import { useCallback, useEffect, useState } from "react";
import { Minus, Pencil, Plus, X } from "lucide-react";

import { PanelFooter } from "@/components/shell/panel";
import { VaultRemovalSheet } from "@/components/settings/vault-removal";
import { VaultSheet } from "@/components/settings/vault-sheet";
import { Button } from "@/components/ui/button";
import { showNativeContextMenu } from "@/lib/native-menu";
import { cn } from "@/lib/utils";
import {
  forgetSecret,
  loadVaultEntries,
  vaultPersistence,
  writeSecret,
  type Persistence,
  type VaultEntry,
} from "@/lib/settings/vault";

/**
 * The secrets this Mac holds, and none of their values.
 *
 * A row says whose entry it is and what it is called, and that is the whole of
 * what crosses to this section: nothing it can call answers with a value, so it
 * could not show one if it decided to.
 *
 * **It is a list with a bar under it, and nothing else.** The gesture that adds
 * to a list sits beside the list — the shell's own rule, and the one macOS
 * keeps under every list in Settings. What that gesture *asks* is a sheet: a
 * pane standing open with three fields in it is a web page in a window, and the
 * pane's job is to show what is held rather than to be a form. Nothing here
 * restates the section's name or its headline either, because the window
 * already printed both above this.
 *
 * **The keychain is the only record of what exists.** Nothing beside it lists
 * what Sync stored, so the list is a search rather than a read of our own: an
 * entry deleted in Keychain Access is simply absent next time, with nothing to
 * reconcile. It is also why no row says when its entry was written — the store
 * answers with the entry and not with a history, and a date kept here would be
 * a second truth that disagrees with the first the moment anything happens
 * outside this window.
 *
 * Two facts about somebody else's system are asked rather than assumed: how
 * long the store holds an entry, and whether it can be reached at all. A person
 * who was asked for their login password and said no has not got an empty
 * vault, and drawing them one would be this window reporting their own refusal
 * back to them as a fact about their secrets.
 */
export function VaultSection() {
  // `null` until the keychain has answered. An empty list and an unread one are
  // different claims and only one of them means "nothing is stored".
  const [entries, setEntries] = useState<readonly VaultEntry[] | null>(null);
  const [persistence, setPersistence] = useState<Persistence | null>(null);
  const [storage, setStorage] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const [selected, setSelected] = useState<string | null>(null);
  const [storing, setStoring] = useState(false);
  const [replacing, setReplacing] = useState<VaultEntry | null>(null);
  const [forgetting, setForgetting] = useState<VaultEntry | null>(null);

  // A counter rather than a boolean, for the reason the agents list uses one: a
  // read that started before a write finished would otherwise put its rows back.
  const [reading, setReading] = useState(0);
  const refresh = useCallback(() => setReading((count) => count + 1), []);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const held = await loadVaultEntries();
        if (live) {
          setEntries(held);
          setFailure(null);
        }
      } catch (error: unknown) {
        // Not an empty list: the refusal is the thing to read.
        if (live) {
          setEntries(null);
          setFailure(explain(error));
        }
      }
    })();
    return () => {
      live = false;
    };
  }, [reading]);

  useEffect(() => {
    void vaultPersistence().then(setPersistence, (error: unknown) =>
      setStorage(explain(error)),
    );
  }, []);

  const held = entries ?? [];
  const chosen = held.find((entry) => keyOf(entry) === selected) ?? null;

  const store = useCallback(
    async (owner: string, name: string, secret: string) => {
      await writeSecret(owner, name, secret);
      setSelected(`${owner}/${name}`);
      refresh();
    },
    [refresh],
  );

  const forget = useCallback(
    async (entry: VaultEntry) => {
      await forgetSecret(entry.owner, entry.name);
      setSelected(null);
      refresh();
    },
    [refresh],
  );

  // The same two commands the bar carries. A menu under the pointer is
  // invisible to the keyboard, so neither of them lives only here.
  const menuFor = (entry: VaultEntry) => [
    { label: "Replace Secret", onSelect: () => setReplacing(entry) },
    "separator" as const,
    { label: "Forget Secret", onSelect: () => setForgetting(entry) },
  ];

  return (
    <section className="flex flex-col gap-3">
      <p className="max-w-[64ch] text-sm text-fg-tertiary">
        {storage ?? DURABILITY[persistence ?? "unknown"]}
      </p>

      <div className="overflow-hidden rounded-(--radius-control) border border-separator-strong bg-panel">
        {entries === null ? (
          <Nothing>
            The keychain has not been read, so what is in it is unknown. That is
            not the same as holding nothing.
          </Nothing>
        ) : held.length === 0 ? (
          <Nothing>
            No secrets on this Mac. A package that reaches a service in your
            name asks for one by the name in its own documentation.
          </Nothing>
        ) : (
          <ul className="max-h-72 overflow-y-auto py-1">
            {held.map((entry) => {
              const key = keyOf(entry);
              const isSelected = key === selected;
              return (
                <li key={key}>
                  {/* Selection is a surface shift and a weight change. No fill,
                      no marker: the row is still the obvious one in greyscale. */}
                  <button
                    type="button"
                    aria-pressed={isSelected}
                    onClick={() => setSelected(key)}
                    onDoubleClick={() => setReplacing(entry)}
                    onContextMenu={(event) => {
                      setSelected(key);
                      showNativeContextMenu(event, menuFor(entry));
                    }}
                    className={cn(
                      "flex w-full flex-col items-start gap-0.5 px-3 py-1.5 text-left transition-colors duration-(--motion-duration-fast) ease-shell",
                      isSelected
                        ? "bg-selected font-medium text-fg"
                        : "text-fg hover:bg-hover",
                    )}
                  >
                    <span className="w-full truncate text-sm">
                      {entry.name}
                    </span>
                    <span className="w-full truncate font-mono text-xs font-normal text-fg-tertiary">
                      {entry.owner}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}

        <PanelFooter>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Store a secret"
            onClick={() => setStoring(true)}
          >
            <Plus />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Forget the selected secret"
            disabled={chosen === null}
            onClick={() => chosen && setForgetting(chosen)}
          >
            <Minus />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Replace the selected secret"
            disabled={chosen === null}
            onClick={() => chosen && setReplacing(chosen)}
          >
            <Pencil />
          </Button>
        </PanelFooter>
      </div>

      {/* A command that did not happen says so, in the store's own words, and
          waits to be dismissed rather than fading on its own. */}
      {failure !== null && (
        <div className="flex max-w-[64ch] items-start gap-2 rounded-(--radius-control) border border-separator-strong bg-panel p-2.5">
          <p className="min-w-0 flex-1 text-sm text-warning">{failure}</p>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Dismiss"
            onClick={() => setFailure(null)}
          >
            <X />
          </Button>
        </div>
      )}

      <VaultSheet
        open={storing || replacing !== null}
        onOpenChange={(open) => {
          if (!open) {
            setStoring(false);
            setReplacing(null);
          }
        }}
        replacing={replacing}
        owners={[...new Set(held.map((entry) => entry.owner))]}
        onSubmit={store}
      />

      <VaultRemovalSheet
        open={forgetting !== null}
        onOpenChange={(open) => {
          if (!open) setForgetting(null);
        }}
        entry={forgetting}
        onForget={forget}
      />
    </section>
  );
}

/** What the box says instead of simulating rows it does not have. */
function Nothing({ children }: { children: React.ReactNode }) {
  return (
    <p className="px-3 py-6 text-center text-sm text-fg-tertiary">{children}</p>
  );
}

function keyOf(entry: VaultEntry): string {
  return `${entry.owner}/${entry.name}`;
}

/**
 * What the store promises, in words rather than in a name from its vocabulary.
 *
 * Every one of these is a correct implementation of the same trait, and the
 * difference between them is what somebody weighs before typing a token in.
 */
const DURABILITY: Record<Persistence, string> = {
  untilDeleted:
    "Kept until you take it out — here, or in Keychain Access.",
  untilLogout: "Lost when you log out of this Mac.",
  untilReboot: "Lost when this Mac restarts.",
  whileRunning:
    "Held only while something is running: nothing here survives on its own.",
  unknown: "This Mac's store did not say how long it keeps what it is given.",
};

/**
 * A refusal in the words it arrived in.
 *
 * The commands answer with a sentence written for a person — the store with
 * nowhere to keep a secret, the dialog nobody was there to answer — and a
 * sentence of our own would drop exactly the part somebody acts on.
 */
function explain(error: unknown): string {
  if (typeof error === "string" && error.trim() !== "") return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") return message;
  }
  return "The keychain could not be reached.";
}
