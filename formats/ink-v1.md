# Goodtype ink binary format, codec version 1

Status: implementation contract, 2026-09-08. This specification ships with the source.
Notebook schema 3 introduces `.ink` files; the codec version below is independent of
the notebook schema version. Notebook metadata and pages remain JSON. Older schema
1/2 pages may retain their original JSON ink layers in a schema-3 notebook.

## File layout

All header integers are unsigned little-endian. Offsets count bytes from the start.

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 8 | Magic: `47 54 49 4e 4b 00 0d 0a` (`GTINK` + NUL + CR + LF) |
| 8 | 4 | Codec version, exactly 1 |
| 12 | 4 | UTF-8 metadata byte length, M |
| 16 | 4 | Total sample count, P |
| 20 | M | JSON metadata, no byte-order mark |
| 20 + M | 48 × P | Sample payload |

The complete length must equal `20 + M + 48 * P`. Padding, trailing bytes,
additional compression, and arbitrary extensions are not supported in codec 1.
Unknown versions must fail visibly, never fall back to guessing the format.

## Metadata

The metadata is a JSON object with `schemaVersion`, `id`, `pageId`, and `strokes`.
The strokes array preserves canonical stroke order. A stroke has the ordinary
canonical fields `id`, `zIndex`, `tool`, `color`, `widthPt`, `pattern`, `pressure`,
`taper`, `opacity`, `groupId`, and `transform`, plus `pointCount`. The `points` field
is absent. `pressure` at stroke level is the boolean nib behavior, not a sample value.

`transform` has `translateX`, `translateY`, `scaleX`, `scaleY`, and `rotation`.
`tool` is `pen` or `highlighter`. `pattern` is `solid`, `dashed`, or `dotted`.
`groupId` is a string or null. IDs, colors, numeric values, supported schema versions,
and transform constraints follow Goodtype's canonical ink validation rules.

The sum of all `pointCount` values must equal P. An empty layer has no strokes and
P=0; an individual stroke must have at least one point. Metadata float fields use
round-trippable JSON numbers. Writers emit all resolved stroke style fields; readers
must not infer appearance from a current application preset.

Codec 1 is a closed schema. Unknown metadata fields are rejected rather than silently
discarded. A future semantic extension requires a new codec version and documented
compatibility rules. Notebook/page JSON and legacy JSON ink retain their own rules.

## Samples

Visit strokes in metadata order, and visit each stroke's `pointCount` samples in
original order. Each sample is exactly six IEEE-754 binary64 values, little-endian:

1. `x` in page points.
2. `y` in page points.
3. `pressure` in the inclusive interval 0–1.
4. `timeMs` in milliseconds.
5. `tiltX` in degrees.
6. `tiltY` in degrees.

No extra quantization, rounding, downsampling, or coordinate conversion is performed.
Binary64 bits, including signed zero, are preserved for valid sample values. NaN and
infinity are invalid. Canonical time/tilt/geometry validation also applies. Metadata
transforms are separate from the original sample coordinates.

## Resource limits and decoding

- File: at most 64 MiB (67,108,864 bytes).
- Metadata: at most 8 MiB (8,388,608 bytes).
- Strokes: at most 20,000 per layer.
- Samples: at most 750,000 per layer.

Read and check the fixed header before interpreting the metadata or allocating sample
arrays. Check arithmetic for overflow, bounds for every slice, exact payload length,
metadata shape, count totals, and canonical ink constraints. A claimed count must not
trigger allocation before it is checked against both limits and actual available bytes.
Rust validates layer identity against the containing page and its reference before use.

Conversion to ordinary ink JSON removes each `pointCount` and inserts its decoded
`points` array. Conversion does not change any style, transform, ordering, sample, or
schema value.

Decode an explicit file without launching the desktop app:

```text
cargo run -p goodtype-core --bin goodtype-ink -- decode INPUT
```

The standalone executable prints ordinary pretty JSON to stdout and diagnostics to
stderr. It rejects inputs over 64 MiB and does not write or overwrite any notebook
file. After building, `goodtype-ink decode INPUT` also works directly. A shell can
redirect stdout to a separate inspection file; never use the input path as that output.

## Persistence and compatibility

The page's ink reference identifies the authoritative layer. Schema-3 writers use
`.ink` files. Readers continue to accept supported legacy `.json` ink references.
Migration writes binary data before changing the page reference and retains the old
JSON file. That retained file is an original, not a second authoritative copy.
Existing unrelated migration targets must not be overwritten; when a preferred name
is occupied, a different unused sibling name is selected. A failed or interrupted
write preserves the last confirmed page through Goodtype's recovery protocol.

The application must not open or rewrite an unsupported future schema/codec as if it
were supported. A documented decoder keeps files inspectable without the desktop app.

## Codec version 2

Codec 2 (ADR 0023) is codec 1 with one addition: a stroke's metadata may carry
`patternSpacing`, a finite number from 0.5 to 3 that scales the gap between pattern
marks. The header, layout, sample payload, limits, and every other rule are unchanged,
and the version field is `2`.

Writers emit codec 2 only for a layer in which at least one stroke has a spacing other
than 1, and omit `patternSpacing` on strokes whose spacing is 1. All other layers are
written as codec 1, byte for byte as before. Readers that support codec 2 accept both
versions; codec-1 metadata containing `patternSpacing` is invalid. Readers that only
know codec 1 reject codec 2 visibly, as required above.

## Codec version 3

Codec 3 (ADR 0037) is codec 2 with one addition: a stroke's metadata may carry `trim`,
an object `{ "startPt": a, "endPt": b }` of two finite numbers from 0 to 1,000,000. It
marks a fragment left by the precise eraser: `a` and `b` are the arc length of the
original stroke before and after the fragment, in the stroke's own points. The taper ramp
is measured from the original ends and dash/dot phase continues from `a`. Without `trim`
a stroke is whole. The header, layout, sample payload, limits, and every other rule are
unchanged, and the version field is `3`; codec-3 metadata may also carry `patternSpacing`.

Writers emit codec 3 only for a layer in which at least one stroke carries `trim`; all
other layers keep their codec 1 or 2 bytes. Codec-1 or codec-2 metadata containing `trim`
is invalid. Readers that only know codecs 1 and 2 reject codec 3 visibly.
