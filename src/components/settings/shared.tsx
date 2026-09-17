"use client";

import { Check } from "lucide-react";

import { cn } from "@/lib/utils";

/**
 * The shape every control in this window shares.
 *
 * Named once rather than copied into each section, because the copies had
 * already drifted — `disabled:opacity-50` in one, `bg-transparent` in another —
 * and a reader who noticed one wondered whether the differences were on
 * purpose. They were not.
 */
export const FIELD =
  "h-(--control-height-lg) rounded-(--radius-control) border border-separator-strong bg-workspace px-2 text-sm text-fg";

/**
 * One labelled decision in the column, with its control below.
 *
 * Every section in this window is built from these, and a `border-t` between
 * them is what separates one decision from the next. The first in a column
 * carries no border, so a section opens on its content rather than on a line;
 * a paragraph between two — a footnote, an error — breaks the run, and the
 * Setting after it takes a border again, which is the right thing: a decision
 * that follows a footnote is a new decision, not a continuation of one.
 */
export function Setting({
  label,
  detail,
  children,
}: {
  label: string;
  detail: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-2 border-t border-separator pt-5 first:border-t-0 first:pt-0">
      <div className="space-y-0.5">
        <h2 className="text-base font-medium text-fg">{label}</h2>
        <p className="max-w-[64ch] text-sm text-fg-tertiary">{detail}</p>
      </div>
      {children}
    </div>
  );
}

/**
 * A segmented radio button, as used throughout the window.
 *
 * The shape Appearance settled on, copied into Notifications, Voice and Server
 * until the copies were the majority of the code. The selected state is a
 * surface shift and a weight change — no fill, no marker beyond the check —
 * which is the rule the sidebar's selection follows too.
 */
export function Segment({
  label,
  isSelected,
  onSelect,
  disabled,
  title,
}: {
  label: string;
  isSelected: boolean;
  onSelect: () => void;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={isSelected}
      disabled={disabled}
      title={title}
      onClick={onSelect}
      className={cn(
        "flex h-(--control-height-lg) items-center gap-1.5 rounded-(--radius-control) border border-transparent px-2.5 text-sm transition-colors duration-(--motion-duration-fast) ease-shell disabled:opacity-50",
        isSelected
          ? "border-separator-strong bg-selected font-medium text-fg"
          : "text-fg-secondary hover:bg-hover hover:text-fg",
      )}
    >
      {isSelected ? <Check aria-hidden="true" className="size-3 shrink-0" /> : null}
      {label}
    </button>
  );
}

/**
 * A two-state segmented control: the same shape as a row of Segments, but for
 * On and Off.
 *
 * The window has no switch of its own, and a lone one built here would be the
 * only control in the application drawn that way. This is the segmented control
 * the rest of the window uses, with two segments.
 */
export function SegmentedToggle({
  isOn,
  onChange,
  label,
  disabled,
}: {
  isOn: boolean;
  onChange: (wanted: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <div role="radiogroup" aria-label={label} className="flex gap-1">
      <Segment
        label="Off"
        isSelected={!isOn}
        disabled={disabled}
        onSelect={() => onChange(false)}
      />
      <Segment label="On" isSelected={isOn} disabled={disabled} onSelect={() => onChange(true)} />
    </div>
  );
}

/**
 * A refusal in the words it arrived in.
 *
 * Every command in this window answers a refusal with a sentence written for a
 * person — the file that could not be written, the store that could not be
 * reached — and a sentence of our own would drop the part somebody acts on.
 * The fallback is the caller's, because *what went wrong* is the caller's to
 * name: the keychain and the network are different failures, and one default
 * sentence for both would say nothing about either.
 */
export function messageOf(error: unknown, fallback: string): string {
  if (typeof error === "string" && error.trim() !== "") return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") return message;
  }
  if (error instanceof Error) return error.message;
  return fallback;
}
