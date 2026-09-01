import { MobileDesk } from "@/components/prototype/mobile-desk";

/**
 * A prototype of the four frames at the width of a phone, kept beside the
 * window rather than inside it.
 *
 * It is a route of its own so that it can be opened in a browser at a stated
 * width and dragged with a pointer, which is the only way the transitions it
 * exists to show can be judged at all. Nothing in the window links to it and
 * nothing in it is imported by the window: the whole prototype is these two
 * folders — this one and `src/components/prototype/` — and removing them is
 * how it is thrown away.
 */
export default function Page() {
  return <MobileDesk />;
}
