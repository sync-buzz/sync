// Refreshes the extension archives this application ships with.
//
// The recommended extensions are bundle resources, so a first launch with no
// network can still compose a project. Their code is not in this tree — they
// are built by the registry's CI and published as a release — so the archives
// have to be fetched, and fetching them is a thing somebody does deliberately
// before a release rather than something a build does behind their back.
//
//   pnpm extensions:seed
//
// Every artefact is checked against the sha256 the index names before it is
// written, and an archive here that the index no longer lists is removed: the
// directory says what the current registry says, or the run fails.
import { createHash } from "node:crypto";
import { mkdir, readdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const INDEX_URL =
  "https://raw.githubusercontent.com/sync-buzz/sync-extensions/main/registry.json";

const here = fileURLToPath(new URL(".", import.meta.url));
const into = join(here, "..", "src-tauri", "resources", "extensions");

/** Reads the published index, refusing a format this script does not know. */
async function index() {
  const answer = await fetch(INDEX_URL);
  if (!answer.ok) {
    throw new Error(`the registry answered ${answer.status} for its index`);
  }
  const parsed = await answer.json();
  if (parsed.formatVersion !== 1) {
    throw new Error(
      `the index is written in format ${parsed.formatVersion}, and this script reads 1`,
    );
  }
  return parsed.extensions ?? [];
}

/** Downloads one artefact, refusing bytes the index did not name. */
async function artefact({ id, version, artefact: { url, sha256, bytes } }) {
  const answer = await fetch(url);
  if (!answer.ok) {
    throw new Error(`${id} answered ${answer.status}`);
  }
  const body = Buffer.from(await answer.arrayBuffer());
  const found = createHash("sha256").update(body).digest("hex");
  if (found !== sha256) {
    throw new Error(`${id} is not the file the index named: ${sha256} vs ${found}`);
  }
  if (body.length !== bytes) {
    throw new Error(`${id} is ${body.length} bytes and the index says ${bytes}`);
  }
  return { name: `${id}-${version}.syncext`, body, found };
}

const listed = await index();
if (listed.length === 0) {
  throw new Error("the index lists nothing — refusing to empty the seeded set");
}

await mkdir(into, { recursive: true });
const written = await Promise.all(listed.map(artefact));

for (const { name, body, found } of written) {
  await writeFile(join(into, name), body);
  console.log(`${name}  ${body.length} bytes  ${found}`);
}

// Anything the index no longer names. A stale archive would go on being seeded
// into every machine that installs this build, under an id nobody publishes.
const keeping = new Set(written.map(({ name }) => name));
for (const found of await readdir(into)) {
  if (found.endsWith(".syncext") && !keeping.has(found)) {
    await rm(join(into, found));
    console.log(`removed ${found}, which the index no longer lists`);
  }
}
