"use client";

import { useState, type CSSProperties } from "react";
import { FRAMES, type FrameId } from "@/lib/shell-frames";
import { cn } from "@/lib/utils";
import { MobilePhone } from "@/components/prototype/mobile-phone";
import { DEVICE } from "@/components/prototype/mobile-geometry";

/**
 * The prototype and the switches it is looked at through.
 *
 * The frames are read out of `src/lib/shell-frames.ts` rather than listed here,
 * so this page cannot drift from the set the window has: a frame added there
 * appears here with no edit, and a frame this prototype cannot draw is a
 * failure that shows up rather than one that has to be remembered.
 *
 * The device is a plain box at 390 by 844 points — no bezel, no rounded
 * corners, no simulated status bar. It is a viewport, not a picture of a phone,
 * and a drawing of hardware around it would be the one part of this screen that
 * is decoration. The space at the head and foot of a screen is claimed with
 * `env(safe-area-inset-*)`, which is nothing here and the true inset when the
 * same page is opened on a device.
 *
 * **The box carries `data-device="phone"` itself**, which is what puts the
 * phone's own design inside it: `src/app/mobile.css` hangs every value on that
 * attribute, and custom properties are inherited, so a box wearing it is a box
 * where the whole appearance changes. The desk around it stays the desk, which
 * is the point of looking at them side by side.
 */
export function MobileDesk() {
  const [frame, setFrame] = useState<FrameId>("browse");
  const [appearance, setAppearance] = useState<"light" | "dark">("dark");
  const [waiting, setWaiting] = useState(false);
  const [insets, setInsets] = useState<"phone" | "none">("phone");

  const frames = Object.keys(FRAMES) as FrameId[];

  return (
    <div className="flex h-dvh flex-col items-center gap-4 overflow-auto bg-window p-4 text-fg">
      <div className="flex w-full max-w-[720px] shrink-0 flex-col gap-3">
        <Choice
          legend="Frame"
          options={frames.map((id) => ({ id, label: id }))}
          chosen={frame}
          onChoose={setFrame}
        />
        <Choice
          legend="Appearance"
          options={[
            { id: "dark" as const, label: "the field is off" },
            { id: "light" as const, label: "paper" },
          ]}
          chosen={appearance}
          onChoose={setAppearance}
        />
        <Choice
          legend="Hardware"
          options={[
            { id: "phone" as const, label: "notch and home indicator" },
            { id: "none" as const, label: "neither" },
          ]}
          chosen={insets}
          onChoose={setInsets}
        />
        <Choice
          legend="Waiting"
          options={[
            { id: false as const, label: "answered" },
            { id: true as const, label: "asking the computer" },
          ]}
          chosen={waiting}
          onChoose={setWaiting}
        />
      </div>

      {/* The measurement being made. Its width is the claim — 390 points — and
          it gives way only to a window narrower than that, which is a phone
          already. The height gives way on a short window, so the page can be
          read on a laptop without the foot of the device being cut off. */}
      <div
        data-device="phone"
        data-appearance={appearance}
        // A column, because that is what the window gives these screens: the
        // wheel and the project window both fill the space they are handed
        // rather than claiming a height of their own. A plain box here let the
        // wheel run off the end of the device and take its button with it.
        className="flex shrink flex-col overflow-clip border border-separator-strong"
        style={{
          width: `min(${DEVICE.width}px, 100%)`,
          height: `${DEVICE.height}px`,
          maxHeight: "100%",
          // What a phone claims at either edge, which `env()` reports as zero
          // in a browser. Without this the one thing that cannot be checked on
          // a desk is the one thing every arrangement here ends against.
          ...(insets === "phone" ? DEVICE.insets : { ...ZERO }),
        } as CSSProperties}
      >
        <MobilePhone frame={frame} waiting={waiting} />
      </div>
    </div>
  );
}

/** A device with nothing in the way, for comparison. */
const ZERO = { "--safe-top": "0px", "--safe-bottom": "0px" };

/** One row of the harness: a legend and the values it is switched between. */
function Choice<T extends string | boolean>({
  legend,
  options,
  chosen,
  onChoose,
}: {
  legend: string;
  options: readonly { id: T; label: string }[];
  chosen: T;
  onChoose: (id: T) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="w-20 shrink-0 text-sm text-fg-secondary">{legend}</span>
      {options.map((option) => (
        <button
          key={String(option.id)}
          type="button"
          onClick={() => onChoose(option.id)}
          className={cn(
            "h-(--control-height) rounded-(--radius-control) border border-separator-strong px-2 text-sm",
            option.id === chosen
              ? "bg-selected font-semibold"
              : "bg-raised hover:bg-hover",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
