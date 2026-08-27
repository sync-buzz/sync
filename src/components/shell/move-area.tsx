"use client";

import type { ReactNode } from "react";
import {
  DndContext,
  useDraggable,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";

/**
 * The region a thing can be dragged across.
 *
 * It wraps the columns rather than living inside one, because the drag that
 * matters crosses them: a record is dragged from the list in the workspace onto
 * a folder in the navigator, and a source and a target in two different
 * contexts would never meet.
 *
 * What a drop *means* is not decided here. This reports the payload the dragged
 * thing carried and what the row it landed on says it is; whether that is a
 * move, a copy or a refusal belongs to whoever knows the domain.
 */
export function MoveArea({
  onDrop,
  children,
}: {
  /** Something was dropped. Both values are the caller's own. */
  onDrop: (target: unknown, payload: unknown) => void;
  children: ReactNode;
}) {
  // A drag has to start deliberately. Without a distance the first press on a
  // row would begin one, and a list whose rows lift when you click them is a
  // list you cannot click.
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
  );

  return (
    <DndContext
      sensors={sensors}
      onDragEnd={({ active, over }: DragEndEvent) => {
        if (!over) return;
        const payload = active.data.current?.payload;
        const target = over.data.current?.target;
        if (payload === undefined || target === undefined) return;
        onDrop(target, payload);
      }}
    >
      {children}
    </DndContext>
  );
}

/**
 * What makes one thing draggable, for a list that is not a tree.
 *
 * The tree wires its own rows; everything else — a row in a list, a card —
 * asks for this and spreads what it answers onto the element that should move.
 * `payload` is the caller's own and is handed back to `MoveArea`'s `onDrop`
 * untouched.
 */
export function useDragHandle(id: string, payload: unknown) {
  const { setNodeRef, listeners, attributes, isDragging } = useDraggable({
    id,
    data: { payload },
  });
  return {
    ref: setNodeRef,
    ...listeners,
    ...attributes,
    "data-dragging": isDragging,
  };
}
