"use client";

import { Check, ChevronsUpDown, FolderOpen, Search, Settings } from "lucide-react";
import type { ProjectSetup } from "@/components/shell/project-setup";
import { SyncIndicator } from "@/components/shell/sync-indicator";
import { LayoutControls } from "@/components/shell/layout-controls";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { SyncStatus } from "@/lib/memory/use-sync-state";
import { openSettings } from "@/lib/settings/window";
import type {
  CollapsedPanels,
  CollapsiblePanelRole,
} from "@/lib/shell-layout";
import type { OpenProject, RecentProject } from "@/lib/project/types";

/**
 * Application chrome.
 *
 * The header is the top band of the slab, spanning it end to end above the
 * columns: one opaque surface, no seam, and no material of its own — the glass
 * in this window is the frame around the slab, never a strip inside it.
 *
 * It sits inside the native macOS title bar: the traffic lights are the real
 * ones and the bar is a drag region, so leading space is reserved for them
 * through the `--titlebar-inset` token, measured from the edge of the slab.
 * Anything interactive carries its own hit area and is therefore excluded from
 * dragging by Tauri.
 *
 * The leading side carries the project switcher and, beside it, whether that
 * project's memory is in step with its remote. Workspace, project and
 * synchronisation scope every column at once, so they belong to the window
 * rather than to any one of them — and putting them in the title bar costs no
 * vertical space, which is the point of a title bar you are already paying
 * for.
 *
 * **Writing a record is not here.** Sync is not a text editor, and the command
 * an application puts in its title bar is the one it exists for — composing, in
 * Mail and Notes. A claim is written far more often than a type is added and
 * far less often than the window is read, so the command belongs beside the
 * list it adds to rather than above every column at once.
 *
 * With no project open the header keeps its shape and loses what has nothing to
 * act on: there are no panels to collapse and nothing to search.
 */
export function AppHeader({
  project,
  setup,
  layout,
  onSearch,
  sync,
  onOpenSync,
}: {
  project: OpenProject | null;
  setup: ProjectSetup;
  /**
   * Whether this project's memory is in step with its remote, and the commands
   * that change it. Absent with no project open, which has no memory to be out
   * of step with.
   */
  sync?: SyncStatus;
  /** Open the sheet the indicator is the door to. */
  onOpenSync?: () => void;
  /**
   * Open the search palette. Absent with no project open, which is the state
   * that has nothing to search rather than a search that is switched off.
   */
  onSearch?: () => void;
  layout?: {
    collapsed: CollapsedPanels;
    canOpen: Record<CollapsiblePanelRole, boolean>;
    onTogglePanel: (role: CollapsiblePanelRole) => void;
  };
}) {
  return (
    <header
      data-tauri-drag-region
      className="relative flex h-(--header-height) shrink-0 items-center gap-2 border-b border-separator bg-sidebar pr-2 pl-(--titlebar-inset)"
    >
      <ProjectSwitcher project={project} setup={setup} />
      {project && sync && onOpenSync ? (
        <SyncIndicator sync={sync} onOpen={onOpenSync} />
      ) : null}

      <div data-tauri-drag-region className="min-w-4 flex-1 self-stretch" />

      {/* Centred on the window, not on the space left over, so it does not
          shift when a column is collapsed. */}
      {project && onSearch ? (
        <div className="pointer-events-none absolute inset-x-0 flex justify-center">
          <div className="pointer-events-auto">
            <SearchAffordance onSearch={onSearch} />
          </div>
        </div>
      ) : null}

      {layout ? (
        <LayoutControls
          collapsed={layout.collapsed}
          canOpen={layout.canOpen}
          onTogglePanel={layout.onTogglePanel}
        />
      ) : null}

      <SettingsControl />
    </header>
  );
}

/**
 * The way into settings that the keyboard is not.
 *
 * `⌘,` opens the same window, and it is what most people will use — but a
 * command reachable only from a shortcut is a command nobody discovers, and
 * this one is how an agent is connected to a project at all. It is in the
 * header rather than in the sidebar because settings belong to the
 * installation: they are true of every project, including none.
 *
 * Nothing about a project travels with it: one server answers for every
 * project on this machine, so even connecting an agent no longer names one.
 */
function SettingsControl() {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Settings"
          onClick={() => void openSettings()}
          className="text-fg-secondary"
        >
          <Settings />
        </Button>
      </TooltipTrigger>
      <TooltipContent>Settings</TooltipContent>
    </Tooltip>
  );
}

/**
 * What is open, what was open before, and how to open something else.
 *
 * The project is what you are in, so it is what the button says. The menu is
 * the list of projects this installation has opened, each under its own path —
 * the path is what tells two folders with the same name apart, which is the
 * only question a list like this ever has to answer.
 */
function ProjectSwitcher({
  project,
  setup,
}: {
  project: OpenProject | null;
  setup: ProjectSetup;
}) {
  const projects = listedProjects(project, setup.recent);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="min-w-0 gap-1.5 text-fg"
          aria-label="Project"
        >
          <span className="truncate font-medium">
            {project ? project.name : "No project"}
          </span>
          <ChevronsUpDown className="opacity-50" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-80">
        {projects.length > 0 ? (
          <>
            <DropdownMenuLabel>Projects</DropdownMenuLabel>
            {projects.map((entry) => {
              const isOpen = entry.path === project?.path;

              return (
                <DropdownMenuItem
                  key={entry.path}
                  disabled={isOpen}
                  onSelect={() => setup.open(entry.path)}
                  className="items-start gap-2 py-1.5"
                >
                  <Check
                    aria-hidden="true"
                    className={isOpen ? "mt-0.5" : "mt-0.5 invisible"}
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate">{entry.name}</span>
                    <span
                      className="block truncate font-mono text-xs text-fg-tertiary"
                      title={entry.path}
                    >
                      {entry.path}
                    </span>
                  </span>
                </DropdownMenuItem>
              );
            })}
            <DropdownMenuSeparator />
          </>
        ) : null}
        <DropdownMenuItem onSelect={setup.begin} disabled={setup.isBusy}>
          <FolderOpen />
          Open Folder
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * The open project belongs at the top of the list even on the first launch
 * that opened it, when the stored list has not been read back yet — the menu
 * must never fail to show what the button beside it is naming.
 */
function listedProjects(
  project: OpenProject | null,
  recent: readonly RecentProject[],
): readonly RecentProject[] {
  if (!project || recent.some((entry) => entry.path === project.path)) {
    return recent;
  }
  return [{ path: project.path, name: project.name }, ...recent];
}

/**
 * The way into search.
 *
 * A field to look at and a button to use: what it opens is a palette, and
 * typing here would mean two places to type the same question, one of which
 * would have to hand its keystrokes to the other. The shortcut is shown rather
 * than only bound, because a control that opens something else should say how
 * it is reached without the pointer.
 */
function SearchAffordance({ onSearch }: { onSearch: () => void }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={onSearch}
          className="flex h-(--control-height) w-80 items-center gap-2 rounded-(--radius-control) border border-separator-strong bg-raised/60 px-2 text-sm text-fg-tertiary transition-colors duration-(--motion-duration-fast) ease-shell hover:border-separator-strong hover:bg-raised hover:text-fg-secondary"
        >
          <Search className="size-3.5 shrink-0" />
          <span className="min-w-0 flex-1 truncate text-left">
            Search this project
          </span>
          <kbd className="shrink-0 font-sans text-xs text-fg-tertiary">
            &#8984;K
          </kbd>
        </button>
      </TooltipTrigger>
      <TooltipContent>Search this project</TooltipContent>
    </Tooltip>
  );
}
