"use client";

/**
 * How a record's text is set.
 *
 * Numbers rather than sizes with names. "Medium" is the interface guessing on
 * behalf of somebody who already knows they read at seventeen pixels, and this
 * is a window people keep open for hours — the difference between comfortable
 * and nearly comfortable is worth a number.
 *
 * Every choice applies as it is made, and the preview beside them is not a
 * preview: it is the same `.prose-surface` a record is set on, so what it shows
 * is what a record will look like rather than an impression of it. A settings
 * window with an Apply button asks a person to confirm something they can
 * already see.
 *
 * `Reset` is here because four numbers are four ways to end up with a page you
 * cannot read and no memory of what it was.
 */

import { Check } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  DEFAULT_TYPOGRAPHY,
  LIMITS,
  PROSE_FAMILIES,
  useTypography,
  type ProseFamily,
} from "@/lib/settings/typography";
import { cn } from "@/lib/utils";

export function TypographySection() {
  const { settings, set, reset } = useTypography();
  const untouched =
    settings.size === DEFAULT_TYPOGRAPHY.size &&
    settings.family === DEFAULT_TYPOGRAPHY.family &&
    settings.measure === DEFAULT_TYPOGRAPHY.measure &&
    settings.leading === DEFAULT_TYPOGRAPHY.leading;

  return (
    <section className="flex flex-col gap-5">
      <Choice
        label="Size"
        detail="The base every other size in a record is measured from. Headings, quotes, code and tables move with it."
      >
        <Number
          label="Size"
          unit="px"
          value={settings.size}
          step={1}
          limits={LIMITS.size}
          onChange={(size) => set({ size })}
        />
      </Choice>

      <Choice
        label="Typeface"
        detail="Four the system guarantees. A free choice over every font installed would include ones with no Cyrillic and no italic, and a record set in one of those is a record somebody has to repair."
      >
        <div role="radiogroup" aria-label="Typeface" className="flex gap-1">
          {PROSE_FAMILIES.map((option) => (
            <button
              key={option.id}
              type="button"
              role="radio"
              aria-checked={settings.family === option.id}
              onClick={() => set({ family: option.id as ProseFamily })}
              style={{ fontFamily: option.stack }}
              className={cn(
                "flex h-(--control-height-lg) items-center gap-1.5 rounded-(--radius-control) border border-transparent px-2.5 text-sm transition-colors duration-(--motion-duration-fast) ease-shell",
                settings.family === option.id
                  ? "border-separator-strong bg-selected text-fg"
                  : "text-fg-secondary hover:bg-hover hover:text-fg",
              )}
            >
              {settings.family === option.id ? (
                <Check aria-hidden="true" className="size-3 shrink-0" />
              ) : null}
              {option.label}
            </button>
          ))}
        </div>
      </Choice>

      <Choice
        label="Column width"
        detail="How far a line runs before it wraps. Fixed in pixels, so making the text bigger shortens the line rather than widening the window."
      >
        <Number
          label="Column width"
          unit="px"
          value={settings.measure}
          step={20}
          limits={LIMITS.measure}
          onChange={(measure) => set({ measure })}
        />
      </Choice>

      <Choice
        label="Line spacing"
        detail="A multiple of the size. The gap between paragraphs is half a line, so it follows this rather than being set on its own."
      >
        <Number
          label="Line spacing"
          value={settings.leading}
          step={0.05}
          limits={LIMITS.leading}
          onChange={(leading) => set({ leading: Math.round(leading * 100) / 100 })}
        />
      </Choice>

      <div className="space-y-2">
        <div className="space-y-0.5">
          <h2 className="text-base font-medium text-fg">Preview</h2>
          <p className="max-w-[64ch] text-sm text-fg-tertiary">
            The surface a record is set on, not a picture of it.
          </p>
        </div>
        <div className="rounded-(--radius-control) border border-separator bg-workspace p-4">
          <div className="prose-surface prose-blocks">
            <h3 className="pt-0 text-[1.54em] leading-tight font-semibold text-fg">
              A heading, at the size it will be
            </h3>
            <p className="text-[1em] text-fg-secondary">
              A claim is prose, and prose is what the widest column in the
              window is for. This paragraph is set exactly as one in a record
              is — the same face, the same measure, the same line.
            </p>
            <p className="text-[1em] text-fg-secondary">
              A second paragraph, so the gap between them is visible too.
            </p>
          </div>
        </div>
      </div>

      <div>
        <Button variant="outline" size="sm" disabled={untouched} onClick={reset}>
          Reset to the design
        </Button>
      </div>
    </section>
  );
}

function Choice({
  label,
  detail,
  children,
}: {
  label: string;
  detail: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="space-y-0.5">
        <h2 className="text-base font-medium text-fg">{label}</h2>
        <p className="max-w-[64ch] text-sm text-fg-tertiary">{detail}</p>
      </div>
      {children}
    </div>
  );
}

/**
 * A number, typed or stepped, and never out of range.
 *
 * The bounds are the control's rather than a warning afterwards: a size of two
 * hundred is not a preference somebody has, it is a slip, and a settings window
 * that lets one through has made a record unreadable in the window it was meant
 * to improve.
 */
function Number({
  label,
  unit,
  value,
  step,
  limits,
  onChange,
}: {
  label: string;
  unit?: string;
  value: number;
  step: number;
  limits: { readonly min: number; readonly max: number };
  onChange: (value: number) => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <input
        type="number"
        aria-label={label}
        value={value}
        min={limits.min}
        max={limits.max}
        step={step}
        onChange={(event) => {
          const next = event.target.valueAsNumber;
          if (globalThis.Number.isFinite(next)) {
            onChange(Math.min(limits.max, Math.max(limits.min, next)));
          }
        }}
        className="h-(--control-height-lg) w-24 rounded-(--radius-control) border border-separator-strong bg-transparent px-2 text-sm text-fg outline-none focus-visible:border-focus"
      />
      {unit ? <span className="text-sm text-fg-tertiary">{unit}</span> : null}
      <span className="text-xs text-fg-tertiary">
        {limits.min}–{limits.max}
      </span>
    </div>
  );
}
