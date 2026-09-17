"use client";

import {
  APPEARANCES,
  TINTS,
  useAppearance,
  type Appearance,
  type Tint,
} from "@/lib/settings/appearance";
import { Segment, Setting } from "@/components/settings/shared";

/**
 * How the shell is painted.
 *
 * Two choices and no more. The appearance is the system's unless somebody holds
 * it light or dark, which is the order the options are in — the default first,
 * because it is the one a desktop application should be in. The base colour is
 * the hue every grey is mixed from, which is the whole of what separates the
 * palettes shadcn/ui publishes; nothing else about the design moves with it, so
 * there is nothing else to preview.
 *
 * Both apply as they are chosen. A settings window with an Apply button asks a
 * person to confirm something they can already see.
 */
export function AppearanceSection() {
  const { settings, set } = useAppearance();

  return (
    <section className="flex flex-col gap-5">
      <Setting
        label="Appearance"
        detail="Following the system is the default, and it changes with it."
      >
        <div role="radiogroup" aria-label="Appearance" className="flex gap-1">
          {APPEARANCES.map((option) => (
            <Segment
              key={option.id}
              label={option.label}
              isSelected={settings.appearance === option.id}
              onSelect={() => set({ appearance: option.id as Appearance })}
            />
          ))}
        </div>
      </Setting>

      <Setting
        label="Base colour"
        detail="The hue the greys are mixed from. Surfaces, text and separators move together; nothing else changes."
      >
        <div role="radiogroup" aria-label="Base colour" className="flex gap-1">
          {TINTS.map((option) => (
            <button
              key={option.id}
              type="button"
              role="radio"
              aria-checked={settings.tint === option.id}
              onClick={() => set({ tint: option.id as Tint })}
              className="flex h-(--control-height-lg) items-center gap-1.5 rounded-(--radius-control) border border-transparent px-2 text-sm text-fg-secondary transition-colors duration-(--motion-duration-fast) ease-shell hover:bg-hover hover:text-fg aria-checked:border-separator-strong aria-checked:bg-selected aria-checked:text-fg"
            >
              <span
                aria-hidden="true"
                data-tint={option.id}
                className="tint-swatch size-3 shrink-0 rounded-full"
              />
              {option.label}
            </button>
          ))}
        </div>
      </Setting>
    </section>
  );
}
