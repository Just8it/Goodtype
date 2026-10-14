# Ink storage benchmark

> Status: production binary-ink implementation benchmark; final integrated measurements pending.

This benchmark measures the Phase 1 dense-page gate with deterministic handwriting-shaped data:
5,000 strokes, 60 points per stroke, and 300,000 points in one ink layer. Every point retains the
six current `f64` fields. Values use the same decimal quantization as the frontend persistence
pipeline. Stroke styles vary deterministically so the fixture does not compress as one repeated
record.

Run the standard release profile from the repository root:

```text
cargo bench -p goodtype-core --bench ink_storage
```

Use `--codec-only` when comparing representations without disk I/O, `--storage-only` for the
current Rust path, or `--quick` for a short correctness smoke test. `--strokes`, `--points`,
`--pages`, `--warmup`, and `--repeats` make larger or narrower runs explicit. The harness refuses
a layer above the current 20,000-stroke or 750,000-point limits. The standard profile uses one
warmup and seven measured repeats; increase repeats when collecting decision-grade tail data.

The codec wall-clock timings exclude disk I/O but include allocation work:

- Current compact JSON through `serde_json`.
- The production codec-v1 `.ink` representation through
  `goodtype_core::ink_codec::{encode, decode, encoded_len}`.
- Compact JSON compressed with DEFLATE level 6. Compression time starts from already serialized
  JSON; inflate timing includes JSON parsing.

The production codec is the versioned JSON-metadata and exact binary64-sample envelope documented
in [`formats/ink-v1.md`](../formats/ink-v1.md). The benchmark checks `encoded_len` against the
actual bytes and performs an exact value round trip before timing it.

The storage section uses public `goodtype-core` functions and a fresh temporary directory. It
reports create/save, open, commit, undo/redo, current binary and legacy JSON interrupted-open
recovery, and a multi-page
`open_page` sweep. Commit includes validation, current binary recovery-intent staging/removal,
canonical writes, fingerprinting, and undo snapshot accounting. The recovery rows install either
the documented `GTREC` version-2 envelope or a legacy version-1 JSON intent, then time the real
public open path as it restores the confirmed snapshot and archives the candidate. Temporary
notebooks disappear at the end of the process. No privileged fault-injection API is exposed solely
for this benchmark.

Each row reports median, nearest-rank p95, and worst wall-clock duration. Results depend on the
machine and filesystem. The harness does not measure heap or peak memory, rendering, selection,
erasing, export, active pen latency, or physical hardware behavior. Codec rows must not be quoted
as save/open latency.

## Pre-implementation baseline

On 2026-09-08, before production binary ink and compact recovery were integrated, this Windows
workspace ran:

```text
cargo bench -p goodtype-core --bench ink_storage -- --storage-only --warmup 1 --repeats 3 --pages 3
```

| Current JSON storage operation | Median | p95 / worst |
|---|---:|---:|
| Create/save dense notebook | 235.213 ms | 263.038 ms |
| Open dense page | 219.072 ms | 263.460 ms |
| Commit dense page | 1,214.829 ms | 1,394.432 ms |
| Undo/redo pair half-time | 914.895 ms | 964.599 ms |
| Legacy JSON interrupted-open recovery | 1,042.026 ms | 1,212.749 ms |
| Open three pages, one sweep | 707.520 ms | 760.956 ms |

With three samples, nearest-rank p95 equals the worst observation. Run the identical command after
integration for the comparison; do not compare a quick profile or a different repeat count.

## Pre-implementation codec evidence

On 2026-09-07, this Windows workspace ran the 5,000 × 60 codec profile in release mode with one
warmup and five measured repeats:

| Representation or operation | Bytes | Median | p95 / worst |
|---|---:|---:|---:|
| Compact JSON | 26,983,367 | encode 49.123 ms | 50.046 ms |
| Compact JSON | 26,983,367 | decode 70.341 ms | 75.746 ms |
| Experimental packed exact-f64 baseline | 14,965,047 (55.5% of JSON) | encode 12.216 ms | 14.082 ms |
| Experimental packed exact-f64 baseline | 14,965,047 | decode 10.108 ms | 12.917 ms |
| DEFLATE level 6 from existing JSON | 4,143,076 (15.4% of JSON) | compress 688.227 ms | 753.608 ms |
| Inflate plus JSON decode | 4,143,076 | 101.442 ms | 117.569 ms |

With only five samples, nearest-rank p95 equals the worst observation. These results show a useful
space/CPU tradeoff that informed ADR 0020. They are retained as historical evidence and are not
production-codec measurements. A release `--quick` run also exercised every pre-implementation
storage operation and exact round-trip check successfully.

## Production binary implementation results

The implementation run used the same 5,000-stroke / 300,000-point fixture:

```text
cargo bench -p goodtype-core --bench ink_storage -- --codec-only --warmup 1 --repeats 5
cargo bench -p goodtype-core --bench ink_storage -- --storage-only --warmup 1 --repeats 3 --pages 3
```

| Representation | Bytes | Encode median | Decode median |
|---|---:|---:|---:|
| Compact JSON | 26,983,367 | 153.691 ms | 179.362 ms |
| Production binary ink | 15,773,989 | 19.704 ms | 32.219 ms |
| DEFLATE level 6 | 4,143,077 | 2,074.167 ms | 308.799 ms |

DEFLATE encode starts from JSON; decode includes inflation and JSON parsing.
Binary ink uses 58.5% of the JSON bytes, with exact sample values preserved.

| Storage operation | Previous JSON median | Production median |
|---|---:|---:|
| Create/save dense notebook | 235.213 ms | 120.167 ms |
| Open dense page | 219.072 ms | 65.085 ms |
| Commit dense page | 1,214.829 ms | 380.438 ms |
| Undo/redo pair half-time | 914.895 ms | 377.775 ms |
| Legacy JSON interrupted-open recovery | 1,042.026 ms | 595.279 ms |
| Current binary interrupted-open recovery | Not available | 281.944 ms |
| Open three pages, one sweep | 707.520 ms | 177.375 ms |

These are worker-recorded medians from the completed implementation run; tail values
were not retained in this report. The benchmark prints them for subsequent runs.
The before/after runs were separate sessions, not a controlled same-session A/B test.
The implementation also enables `serde_json`'s `float_roundtrip` feature to preserve
metadata exactly; JSON parser configuration therefore differs from the historical
baseline. Do not attribute every timing difference solely to the file encoding.
Synthetic warm filesystem timings with three/five samples do not establish UI latency,
peak memory, physical pen behavior, or a 100-page notebook performance gate.

Focused benchmark Clippy and the release `--quick` correctness run passed alongside
the dense runs. Full repository verification also passed after integration.
