import type { LucideIcon } from "lucide-react";
import * as lucide from "lucide-react";
import {
  CircleAlert,
  CircleCheck,
  CircleDashed,
  CircleX,
  Shapes,
} from "lucide-react";
import type { Freshness } from "@/lib/memory/types";
import { cn } from "@/lib/utils";

/**
 * How a typed claim shows what it is and how far it can be trusted.
 *
 * This is the one place in the shell where a visual language is spent on the
 * product's own subject rather than on its furniture. The store's advantage is
 * that context is typed and that every record carries a freshness state, so
 * those two facts are given a mark instead of being spelled out in an
 * eleven-pixel word that loses to the rest of the row.
 *
 * Both marks are readable without colour, which is the rule the shell is held
 * to: the kind is carried by the shape of its icon, the state by the shape of
 * its ring — solid, dashed, alerted, crossed — and by the word that always
 * accompanies it. Colour only reinforces what the shape and the word say, and
 * only the two states that mean "this stopped matching the code" also take
 * weight.
 *
 * Neither mark is keyed to a list of kinds held here. The types come from the
 * project's own memory, and each one arrives naming the icon the build that
 * published it assigned — see `EntityKind::icon` in `crates/sync-memory`. This
 * module only resolves that name to a drawing, so a project holding a type this
 * build never published is listed with a neutral mark rather than hidden.
 */

/**
 * A mark name, and the drawing it resolves to.
 *
 * The vocabulary is Lucide's, whole: a name is converted to the library's own
 * spelling and looked up. It used to be a table of thirty-three names written
 * here, which made every other name resolve to the neutral mark with no error
 * on either side — the package author saw a green build, the window saw a
 * string it could not draw and had decided, by design, not to guess.
 *
 * The name is checked before it is used as a key. It arrives from a manifest
 * this application did not write, and it is about to select something that
 * gets rendered as a component; `icon` is refused because the library exports
 * that name for the generic component that takes a drawing as a prop.
 */
const MARK_NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

function markComponent(name: string): LucideIcon | null {
  if (name === "icon" || !MARK_NAME.test(name)) return null;

  const spelling = name.replace(/(^|-)([a-z0-9])/g, (_match, _dash, first: string) =>
    first.toUpperCase(),
  );
  const found = (lucide as unknown as Record<string, LucideIcon | undefined>)[
    spelling
  ];

  return found ?? null;
}

/**
 * The marks the type sheet offers, in the order they are worth reading.
 *
 * A shortlist rather than the library: this is what a person is shown while
 * naming a kind of their own, and two thousand tiles is a catalogue to be
 * searched, not a vocabulary to be chosen from. Every name here resolves like
 * any other — the list decides what is offered, never what can be drawn.
 */
export const MARK_CHOICES: readonly string[] = [
  "folder-git-2",
  "target",
  "flag",
  "ruler",
  "signpost",
  "lock",
  "eye",
  "circle-help",
  "circle-dot",
  "circle-check",
  "package",
  "file-text",
  "message-square",
  "lightbulb",
  "flask-conical",
  "bug",
  "shield",
  "book-open",
  "book-marked",
  "clipboard-list",
  "list-checks",
  "alarm-clock",
  "braces",
  "git-branch",
  "users",
  "calendar",
  "link",
  "star",
  "zap",
  "compass",
  "scale",
  "wrench",
  "shapes",
];

/** The mark a type gets when nobody has chosen one. */
export const DEFAULT_ICON = "shapes";

/**
 * The bare glyph, for places too narrow to carry the full mark. The navigator
 * lists the kinds themselves, so it uses this and stays one language with the
 * rows it filters.
 */
export function kindIcon(icon: string | null | undefined): LucideIcon {
  return (icon && markComponent(icon)) || Shapes;
}

/**
 * The bare glyph as a component, for a row that draws its own surround.
 *
 * The same lookup as [`kindIcon`] and a different shape, because the shape
 * matters: a function that answers with a component type has to be called from
 * somewhere, and calling it in the body of a component makes a new component
 * identity on every render — which React reads as a different element type and
 * rebuilds beneath. This is one component whose *props* change instead.
 */
export function KindGlyph({
  icon,
  className,
}: {
  icon: string | null | undefined;
  className?: string;
}) {
  // A lookup, not a factory: one name always answers with the same module
  // export, so the identity the rule below guards is stable already.
  const Icon = (icon && markComponent(icon)) || Shapes;
  // eslint-disable-next-line react-hooks/static-components
  return <Icon aria-hidden="true" className={className} />;
}

/**
 * The kind, as a glyph. It is decorative in the accessibility sense — every
 * place it appears also names the kind in text — so it is hidden from assistive
 * technology rather than read out twice.
 */
export function KindMark({
  icon,
  className,
}: {
  icon: string | null | undefined;
  className?: string;
}) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "flex size-6 shrink-0 items-center justify-center rounded-(--radius-control) bg-hover text-fg-secondary",
        className,
      )}
    >
      <KindGlyph icon={icon} className="size-3.5" />
    </span>
  );
}

/** The states the engine reports, in the order they are worth reading. */
export const FRESHNESS_STATES = [
  "fresh",
  "unverified",
  "stale",
  "invalid",
] as const satisfies readonly Freshness[];

const STATE_ICON: Record<string, LucideIcon> = {
  fresh: CircleCheck,
  unverified: CircleDashed,
  stale: CircleAlert,
  invalid: CircleX,
};

const STATE_TONE: Record<string, string> = {
  fresh: "text-success",
  unverified: "text-fg-tertiary",
  stale: "font-medium text-warning",
  invalid: "font-medium text-danger",
};

/**
 * The freshness state, as a mark and the engine's own word.
 *
 * A state this build has no mark for is shown as it arrived, with the neutral
 * ring: a newer engine naming a state we cannot draw is not a reason to claim
 * the record is in one we can.
 */
export function StateMark({
  freshness,
  className,
}: {
  freshness: Freshness;
  className?: string;
}) {
  const Icon = STATE_ICON[freshness] ?? CircleDashed;

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 text-xs",
        STATE_TONE[freshness] ?? "text-fg-tertiary",
        className,
      )}
    >
      <Icon className="size-3.5 shrink-0" aria-hidden="true" />
      {freshness}
    </span>
  );
}
