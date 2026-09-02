"use client";

import { useState } from "react";
import { Laptop } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { Pairing } from "@/lib/pairing";

/**
 * The window with no computer.
 *
 * It is the phone's version of the window with no project: the surface, empty,
 * with the one way out of it. The two are the same composition on purpose —
 * this is not a different product on a smaller screen, it is the same window
 * answering the question that comes first here and does not exist on a Mac.
 *
 * There is no header. The band above holds the project switcher and the state
 * of a project's memory, and a phone that cannot reach a computer has neither
 * — a toolbar drawn over an absence is furniture arranged around it.
 *
 * **The code is the way in, and the two strings are the fallback**, in that
 * order and for the reason the Mac's own sheet gives: a key is sixty-four
 * characters of hex, and a camera does in a second what retyping does badly.
 * The fallback is not decoration either — it is the only way to pair a
 * simulator, which has no camera at all.
 */
export function PairingScreen({ pairing }: { pairing: Pairing }) {
  const [byHand, setByHand] = useState(false);

  return (
    <div
      className="flex min-h-0 flex-1 flex-col items-center justify-center gap-5 overflow-y-auto bg-workspace px-8"
      // The head and foot a phone keeps for itself. Nothing on a Mac, and the
      // real inset on a device — which is why it is asked for rather than
      // measured. Padding rather than margin: the surface runs to the edges of
      // the screen and the *text* stops short of them, which is the whole
      // point of covering the safe area in the first place.
      style={{
        paddingTop: "max(0px, env(safe-area-inset-top))",
        paddingBottom: "max(var(--header-height), env(safe-area-inset-bottom))",
      }}
    >
      <span
        aria-hidden="true"
        className="flex size-11 items-center justify-center rounded-(--radius-surface) border border-separator-strong bg-panel text-fg-secondary"
      >
        <Laptop className="size-5" />
      </span>

      <div className="max-w-[42ch] space-y-1.5 text-center">
        <h1 className="text-lg font-medium text-fg">No computer is paired</h1>
        <p className="text-sm text-fg-secondary">
          Sync draws here and a computer answers. On that computer open
          Settings, then Remote Access, and pair this device — it shows a code
          for the camera, and the two strings under it for when the camera
          cannot reach the screen.
        </p>
      </div>

      {byHand ? (
        <ByHand pairing={pairing} onCamera={() => setByHand(false)} />
      ) : (
        <>
          <Button
            size="lg"
            onClick={() => void pairing.readCode()}
            disabled={pairing.isBusy}
            className="min-w-40"
          >
            {pairing.isBusy ? "Pairing…" : "Scan the Code"}
          </Button>

          {/* Outlined rather than quiet. A ghost control says what it is by
              answering a pointer, and there is no pointer here: on a screen
              that is only ever touched, the same treatment is a line of text
              that happens to work. */}
          <Button
            variant="outline"
            size="lg"
            onClick={() => setByHand(true)}
            disabled={pairing.isBusy}
            className="min-w-40"
          >
            Put it in by hand
          </Button>
        </>
      )}

      {pairing.cameraRefused ? (
        <div className="flex max-w-[42ch] flex-col items-center gap-2 text-center">
          <p className="text-sm text-warning">
            Sync cannot use the camera. The permission is turned off for this
            application, and only Settings can turn it back on.
          </p>
          <Button
            variant="outline"
            size="lg"
            onClick={() => void pairing.openCameraSettings()}
          >
            Open Settings
          </Button>
        </div>
      ) : null}

      {/* The refusal in the words of whoever refused: the door's phrase for a
          key it will not admit, the plugin's for a camera that failed. A
          friendlier sentence composed here would be this window answering for
          a computer it cannot see. */}
      {pairing.failure ? (
        <p className="max-w-[42ch] text-center text-sm text-warning">
          {pairing.failure}
        </p>
      ) : null}
    </div>
  );
}

/**
 * The two strings, exactly as the other screen shows them.
 *
 * They go over as two values and are made into one payload on the far side, by
 * the function the computer composed the code with. Assembling them here would
 * put the format in a second place, and two spellings of one format is a
 * pairing that works until somebody changes one of them.
 */
function ByHand({
  pairing,
  onCamera,
}: {
  pairing: Pairing;
  onCamera: () => void;
}) {
  const [endpoint, setEndpoint] = useState("");
  const [secret, setSecret] = useState("");
  const ready = endpoint.trim() !== "" && secret.trim() !== "";

  return (
    <div className="flex w-full max-w-[42ch] flex-col gap-3">
      <Field
        label="Computer"
        value={endpoint}
        onChange={setEndpoint}
        disabled={pairing.isBusy}
      />
      <Field
        label="Key"
        value={secret}
        onChange={setSecret}
        disabled={pairing.isBusy}
      />

      <div className="flex items-center justify-between gap-2">
        <Button
          variant="outline"
          size="lg"
          onClick={onCamera}
          disabled={pairing.isBusy}
        >
          Use the camera
        </Button>
        <Button
          size="lg"
          onClick={() => void pairing.pairByHand(endpoint, secret)}
          disabled={pairing.isBusy || !ready}
          className="min-w-28"
        >
          {pairing.isBusy ? "Pairing…" : "Pair"}
        </Button>
      </div>
    </div>
  );
}

/** One long hex string, typed or pasted rather than read. */
function Field({
  label,
  value,
  onChange,
  disabled,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  disabled: boolean;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs text-fg-secondary">{label}</span>
      <input
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
        // Nothing the platform does to ordinary prose is right for hex: a
        // capital letter at the start and a substituted quotation mark are
        // both a key that will be refused, and the person retyping it has no
        // way of seeing which character the phone changed.
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        className="h-8 rounded-(--radius-control) border border-separator-strong bg-panel px-2 font-mono text-sm text-fg outline-none focus-visible:border-accent"
      />
    </label>
  );
}
