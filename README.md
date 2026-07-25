[![status](https://img.shields.io/badge/status-alpha-red)](#-alpha)
[![license](https://img.shields.io/badge/license-Apache%202.0-green)](./LICENSE)
[![platform](https://img.shields.io/badge/platform-Windows-blue)](#build)
[![built with](https://img.shields.io/badge/built%20with-Rust%20%2B%20Svelte%205-orange)](#how-it-is-put-together)

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/mark-dark.svg">
    <img src="brand/mark-light.svg" width="170" alt="Goodtype">
  </picture>
</p>

<h1 align="center">Goodtype</h1>

<p align="center">A local-first technical notebook where handwriting and typesetting share the same page.</p>

Write with a pen, drop in a Typst block for the equation you do not want to draw by hand, and
place images or PDF material alongside both — on fixed pages that export as one PDF, with the ink
still vector and the text still selectable.

Goodtype keeps your notebooks as plain files on your own disk. No account, no sync, no updater,
no telemetry.

## ⚠️ Alpha

> [!WARNING]
> **Goodtype is in alpha and the notebook format is not stable.**
>
> The on-disk format changes when a change is worth making, and those changes are **not
> backwards compatible**: there are no migrations, and a notebook written by one build may not
> open in the next. Fields get added and made required rather than defaulted, because carrying
> compatibility shims this early would fix mistakes in place instead of correcting them.
>
> Do not keep anything you care about only in Goodtype. Expect to lose test notebooks between
> builds. Once the format settles, this will be replaced by a schema version and real migrations —
> until then, treat it as a prototype you write on, not a place you store work.

## What it does today

- **Pen input** with pressure, tilt, and palm rejection, on a canvas built for stylus latency.
- **Vector ink.** A stroke is stored as points and rendered as its silhouette, so pressure and nib
  character survive into the exported PDF instead of flattening to a constant width.
- **Typst blocks** compiled in-process, with completion driven by the same compiler that renders
  them — no `typst.exe`, no subprocess, no cold start.
- **Typst Universe packages**, resolved from a local cache first and downloaded only on a miss.
- **Images and PDF material** placed and scaled on the page.
- **Multi-page notebooks** that scroll continuously, with per-page undo and redo.
- **PDF export** of the whole notebook, in manifest order, from the files on disk rather than from
  whatever the screen happens to be showing.
- **Crash recovery** built on atomic writes and a revision check, so an interrupted save cannot
  leave a notebook it can no longer open.

## Local-first

A notebook is a directory of ordinary files — a JSON manifest, one file per page, Typst sources as
`.typ`, ink layers as JSON, and assets kept beside them. You can read them, diff them, and put them
in your own version control. Nothing about the format needs Goodtype to be running, or to exist.

The one outbound network path in the whole application is the Typst package downloader, it only
fires on a cache miss, and it can be switched off.

## Build

Goodtype currently targets Windows 10 and 11.

**You will need**

- [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) (already present on current Windows)
- [Rustup](https://rustup.rs/) — the pinned toolchain installs itself from `rust-toolchain.toml`
- [Node.js 25.2.1](https://nodejs.org/) and [pnpm 11.9.0](https://pnpm.io/installation)
- [Visual Studio Build Tools 2022](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with
  **Desktop development with C++** and a Windows 10 or 11 SDK

```powershell
git clone https://github.com/Just8it/Goodtype.git
cd Goodtype
pnpm install --frozen-lockfile
cargo xtask doctor    # checks your toolchain
cargo xtask verify    # fmt, clippy, Rust tests, frontend tests, type-check, build
```

Run it:

```powershell
pnpm tauri dev
```

Build a release executable — one self-contained binary, Typst compiler included:

```powershell
pnpm tauri build      # -> target\release\goodtype-desktop.exe
```

To hand a build to someone else, `scripts\build-windows-demo.ps1` produces
`target\Goodtype-demo.zip` with a launcher script. The build is not code-signed, so SmartScreen
will warn about an unrecognised app.

## How it is put together

| | |
|---|---|
| `apps/desktop/` | Tauri 2 shell and Svelte 5 frontend — canvas, palette, editor, workspace |
| `crates/goodtype-core/` | Notebook format, atomic persistence, history, recovery, stroke geometry |
| `crates/goodtype-typst/` | Embedded Typst compilation, completion, and PDF export |
| `fixtures/` | Notebook, ink, and export material the tests assert against |
| `xtask/` | `cargo xtask doctor` and `cargo xtask verify` |

Two decisions shape most of the code. **The Typst compiler is linked in rather than shelled out
to**, which is what makes rendering feel immediate and keeps the build to a single binary. And
**ink geometry is computed identically in Rust and TypeScript** — the live canvas needs it in the
webview, export needs it in Rust — with both implementations asserted against the same fixture, so
the two cannot drift apart without failing `cargo xtask verify`.

## Contributing

Issues and pull requests are welcome. `cargo xtask verify` is the gate; it runs formatting, Clippy
with warnings denied, both test suites, type-checking, and a production build, and it runs offline.

## License

Apache License 2.0 — see [LICENSE](./LICENSE).

Goodtype links the Typst compiler into its binary and embeds the fonts Typst ships with, so a built
executable redistributes third-party work under its own licences — Typst under Apache-2.0, and New
Computer Modern, Libertinus, and DejaVu Sans Mono under the SIL Open Font License and the Bitstream
Vera licence. [NOTICE](./NOTICE) lists all of it.
