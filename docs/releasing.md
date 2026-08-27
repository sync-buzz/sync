# Releasing

## The version has one home

`src-tauri/tauri.conf.json` holds it. The bundler stamps that number into the
installer's name and into the bundle itself, and the updater in an installed
copy compares against exactly it — so it is the number, and `src-tauri/Cargo.toml`
and `package.json` are mirrors `scripts/release.sh` keeps in step. Nothing
reads the mirrors; they exist so that a person looking at either file is not
told something false.

## The two handles

A release happens in two acts, and they are separate on purpose.

| act | what it does | who sees it |
| --- | --- | --- |
| push the tag | builds the installers and publishes the GitHub release | anyone who goes looking for a download |
| push `updater/manifest.json` | the update manifest names the new version | every installed copy, on its next launch |

Between them a version is *built but not released*: the installers exist and
can be handed to somebody, and no machine in the field will take it on its own.
That gap is where a release is checked. Skipping it means the first person to
find a defect is a user who never chose to upgrade.

## Phase 1 — cut the release

```sh
scripts/release.sh 0.8.1            # writes the version, commits, tags
git push origin main v0.8.1         # or: scripts/release.sh 0.8.1 --push
```

The tag starts `.github/workflows/release.yml`, which builds and publishes one
GitHub release carrying:

```
Sync_macOS_aarch64.dmg
```

plus the signed update package and its `.sig` file, when the signing key is
provisioned.

**One platform, on purpose.** The window is drawn against macOS, and no Linux or
Windows build has been launched by anybody. Adding a row to the workflow matrix
and an entry to `PLATFORMS` in `scripts/release.sh` is the whole of putting a
platform back — the two lists have to move together, because the second is what
refuses to publish a manifest that is missing one.

The runner builds the sidecar before the bundler runs. `sync-mcp` links the
memory engine, so that step is a release build of LanceDB and llama.cpp and is
most of the wall-clock time — expect the release build to take considerably
longer than CI does.

The installer filenames carry no version. The README links to
`releases/latest/download/<name>`, which GitHub resolves to the newest release,
so those links never need editing; the version is carried by the tag and the
release title.

## Phase 2 — release it to installed copies

Once the build for the tag has finished:

```sh
scripts/release.sh 0.8.1 --publish-manifest        # assembles and commits
git push origin main                               # this is the moment it ships
```

It reads each platform's `.sig` back off the release, refuses if any platform's
package or signature is missing — a manifest with a platform absent is an update
path broken for that platform alone, which is the kind of thing nobody notices
for a month — and writes `updater/manifest.json`.

## `updater/latest.json` is not that file

A different product used to live at this address, and copies of it are still
installed on machines. They poll
`raw.githubusercontent.com/sync-buzz/sync/main/updater/latest.json` on every
launch, and that path now resolves to this repository.

`updater/latest.json` is therefore a tombstone, frozen at `0.6.13` — the last
version that product ever released through its own channel — with its download
URLs pointed at the archived repository so they still resolve. An old copy
asking gets a truthful "nothing newer" and stays where it is.

**Never write a current version into it.** The two products share no data
format, and the updater does not care: it would download the package, verify
it against a key those builds do not have — and fail, or worse, succeed if the
key were ever reused. This file is read-only history. New versions go to
`updater/manifest.json`, which nothing old has ever heard of.

## Signing the updates

Update packages are minisign-signed, and an installed copy verifies a download
against `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. This keypair
has nothing to do with the Apple certificate.

| secret (repository → Actions) | value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | the private key's **content** |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | only if the key was made with one. The workflow passes the variable even when it is empty — *unset*, the bundler prompts and hangs the runner |

`createUpdaterArtifacts` lives in its own config layer,
`src-tauri/tauri.updater.conf.json`, which the workflow applies **only when the
signing secret is present**. A fork, or a local `tauri build`, needs no key and
stays green; it simply produces no update packages.

**The key cannot be rotated casually.** Every copy in the field verifies
against the public half compiled into it, and will reject anything signed with
a different one. Rotating means every user downloads an installer by hand.

## Rolling back

Revert the manifest commit on `main` and push. That protects only the copies
that have not updated yet: the updater compares versions upward and never goes
back. A broken build that has already shipped is fixed by a new patch release
through both phases, never by a rollback.

## What an update does

- **macOS** — the `.app` is replaced from the signed `.app.tar.gz` while Sync
  keeps running. The new version starts on the next launch; the menu bar item
  offers "Restart to Update to …" for anybody who does not want to wait.
- **Development builds** never check, because `updates::in_the_background`
  returns early under `debug_assertions`. The plugin itself would happily
  check and install in a debug build — `tauri dev` runs a bare binary rather
  than a bundle, and letting it replace itself with one is not a state worth
  discovering.

## Trying the update cycle before shipping one

The endpoint is compiled into the bundle, so a real rehearsal needs a build
pointed somewhere you control:

1. Point `plugins.updater.endpoints` at `http://localhost:8000/manifest.json`
   and set `dangerousInsecureTransportProtocol` beside it — a release build
   refuses a plain-http endpoint outright, and this rehearsal has to be a
   release build. Then build a signed release at the current version with
   `TAURI_SIGNING_PRIVATE_KEY` (and `…_PASSWORD`) in the environment, and
   install it.
2. Raise the version locally and build again. That produces the "newer" update
   packages under `src-tauri/target/release/bundle/`.
3. Assemble a manifest against those and serve both:

   ```sh
   scripts/release.sh <new version> --publish-manifest \
     --dist <directory holding the packages and their .sig files> \
     --base-url http://localhost:8000 \
     --out /tmp/serve/manifest.json
   python3 -m http.server 8000 -d /tmp/serve
   ```

4. Launch the copy from step 1 and leave it alone. Within a few seconds of
   launch it should download the update and the menu bar item should grow
   "Restart to Update to …". Restart and check the version it comes back as.

Put the endpoint back, and take the insecure-transport flag out, before
committing anything.
