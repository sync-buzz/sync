import type { LucideIcon } from "lucide-react";
import {
  BookOpen,
  Bug,
  Calendar,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  CircleDot,
  CircleHelp,
  CircleX,
  ClipboardList,
  Compass,
  Eye,
  FileText,
  Flag,
  FlaskConical,
  FolderGit2,
  GitBranch,
  Lightbulb,
  Link,
  ListChecks,
  Lock,
  MessageSquare,
  Package,
  Ruler,
  Scale,
  Shapes,
  Shield,
  Signpost,
  Star,
  Target,
  Users,
  Wrench,
  Zap,
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
 * The marks a type can be given, by name.
 *
 * This is the vocabulary of the picker as much as the lookup table for drawing:
 * a type carries the *name* of its mark, and a name this build cannot draw is
 * shown neutrally rather than guessed at. Adding to this list adds a choice; it
 * never changes what an existing type is drawn with.
 */
export const KIND_ICON: Record<string, LucideIcon> = {
  "folder-git-2": FolderGit2,
  target: Target,
  flag: Flag,
  ruler: Ruler,
  signpost: Signpost,
  lock: Lock,
  eye: Eye,
  "circle-help": CircleHelp,
  // Two names GitHub made ordinary, and a section that reads a tracker has no
  // other glyph for what is open and what was dealt with. Here rather than in
  // whatever package wants them: this map is the window's icon vocabulary, and
  // a type invented this morning draws from the same set as a section is.
  "circle-dot": CircleDot,
  "circle-check": CircleCheck,
  package: Package,
  "file-text": FileText,
  "message-square": MessageSquare,
  lightbulb: Lightbulb,
  "flask-conical": FlaskConical,
  bug: Bug,
  shield: Shield,
  "book-open": BookOpen,
  "clipboard-list": ClipboardList,
  "list-checks": ListChecks,
  "git-branch": GitBranch,
  users: Users,
  calendar: Calendar,
  link: Link,
  star: Star,
  zap: Zap,
  compass: Compass,
  scale: Scale,
  wrench: Wrench,
  shapes: Shapes,
};

/** The mark a type gets when nobody has chosen one. */
export const DEFAULT_ICON = "shapes";

/**
 * The bare glyph, for places too narrow to carry the full mark. The navigator
 * lists the kinds themselves, so it uses this and stays one language with the
 * rows it filters.
 */
export function kindIcon(icon: string | null | undefined): LucideIcon {
  return (icon && KIND_ICON[icon]) || Shapes;
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
  const Icon = (icon && KIND_ICON[icon]) || Shapes;
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
  const Icon = (icon && KIND_ICON[icon]) || Shapes;

  return (
    <span
      aria-hidden="true"
      className={cn(
        "flex size-6 shrink-0 items-center justify-center rounded-(--radius-control) bg-hover text-fg-secondary",
        className,
      )}
    >
      <Icon className="size-3.5" />
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
