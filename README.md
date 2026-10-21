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
place images or PDF material alongside both, on fixed pages that export as one PDF, with the ink
still vector and the text still selectable.

Goodtype keeps your notebooks as plain files on your own disk. No account, no sync, no telemetry.

## ⚠️ Alpha

> [!WARNING]
> **Goodtype is in alpha and the notebook format is not stable.**
>
> Current builds read notebook schemas 1, 2, and 3. A confirmed page save lazily migrates older
> ink to schema 3 and preserves the original JSON files. Older builds cannot read schema 3.
> Future alpha versions may still change the format; keep a separate copy of important work.
> The [ink format specification](./formats/ink-v1.md) documents binary storage and migration.

## What it does today

- **Pen input** with pressure, tilt, and palm rejection, on a canvas built for stylus latency.
- **Vector ink.** A stroke is stored as points and rendered as its silhouette, so pressure and nib
  character survive into the exported PDF instead of flattening to a constant width.
  Pens and shapes support solid, dashed, and dotted marks with adjustable spacing.
- **Typst blocks** compiled in-process, without a system Typst installation. Editor intelligence
  uses the bundled Tinymist helper with an embedded completion/hover/formatting fallback.
  Bundled fonts and fonts installed on your computer are available to both preview and export.
- **Page text**, a Typst writing surface that belongs to the page rather than floating on it, with
  headings and display equations sitting on the paper's own ruling.
  The sidebar colour picker inserts ordinary Typst source for text, math, and colour values.
- **Typst Universe packages**, resolved from a local cache first and downloaded only on a miss.
- **Paper worth writing on.** Ruled, squared, dotted and plain templates in three paper tones, at
  sizes from A6 to Tabloid in either orientation, previewed before the page exists.
- **Images and PDF material** placed and scaled on the page, or a PDF brought in a page at a time
  as pages of its own.
- **Goodnotes import** from `.goodnotes` files or backup ZIPs into a folder of ordinary notebooks.
  Open **New → Import from Goodnotes…** in the Library to download the optional
  [importer](https://github.com/Just8it/Goodtype-Goodnodes-importer). Conversion runs locally,
  preserves the originals, and brings across editable handwriting and supported shapes, with
  cropped vector artwork for other supported content. Some Goodnotes features are not supported.
- **Mixed selection** with lasso filters for handwriting, highlighting, shapes, images and Typst
  blocks, so you can move, resize, duplicate or delete them together.
- **Multi-page notebooks** that scroll continuously, with pages you can reorder, duplicate, clear
  or delete, and undo scoped either to the page in view or to the whole notebook.
- **Several notebooks open at once** in tabs, with search across a notebook's typed content.
- **PDF export** of the whole notebook, in manifest order, from the files on disk rather than from
  whatever the screen happens to be showing.
- **Crash recovery** built on atomic writes and a revision check, so an interrupted save cannot
  leave a notebook it can no longer open.

## Local-first

A notebook is a directory of ordinary files, a JSON manifest, one file per page, Typst sources as
`.typ`, documented binary ink layers, and ordinary assets kept beside them. Older JSON ink stays
readable. The supplied ink decoder prints binary layers as JSON; see the
[wire format](./formats/ink-v1.md). Nothing about the format requires Goodtype to be running.

Typst packages download only on a cache miss, and package downloads can be switched off. The
optional Goodnotes importer downloads from GitHub when you request it; update checks and
downloads are explicit, with signed release metadata verified before an update is installed.
Your notebooks stay on your device. Once installed, the importer works offline, and Goodtype's
core workflows work without it.

## Build

Goodtype currently targets Windows 10 and 11.

**You will need**

- [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) (already present on current Windows)
- [Rustup](https://rustup.rs/), the pinned toolchain installs itself from `rust-toolchain.toml`
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

Build the release executable with its embedded Typst compiler and bundled editor helper:

```powershell
pnpm tauri build      # -> target\release\goodtype-desktop.exe
```

To produce the portable Windows folder and ZIP, run from the repository root:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-windows-demo.ps1
```

The script installs locked frontend dependencies, builds the release app and ink decoder, then
creates `target\Goodtype-Windows-<date>-<time>\` and the matching `.zip`. Both contain
`Goodtype.exe`, `tinymist.exe`, `goodtype-ink.exe`, `ink-v1.md`, and `README.txt`.
Open `Goodtype.exe` directly; keep the entire folder together. No launcher script is needed.
The build is not code-signed, so SmartScreen
will warn about an unrecognised app.

## How it is put together

| | |
|---|---|
| `apps/desktop/` | Tauri 2 shell and Svelte 5 frontend. canvas, palette, editor, workspace |
| `crates/goodtype-core/` | Notebook format, atomic persistence, history, recovery, stroke geometry |
| `crates/goodtype-typst/` | Embedded Typst compilation, completion, and PDF export |
| `fixtures/` | Notebook, ink, and export material the tests assert against |
| `xtask/` | `cargo xtask doctor` and `cargo xtask verify` |

Two decisions shape most of the code. **The Typst compiler is linked in rather than shelled out
to**, which keeps preview and export inside Rust; editor intelligence ships a separate helper. And
**ink geometry is computed identically in Rust and TypeScript**, the live canvas needs it in the
webview, export needs it in Rust, with both implementations asserted against the same fixture, so
the two cannot drift apart without failing `cargo xtask verify`.

## Contributing

Issues and pull requests are welcome. `cargo xtask verify` is the gate; it runs formatting, Clippy
with warnings denied, both test suites, type-checking, and a production build, and it runs offline.

## License

Apache License 2.0 - see [LICENSE](./LICENSE).

Goodtype links the Typst compiler into its binary and embeds the fonts Typst ships with, so a built
executable redistributes third-party work under its own licences, Typst under Apache-2.0, and New
Computer Modern, Libertinus, and DejaVu Sans Mono under the SIL Open Font License and the Bitstream
Vera licence. [NOTICE](./NOTICE) lists all of it.
