"use client";

import { useState } from "react";
import { FRAMES, type FrameId } from "@/lib/shell-frames";
import { cn } from "@/lib/utils";
import { MobilePhone } from "@/components/prototype/mobile-phone";
import { DEVICE } from "@/components/prototype/mobile-geometry";
import type { InspectorPresentation } from "@/lib/mobile-geometry";

/**
 * The prototype and the two switches it is looked at through.
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
 * `env(safe-area-inset-*)`, which is nothing here and the real inset when the
 * same page is opened on a device.
 */
export function MobileDesk() {
  const [frame, setFrame] = useState<FrameId>("browse");
  const [inspector, setInspector] = useState<InspectorPresentation>("push");

  const frames = Object.keys(FRAMES) as FrameId[];
  const showsInspector = FRAMES[frame].inspector;

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
          legend="Inspector"
          options={[
            { id: "push" as const, label: "pushed after the workspace" },
            { id: "sheet" as const, label: "raised over it" },
          ]}
          chosen={inspector}
          onChoose={setInspector}
          // A frame with no inspector has nothing to decide, and a control
          // that does nothing is worse than one that is not offered.
          disabled={!showsInspector}
        />
      </div>

      {/* The measurement being made. Its width is the claim — 390 points — and
          it gives way only to a window narrower than that, which is a phone
          already. The height gives way on a short window, so the page can be
          read on a laptop without the foot of the device being cut off. */}
      <div
        className="shrink overflow-clip border border-separator-strong shadow-(--shadow-content)"
        style={{
          width: `min(${DEVICE.width}px, 100%)`,
          height: `${DEVICE.height}px`,
          maxHeight: "100%",
        }}
      >
        <MobilePhone frame={frame} inspector={inspector} />
      </div>
    </div>
  );
}

/** One row of the harness: a legend and the values it is switched between. */
function Choice<T extends string>({
  legend,
  options,
  chosen,
  onChoose,
  disabled,
}: {
  legend: string;
  options: readonly { id: T; label: string }[];
  chosen: T;
  onChoose: (id: T) => void;
  disabled?: boolean;
}) {
  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-2",
        disabled && "opacity-40",
      )}
    >
      <span className="w-20 shrink-0 text-sm text-fg-secondary">{legend}</span>
      {options.map((option) => (
        <button
          key={option.id}
          type="button"
          disabled={disabled}
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
