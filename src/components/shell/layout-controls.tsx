"use client";

import { Columns3, PanelLeft, PanelRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type {
  CollapsedPanels,
  CollapsiblePanelRole,
} from "@/lib/shell-layout";

const PANEL_TOGGLES = [
  { role: "primarySidebar", label: "sidebar", icon: PanelLeft },
  { role: "contextNavigator", label: "navigator", icon: Columns3 },
  { role: "contextInspector", label: "context panel", icon: PanelRight },
] as const satisfies readonly {
  role: CollapsiblePanelRole;
  label: string;
  icon: typeof PanelLeft;
}[];

/**
 * View controls for the shell.
 *
 * Each toggle is a real button with an accessible name and a tooltip, and
 * reports its state through `aria-pressed` rather than through colour alone.
 * There is no Reset Layout button: double-clicking a panel edge already
 * restores that column to its default width, which is the native gesture and
 * costs the title bar nothing.
 */
export function LayoutControls({
  collapsed,
  canOpen,
  onTogglePanel,
}: {
  collapsed: CollapsedPanels;
  canOpen: Record<CollapsiblePanelRole, boolean>;
  onTogglePanel: (role: CollapsiblePanelRole) => void;
}) {
  return (
    <div className="flex items-center gap-0.5">
      {PANEL_TOGGLES.map(({ role, label, icon: Icon }) => {
        const isShown = !collapsed[role];
        const isBlocked = !isShown && !canOpen[role];
        const action = `${isShown ? "Hide" : "Show"} ${label}`;

        return (
          <Tooltip key={role}>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-pressed={isShown}
                aria-disabled={isBlocked || undefined}
                aria-label={action}
                onClick={() => onTogglePanel(role)}
                className="text-fg-secondary aria-disabled:opacity-40 aria-pressed:bg-selected aria-pressed:text-fg"
              >
                <Icon />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {isBlocked
                ? `The window is too narrow for the ${label}`
                : action}
            </TooltipContent>
          </Tooltip>
        );
      })}

    </div>
  );
}
