# Goodtype

Goodtype is a local-first, canvas-first technical notebook for handwriting, Typst, images, and PDF material on fixed pages.

The current Windows demo supports pen input, Typst blocks, images, local persistence, undo/redo, PDF export, and multiple vertically scrolling pages. It is an early prototype, not a production release.

## Build on Windows

### Requirements

- Windows 10 or 11 with [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/).
- [Git](https://git-scm.com/download/win).
- [Node.js 25.2.1](https://nodejs.org/).
- [pnpm 11.9.0](https://pnpm.io/installation).
- [Rustup](https://rustup.rs/). The repository installs its pinned Rust toolchain automatically.
- [Visual Studio Build Tools 2022](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with **Desktop development with C++** and a Windows 10 or 11 SDK.
- [Typst 0.15.1](https://github.com/typst/typst/releases/tag/v0.15.1). Put `typst.exe` on `PATH`, or copy it to `target\tools\typst.exe`.

Install the pinned package manager if needed:

```powershell
npm install --global pnpm@11.9.0
```

Clone and verify the project:

```powershell
git clone https://github.com/Just8it/Goodtype.git
cd Goodtype
pnpm install --frozen-lockfile
cargo xtask doctor
cargo xtask verify
```

Run the development build:

```powershell
pnpm tauri dev
```

Build the release executable:

```powershell
pnpm tauri build
```

The executable is written to:

```text
target\release\goodtype-desktop.exe
```

## Create a ZIP for a friend

With Typst installed or copied to `target\tools\typst.exe`, run:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-demo.ps1
```

The script creates:

```text
target\Goodtype-demo.zip
```

Your friend can extract the ZIP and double-click **Start Goodtype.cmd**. The ZIP contains Goodtype and the required Typst executable. Because the demo is not code-signed, Windows SmartScreen may show an unrecognized-app warning.

Notebook data remains local under the Windows application-data directory. The demo has no cloud sync, account, automatic updater, or telemetry.

## Repository layout

- `apps/desktop/`: Tauri 2 and Svelte 5 desktop application.
- `crates/goodtype-core/`: canonical notebook types, persistence, history, and recovery.
- `crates/goodtype-typst/`: restricted Typst compilation and PDF export.
- `fixtures/`: notebook, ink, Typst, and export test material.
- `xtask/`: repository verification commands.

## License

Apache-2.0. See [LICENSE](./LICENSE) and [NOTICE](./NOTICE).
