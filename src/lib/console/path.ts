/**
 * A path as a person writes it rather than as the system stores it.
 *
 * The home directory is where nearly every project lives, so spelling it out
 * spends a third of the line on the part that is the same for all of them.
 * Read from the path itself: the window is given a project's location and has
 * no other way to be told what home is.
 */
export function abbreviate(path: string): string {
  const home = /^(\/Users\/[^/]+|\/home\/[^/]+)(?=\/|$)/u.exec(path);
  return home === null ? path : `~${path.slice(home[0].length)}`;
}
