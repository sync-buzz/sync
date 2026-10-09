"use client";

import { useEffect, useRef, useState } from "react";

import { sessionBacklog } from "@/lib/agent-sessions/client";
import {
  EMPTY_TRANSCRIPT,
  foldTranscript,
  type Entry,
  type Transcript,
} from "@/lib/agent-sessions/transcript";
import { works } from "@/lib/console/works";
import { cn } from "@/lib/utils";

/**
 * The conversation behind a name, in the panel.
 *
 * **A name is enough to be shown one.** Saying who you are talking to is
 * asking to see what was said to them — waiting for a message to be sent
 * before anything appears would make the panel's memory start at this
 * keystroke, when the agent's does not.
 *
 * **Read once rather than watched.** A session is subscribed to by the thing
 * that is running work in it, and a second subscription from here would be a
 * second claim on one key: what stops watching takes the only watch with it.
 * So this reads the backlog — everything the session has said — and reads it
 * again when the panel's own work moves, which is the only thing that adds to
 * it while somebody is looking.
 *
 * **It is a short form and says so by what it leaves out.** Thinking out loud,
 * plans and pictures belong where a conversation is read in full; what is kept
 * here is what was said, what was run, and what went wrong.
 */
export function useConversation(
  project: string,
  /** Who the line is talking to, or `null` when it is talking to nobody. */
  name: string | null,
  /**
   * What makes this read again: the state of the panel's own work, as one
   * string.
   *
   * A value rather than a subscription, because the only thing that can add to
   * a conversation while this surface is up is a line typed into it — and that
   * line is already state this panel holds.
   */
  stamp: string,
): Transcript | null {
  const [held, setHeld] = useState<{
    readonly name: string;
    readonly transcript: Transcript | null;
  }>({ name: "", transcript: null });

  // Cleared during the render that changed the name rather than in an effect
  // after it: an effect would draw the last agent's conversation for a frame
  // under the name of the next one. The stamp is deliberately not part of this
  // — reading again because a line was taken must not blank what is on the
  // screen while the answer is on its way.
  const wanted = name ?? "";
  if (held.name !== wanted) setHeld({ name: wanted, transcript: null });

  useEffect(() => {
    if (name === null || name.trim() === "") return;
    let reading = true;

    void (async () => {
      const rows = await works(project);
      const found = rows.find(
        (row) => (row.title ?? "").trim().toLowerCase() === name.trim().toLowerCase(),
      );
      // No session under that name is an ordinary answer: it is a name nobody
      // has talked to yet, and what the panel shows then is nothing at all.
      if (found === undefined || !reading) return;

      const backlog = await sessionBacklog(found.key);
      if (!reading) return;
      setHeld({
        name: wanted,
        transcript: backlog.events.reduce(foldTranscript, EMPTY_TRANSCRIPT),
      });
    })().catch(() => {
      // A session the host will not read is a panel with no conversation in
      // it, which is what `null` already draws. The sentence belongs where
      // somebody asked the agent something, not under a name they typed.
    });

    return () => {
      reading = false;
    };
  }, [name, project, stamp, wanted]);

  return held.name === wanted ? held.transcript : null;
}

/** How much of a conversation the panel holds. */
const KEPT = 20;

export function AskConversation({ transcript }: { transcript: Transcript }) {
  const end = useRef<HTMLDivElement>(null);
  const shown = transcript.entries.slice(-KEPT);

  // The end of it, which is where somebody coming back to a conversation is
  // looking. Written rather than scrolled into view: the box is this file's
  // own, so there is nothing above it that a reveal could move instead.
  useEffect(() => {
    const element = end.current;
    if (element !== null) element.scrollTop = element.scrollHeight;
  }, [shown.length]);

  return (
    <div
      ref={end}
      className="max-h-72 overflow-y-auto border-t border-separator px-5 py-2 font-mono text-sm"
    >
      {shown.map((entry) => (
        <Said key={entry.id} entry={entry} />
      ))}
    </div>
  );
}

/**
 * One turn, in the tone its voice is drawn in everywhere in this application:
 * what a person said at full weight, what the agent answered beside it, and
 * what went wrong in the one colour this window keeps for it.
 */
function Said({ entry }: { entry: Entry }) {
  if (entry.voice === "tool") {
    return (
      <p className="flex items-baseline gap-2 text-fg-tertiary">
        <span aria-hidden>›</span>
        <span className="min-w-0 flex-1 truncate">{entry.title}</span>
        <span className="shrink-0 text-xs">{entry.status}</span>
      </p>
    );
  }

  if (entry.voice === "person" || entry.voice === "agent" || entry.voice === "trouble") {
    return (
      <p
        className={cn(
          "break-words whitespace-pre-wrap",
          entry.voice === "person"
            ? "text-fg"
            : entry.voice === "agent"
              ? "text-fg-secondary"
              : "text-danger",
        )}
      >
        {entry.text}
      </p>
    );
  }

  // Everything else is the conversation's own screen to draw: a plan, a
  // picture, a thought, an update this build has no reading for. A panel the
  // width of a sentence that tried to show them would be a worse copy of the
  // place they are read properly.
  return null;
}
