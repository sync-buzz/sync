"use client";

/**
 * A picture in a record's body.
 *
 * `![alt](./diagram.png)` is Markdown, so a picture is content this store can
 * keep — which is the whole test every block in this editor has to pass. It
 * round-trips through the serialiser untouched, alt text included, and it did
 * so before this component existed: what was missing was not the block but a
 * drawing of it. A record holding a picture opened, saved correctly, and showed
 * a gap where the picture was.
 *
 * The bytes are the engine's answer to a path, never the disk's. The window has
 * no filesystem access, and a picture is the one thing in a body that would
 * most like to have some: `../../../.ssh/id_rsa` is a valid relative path, and
 * the reason it is harmless here is that this asks the corpus for a *document*
 * and the corpus only has the folders somebody attached.
 *
 * What it does not do is resize, caption or align. None of those are Markdown —
 * a width dragged in the window would be back where it started the next time
 * the record was opened, which is the same rule that decides everything else
 * this editor offers.
 */

import { useEffect, useState } from "react";

import { cn } from "@/lib/utils";
import { useLinkOrigin, useRecordLinks } from "@/lib/record-link";

/**
 * What a picture is while it is being read, and what it is when it cannot be.
 *
 * The absent state is drawn rather than left blank, and it names the path. A
 * body that says there is a diagram here and shows nothing is a body somebody
 * has to open a terminal to understand; the path is what tells them the file
 * was moved, or is on another branch, or was never committed.
 */
export function Picture({
  url,
  alt,
  className,
}: {
  url: string;
  alt: string;
  className?: string;
}) {
  const links = useRecordLinks();
  const base = useLinkOrigin()?.locator ?? null;
  const [source, setSource] = useState<string | null | undefined>(undefined);

  useEffect(() => {
    if (links === null) return;
    let current = true;
    void links.pictureFor(url, base).then((found) => {
      if (current) setSource(found);
    });
    return () => {
      current = false;
    };
  }, [links, url, base]);

  if (source === undefined) {
    // The space the picture will take is not known until it is read, so this
    // is a line rather than a box: a placeholder the size of a guess makes the
    // page jump when the guess turns out wrong.
    return (
      <span className={cn("block text-[0.85em] text-fg-tertiary", className)}>
        Reading {url}…
      </span>
    );
  }

  if (source === null) {
    return (
      <span
        className={cn(
          "block rounded-(--radius-control) bg-panel p-[0.75em] text-[0.85em] text-fg-tertiary",
          className,
        )}
      >
        {alt.trim() === "" ? "A picture" : alt} — {url} is not a document of
        this project, or not on this branch.
      </span>
    );
  }

  return (
    // `next/image` is not available to this application and never will be:
    // `output: "export"` means there is no Node runtime in the bundle and no
    // image optimizer behind it. The source is a `data:` URL that has already
    // been read, so there is nothing left to optimise anyway.
    // eslint-disable-next-line @next/next/no-img-element
    <img
      src={source}
      alt={alt}
      // Never wider than the measure, and never taller than most of a screen:
      // a photograph straight off a camera would otherwise be a page of its
      // own between two paragraphs.
      className={cn("h-auto max-h-[60vh] max-w-full rounded-(--radius-control)", className)}
    />
  );
}
