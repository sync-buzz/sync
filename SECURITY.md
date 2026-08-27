# Security Policy

## Reporting a Vulnerability

If you believe you've found a security issue in Sync, please report it privately
to **security@sync.buzz**.

Please include:

- A description of the issue and the impact you believe it has.
- Steps to reproduce, or a minimal proof-of-concept.
- The affected version (the release tag, or the commit SHA).
- Your platform and any relevant configuration.

Please do **not** open a public GitHub issue for a suspected security bug.
Public disclosure should happen only after a patched release is available.

## Supported Versions

Sync is pre-1.0. Only the latest release receives security updates. There is no
long-term support branch, and a fix ships as a new patch release rather than as
a backport.

## What the application reaches, and what it does not

Worth knowing before reading a report, because most of the surface a desktop
application usually has is absent here by construction:

- **The window is a static export.** There is no server in the bundle, no route
  that reads a request, and no Node.js runtime after packaging. Everything the
  interface can do, it does through a named Tauri command.
- **The network is Rust's, and the list of hosts is fixed in the binary.** The
  webview is granted no `connect-src`; a page cannot dial out. Extensions reach
  the network only through a command that reads their own manifest's declared
  hosts, checks every redirect against the same list, and offers no way to set a
  header — which is where a token would go.
- **The updater has no entry point from the window.** None of its commands is in
  any capability, so a record's body cannot ask this application to fetch a
  bundle and run it. What is downloaded is verified against a minisign public
  key compiled into the binary before it is installed.
- **A project's memory is in that project's Git repository**, under
  `refs/memory/*`. It travels wherever the repository travels — including to a
  remote somebody else can read. Treat it as you treat the code.
- **Extensions run in the window's own webview.** A package is code, an install
  is a decision, and the capabilities in its manifest are what that decision was
  about. Report an extension that reaches past what it declared as a bug in
  Sync, not in the extension.

## Recognising a build made from this source

`src-tauri/src/main.rs` carries a 64-character BLAKE3 hash as a watermark. The
pre-image is a string held privately by the project owner.

**It proves nothing about who compiled a given binary.** Anybody can copy those
64 characters into anything, and a copy of them means only that somebody copied
them. What it makes possible is the other direction: a string of that length
cannot be arrived at by accident, so finding it inside a binary nobody here
released is evidence that the binary was built from this source. The owner can
then disclose the pre-image, and anybody may check it:

```sh
printf '%s' '<disclosed pre-image>' | b3sum
```

The output must equal the value in the source tree, where the watermark is also
reachable with `strings` on a release build.

This is a way to *notice* a copy. What to do about one is the
[licence](./LICENSE)'s question, not this file's. To ask for the pre-image,
write to **owner@sync.buzz**.
