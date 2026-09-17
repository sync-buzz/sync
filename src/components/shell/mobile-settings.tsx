"use client";

import { useEffect, useState, type ReactNode } from "react";

import { Sheet } from "@/components/shell/mobile-sheet";
import type { ChannelStatus, Pairing } from "@/lib/pairing";
import { cn } from "@/lib/utils";

/**
 * Whether the computer is answering, in one line.
 *
 * Four states rather than two, and the two that are new are the two somebody
 * was left guessing at: a dial in flight read exactly like a dial that had
 * failed. *Not just now* is kept for the state it was always true of and is no
 * longer said for the other three.
 */
function answering(computer: ChannelStatus | null): string {
  if (computer === null) return "—";
  switch (computer.reach) {
    case "talking":
      return "Yes";
    case "reaching":
      return "Connecting…";
    case "away":
      return "Not just now";
    case "unpaired":
      return "—";
  }
}

/**
 * What this phone is, raised over whatever it is showing.
 *
 * Settings on a Mac are the installation's — what is true of this machine
 * whatever project is open — and they are a window of their own, reached from
 * the menu bar with a project open or not. A phone has one window and no menu
 * bar, so the same thing is a sheet raised from the two screens that belong to
 * the window rather than to a package: the list of a computer's projects, and
 * the list of a project's sections. Raised rather than pushed, because it is
 * not a place inside anything on the screen underneath — it is what the screen
 * underneath is being shown *by*.
 *
 * **One section, and it is the one only a phone can answer.** The Mac's
 * Settings decides who may reach *in*; this says what this phone reaches *out*
 * to, which nothing on the computer can tell somebody holding the phone. It is
 * also the half of pairing that had nowhere to happen: a phone could be given a
 * computer and never taken off one, and a device that cannot be unpaired from
 * its own side is a device somebody would have to find the computer to fix.
 *
 * The rest of that window — how the interface is painted, how a record's text
 * is set — is about this phone too, and it is not here. Nothing about it was
 * decided against: this is the section that had no other home, and the others
 * have one.
 */
export function SettingsSheet({
  open,
  pairing,
  onClose,
}: {
  open: boolean;
  pairing: Pairing;
  onClose: () => void;
}) {
  const [confirming, setConfirming] = useState(false);

  // A sheet that was closed half way through asking opens on the question
  // again, and the answer to a question nobody has been asked yet is no. Read
  // during the render that opens it rather than in an effect after it — the
  // way this window reads anything that has to be true before a frame is
  // drawn — because an effect would show the confirmation for one frame.
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) setConfirming(false);
  }

  // Asked when it is opened, though the hook is also told as this changes.
  // The two are not the same question: the telling covers a connection that
  // drops while somebody is looking at this, and this covers a sheet opened
  // after a stretch in which nothing changed and so nothing was said.
  const { refresh } = pairing;
  useEffect(() => {
    if (open) void refresh();
  }, [open, refresh]);

  const computer = pairing.computer;

  return (
    <Sheet open={open} title="Settings" rest="large" onClose={onClose}>
      <div
        className="absolute inset-0 overflow-y-auto overscroll-contain"
        // The home indicator's own space, kept clear by the scroller rather
        // than by a band under it: there is no band here, and a list that ended
        // exactly at the gesture reads as a list that was cut off.
        style={{ paddingBottom: "max(20px, var(--safe-bottom))" }}
      >
        <Group title="Computer">
          {/* The address rather than a name. What this phone was given is
              somewhere to dial; a name would have to be asked of the computer,
              and the one moment it is most worth reading this is the moment the
              computer is not answering. */}
          <Fact label="Address" value={computer?.endpoint ?? "—"} mono />
          {/* The state and, where there is one, the reason for it. Two rows
              rather than a sentence in one, because they are read at different
              moments: the first is glanced at, and the second is only ever
              looked at by somebody who has just read the first and wants to
              know why. The reason's row is absent when there is none — an
              empty field beside a computer that is answering reads as a fact
              that could not be fetched. */}
          <Fact label="Answering" value={answering(computer)} />
          {computer?.trouble ? (
            <Fact label="Because" value={computer.trouble} wrap />
          ) : null}
        </Group>

        <p className="px-4 pt-2 pb-4 text-[13px] leading-[18px] text-fg-tertiary">
          Sync draws here and that computer answers. Everything this phone shows
          — every project, every section, every record — is read from it.
        </p>

        {confirming ? (
          <div className="border-t border-separator px-4 py-3">
            <p className="pb-3 text-[15px] leading-[20px] text-fg-secondary">
              This phone will forget the key it was given. Reaching that
              computer again means pairing again, from its Settings.
            </p>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => setConfirming(false)}
                className="h-11 flex-1 rounded-lg bg-selected text-[17px] leading-[22px] text-fg active:opacity-70"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={pairing.isBusy}
                onClick={() => void pairing.forget()}
                // The window's own destructive treatment — a tinted surface
                // and the colour in the text, never a filled red slab. Colour
                // is reserved here for status and destruction, and a solid one
                // spends the loudest thing the palette has on a confirmation.
                className="h-11 flex-1 rounded-lg bg-destructive/10 text-[17px] leading-[22px] font-semibold text-danger active:opacity-70 disabled:opacity-50"
              >
                {pairing.isBusy ? "Forgetting…" : "Forget"}
              </button>
            </div>
          </div>
        ) : (
          <button
            type="button"
            onClick={() => setConfirming(true)}
            className="flex min-h-11 w-full items-center border-t border-b border-separator px-4 py-2 text-[17px] leading-[22px] text-danger active:bg-hover"
          >
            Forget this computer
          </button>
        )}

        {/* Whatever refused, in its own words — the rule the pairing screen
            keeps, and the same reason: a sentence composed here would be this
            window answering for a computer it cannot see. */}
        {pairing.failure ? (
          <p className="px-4 py-3 text-[15px] leading-[20px] text-warning">
            {pairing.failure}
          </p>
        ) : null}
      </div>
    </Sheet>
  );
}

/** A heading and the rows under it, the way the system groups a settings list. */
function Group({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <>
      <h2 className="px-4 pt-4 pb-1 text-[13px] leading-[18px] text-fg-tertiary uppercase">
        {title}
      </h2>
      <div className="border-t border-b border-separator">{children}</div>
    </>
  );
}

/**
 * Something true, said rather than offered.
 *
 * Not `Row`: a row is a button, and a button that does nothing when it is
 * pressed teaches a person that this screen is unresponsive. What is here is
 * read, so it is drawn as text at the height of a row and nothing else.
 */
function Fact({
  label,
  value,
  mono,
  wrap,
}: {
  label: string;
  value: string;
  mono?: boolean;
  /**
   * A sentence rather than a value, so it runs onto as many lines as it needs.
   *
   * Every other row here holds something short enough to be read at a glance
   * and is cut off rather than allowed to push the label around. A refusal is
   * not one of those: the half of it past the truncation is usually the half
   * that says what to do.
   */
  wrap?: boolean;
}) {
  return (
    <div className="flex min-h-11 items-center gap-4 px-4 py-2">
      <span className="shrink-0 text-[17px] leading-[22px]">{label}</span>
      <span
        className={cn(
          "min-w-0 flex-1 text-right text-fg-secondary",
          mono
            ? "font-mono text-[15px] leading-[20px]"
            : "text-[17px] leading-[22px]",
          wrap ? "text-[15px] leading-[20px]" : "truncate",
        )}
      >
        {value}
      </span>
    </div>
  );
}
