# Computational-page Stage 0 experiment

This is a development experiment. It adds no application IPC, notebook schema or frontend feature.
The desktop app does not depend on this crate, its Python environment or its downloads.
Never use this launcher as a production runner for arbitrary notebook code.

## Requirements

- Windows with AppContainer support, Rust and the normal Goodtype toolchain.
- Python 3.12 on the host, only for the explicit preparation script.
- A prepared portable runtime for the opt-in checks. Default repository verification needs none.

## Prepare explicitly

From the repository root:

```powershell
python tools/computation-spike/prepare.py output/computation-spike/run-001 --download
```

The command downloads a portable Python archive from python.org and pinned wheels from PyPI.
The pinned archive and wheel hashes identify experimental artifacts. They do not establish that third-party code is safe.
Python 3.12.10 matches the available host wheel selector. It is not a proposed production interpreter version.
The script writes only the prepared directory and normal development package-cache data.
It does not modify the host's installed Python packages. Do not run it against an existing completed experiment.

The preparation writes `runtime-manifest.json` and pip's `acquisition.json` with versions, sources and hashes.
All 30 Python distributions live in this experiment. Root Cargo/frontend dependencies remain unchanged.

## Verify

```powershell
$env:GOODTYPE_COMPUTATION_SPIKE_RUNTIME = (Resolve-Path output/computation-spike/run-001).Path
$env:GOODTYPE_SPIKE_SECRET = 'synthetic-environment-canary'
cargo xtask verify --with-computation-spike
```

The entry point runs normal repository verification, standalone formatting/Clippy, the native probe, layout fixtures and PDF.js parser checks.
It never downloads Python or scientific packages. Standalone Cargo operations use `--offline --locked`.
The first development build needs cached Rust dependencies for the standalone crate.

For a focused native iteration:

```powershell
cargo run --manifest-path tools/computation-spike/Cargo.toml --offline --locked -- output/computation-spike/run-001
```

Each native run creates a fresh `trials/probe-<pid>` directory. It preserves previous evidence.
The process creates an ephemeral AppContainer profile and removes it after the test.
Only staged runtime/input/output directories receive that profile's ACL entries.
The launcher never changes existing desktop or window-station permissions.

The child has no capability SIDs and inherits no host handles.
A one-process Job Object denies descendants, denies breakaway, limits process memory to one GiB and kills the process on job close.
The launcher waits for termination after a timeout. It does not rely on Python to obey cancellation.

Checks cover:

- Approved read-only input and writable derived output.
- Denied writes to input/runtime and denied reads/writes to four synthetic private locations.
- Denied native file access and parent-process memory access through `ctypes`.
- Denied child creation and Job Object breakaway.
- An AppContainer token with zero capability SIDs.
- A reachable host loopback listener that receives no sandbox connection.
- NumPy, SciPy, SymPy, pandas and Matplotlib calculations and saved PNG/SVG output.
- IPython state across cells, execution counts and partial mutation after an exception.
- Rejected allocation above the process memory limit.
- External termination of an infinite loop.

The loopback test does not establish all network isolation properties. External DNS, IPv6, UNC and broker-mediated access remain separate gates.
AppContainer also permits some resources declared for all application packages. These probes do not establish a complete allowlist of readable system resources.
The probe runs fixed test code. It does not bound arbitrary output-file growth or define a production wire protocol.

The Windows child-process creation mitigation caused pre-entry `0xc0000142` failures in the initial controls.
The final candidate omits that mitigation. The one-process Job Object still rejects the tested descendants with WinError 1816.
Do not remove that job limit to work around startup problems.

## Layout evidence

The test-only `crates/goodtype-typst/src/computation_spike.rs` uses the existing contained World and PDF backend offline.
The ordinary test checks short height, exact A4 width and deterministic repeated layout.
The ignored artifact test writes short, long, table, optional plot, A4-split and extreme-height PDFs into a fresh directory.

```powershell
$env:GOODTYPE_COMPUTATION_LAYOUT_OUTPUT = Join-Path $PWD 'output/computation-layout/manual-001'
cargo test -p goodtype-typst computational_layout_artifacts -- --ignored --nocapture
node tools/computation-spike/pdf_probe.mjs apps/desktop/node_modules/pdfjs-dist output/computation-layout/manual-001
```

Set `GOODTYPE_COMPUTATION_PLOT` to a saved Matplotlib SVG to include the real plot fixture.
The fixture uses existing fonts and normalizes the World root before reading the copied asset.
The A4 fixture keeps its short code cells together. Long-code splitting, repeated table headers and continuation labels remain production work.
PDF.js checks parsing, dimensions and text extraction. It does not test browser rendering or Acrobat compatibility.

`controls.py` runs fixed scientific/session tests outside AppContainer to separate package failures from launcher failures.
It is a diagnostic control, never an execution fallback. Its evidence explicitly says `sandboxed: false`.

## Remaining gates

Complete adversarial file/network/resource tests, session protocol and rich-output admission before accepting a runtime ADR.
Complete editor/preview height parity and the actual desktop/mobile viewer matrix before choosing a PDF ceiling.
Canonical `.ipynb` persistence, page sharing, inline/side editing, package management and release acceptance are not implemented here.
