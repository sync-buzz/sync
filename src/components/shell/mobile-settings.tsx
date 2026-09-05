"use client";

import { useEffect, useState, type ReactNode } from "react";

import { Sheet } from "@/components/shell/mobile-chrome";
import type { Pairing } from "@/lib/pairing";

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

  // Asked when it is opened rather than kept up to date. Whether the computer
  // is answering changes with a lid and a network, and the answer is only ever
  // read here — a subscription would be the whole application watching a fact
  // one screen shows.
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
        style={{ paddingBottom: "max(20px, env(safe-area-inset-bottom))" }}
      >
        <Group title="Computer">
          {/* The address rather than a name. What this phone was given is
              somewhere to dial; a name would have to be asked of the computer,
              and the one moment it is most worth reading this is the moment the
              computer is not answering. */}
          <Fact label="Address" value={computer?.endpoint ?? "—"} mono />
          <Fact
            label="Answering"
            value={
              computer === null
                ? "—"
                : computer.connected
                  ? "Yes"
                  : "Not just now"
            }
          />
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
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex min-h-11 items-center gap-4 px-4 py-2">
      <span className="shrink-0 text-[17px] leading-[22px]">{label}</span>
      <span
        className={
          mono
            ? "min-w-0 flex-1 truncate text-right font-mono text-[15px] leading-[20px] text-fg-secondary"
            : "min-w-0 flex-1 truncate text-right text-[17px] leading-[22px] text-fg-secondary"
        }
      >
        {value}
      </span>
    </div>
  );
}
