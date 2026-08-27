"use client";

/**
 * A picture arriving from outside the project, and where it lands.
 *
 * The gesture is not ours: `PlaceholderPlugin` catches a paste from the
 * clipboard and a file dropped on the text, checks the type and the size, and
 * puts a placeholder in the document with the file held beside it. What is left
 * for this component is the one thing no library can know — where the bytes
 * belong.
 *
 * They belong in the working tree, as a file, because that is what makes the
 * record the same document everywhere it is read. A picture embedded in the
 * Markdown as `data:` would show here and nowhere else: GitHub strips data URLs
 * from Markdown, so the diagram would be invisible in the pull request that
 * introduced it — and the body would carry a third more bytes than the picture
 * for the privilege.
 *
 * **The root of the storage the record is in**, and no folder invented for it.
 * Where a project keeps its pictures is the project's arrangement, and an
 * application that quietly created `assets/` would be making that arrangement
 * in somebody else's repository and in their diff.
 *
 * A record whose body is not a file has no storage, so there is nowhere to put
 * one — and this says so on the page rather than failing quietly, which is the
 * one place in the editor where there *is* somewhere to put a message.
 */

import { useEffect, useState } from "react";

import { PlaceholderPlugin } from "@platejs/media/react";
import { KEYS } from "platejs";
import {
  useEditorPlugin,
  useReadOnly,
  useSelected,
  type PlateElementProps,
} from "platejs/react";
import { PlateElement } from "platejs/react";

import { createFileDocument } from "@/lib/memory/client";
import { explain } from "@/lib/memory/use-corpus";
import { relativeLocator, useLinkOrigin, useRecordLinks } from "@/lib/record-link";

/**
 * What the file is called once it is in the repository.
 *
 * A dropped file arrives with the name somebody already gave it, and that name
 * is what the file is named after — `diagram.png` stays `diagram.png`, and
 * `My Diagram.png` becomes `my-diagram.png`, because the engine slugs a stem
 * the way it slugs any other. A picture from
 * the clipboard has no name; every browser invents the same one, so `image.png`
 * would fill a folder with `image-2.png`, `image-3.png` and tell nobody
 * anything. Those are named after the record they were pasted into, which is
 * the only thing known about them that is true.
 *
 * Collisions are the engine's to settle: it numbers what is taken.
 */
const NAMELESS = /^(image|clipboard|screenshot|untitled)\.[a-z0-9]+$/i;

function nameFor(file: File, title: string): string {
  const given = file.name.trim();
  if (given !== "" && !NAMELESS.test(given)) return given;

  const extension = given.includes(".") ? given.slice(given.lastIndexOf(".")) : ".png";
  const stem = title.trim() === "" ? "picture" : title.trim();
  return `${stem}${extension}`;
}

/** The bytes, as the base64 the command takes. */
function base64Of(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("The file could not be read."));
    reader.onload = () => {
      const result = typeof reader.result === "string" ? reader.result : "";
      // `readAsDataURL` gives `data:<media>;base64,<bytes>`, and only the bytes
      // travel: the media type is the engine's to derive from the file name,
      // and two answers to what a document is would be one too many.
      resolve(result.slice(result.indexOf(",") + 1));
    };
    reader.readAsDataURL(file);
  });
}

export function PictureDrop(props: PlateElementProps) {
  const { api, editor } = useEditorPlugin(PlaceholderPlugin);
  const links = useRecordLinks();
  const origin = useLinkOrigin();
  const readOnly = useReadOnly();
  const selected = useSelected({ suppressThrow: true });
  const [failure, setFailure] = useState<string | null>(null);

  const id = props.element.id as string | undefined;

  useEffect(() => {
    if (id === undefined || links === null || readOnly) return;
    const file = api.placeholder.getUploadingFile(id);
    if (file === undefined) return;
    // Claimed before a single byte is read, and claimed *here* rather than
    // after the write, because the plugin's own registry is the only thing that
    // survives this component being mounted twice — which React does on purpose
    // in development, and which wrote the picture into the folder twice before
    // this line existed. The second run finds nothing to claim and stops.
    api.placeholder.removeUploadingFile(id);

    void (async () => {
      if (origin === null || origin.locator === null) {
        throw new Error(
          "This record keeps its body in the corpus rather than in a file, so there is no folder to put a picture in.",
        );
      }

      const content = await base64Of(file);
      const created = await createFileDocument(
        links.project,
        origin.kind,
        nameFor(file, origin.title),
        content,
      );
      if (created.locator === null) {
        throw new Error("The picture was written and has no path to link to.");
      }

      // Written the way GitHub reads one, relative to the record it was pasted
      // into. The same path the reading view resolves, so the picture appears
      // without anything having to be told that it is new.
      const url = relativeLocator(origin.locator, created.locator);
      // Found by the id it carries rather than by the element this component
      // was handed. React mounts this twice on purpose in development, so the
      // element in hand may be from a render whose node object the editor has
      // since replaced — and `findPath` on a stale object answers "nowhere",
      // which looked exactly like the picture silently not arriving.
      const entry = editor.api.node({
        at: [],
        match: (node) => (node as { id?: string }).id === id,
      });
      if (entry === undefined) {
        throw new Error("The picture was written and its place in the text was gone.");
      }
      const path = entry[1];

      editor.tf.withoutNormalizing(() => {
        editor.tf.removeNodes({ at: path });
        editor.tf.insertNodes(
          {
            type: editor.getType(KEYS.img),
            url,
            caption: [{ text: "" }],
            children: [{ text: "" }],
          },
          { at: path },
        );
      });
    })().catch((error: unknown) => {
      // Reported to the console as well as to the page, and unconditionally.
      // Reporting only to a component that may already have been unmounted is
      // how a failure became a placeholder that said "saving" forever.
      console.error("The picture could not be saved into the project.", error);
      setFailure(explain(error));
    });

    // Once, for the file this placeholder was put here to carry.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  return (
    <PlateElement {...props} className="py-[0.5em]">
      <div
        contentEditable={false}
        className={
          failure === null
            ? "rounded-(--radius-control) bg-panel p-[0.75em] text-[0.85em] text-fg-tertiary"
            : "rounded-(--radius-control) bg-panel p-[0.75em] text-[0.85em] text-danger"
        }
        data-selected={selected}
      >
        {failure ?? "Saving the picture into the project…"}
      </div>
      {props.children}
    </PlateElement>
  );
}
