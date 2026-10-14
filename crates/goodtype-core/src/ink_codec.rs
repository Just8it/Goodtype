//! Versioned binary encoding for one canonical ink layer.
//!
//! The codec is deliberately pure: callers own bounded file reads, atomic writes, path checks,
//! and validation against the containing page. The wire contract lives in `formats/ink-v1.md`.

use serde::{Deserialize, Serialize};

use crate::storage::{
    MAX_INK_BYTES, MAX_INK_POINTS_PER_LAYER, MAX_INK_STROKES_PER_LAYER, StorageError,
    validate_ink_layer,
};
use crate::{
    InkLayer, MIN_SUPPORTED_SCHEMA_VERSION, SCHEMA_VERSION, Stroke, StrokePattern, StrokePoint,
    StrokeTool, Transform,
};

const MAGIC: &[u8; 8] = b"GTINK\0\r\n";
const CODEC_VERSION: u32 = 1;
/// Codec 2 is codec 1 plus an optional per-stroke `patternSpacing` (ADR 0023). A layer is written
/// as codec 2 only when a stroke actually carries a spacing, so ordinary layers keep their exact
/// codec-1 bytes and older readers fail visibly only on ink they could not render correctly.
const CODEC_VERSION_SPACING: u32 = 2;

fn codec_version(layer: &InkLayer) -> u32 {
    if layer
        .strokes
        .iter()
        .any(|stroke| !crate::is_unit_spacing(&stroke.pattern_spacing))
    {
        CODEC_VERSION_SPACING
    } else {
        CODEC_VERSION
    }
}
const HEADER_BYTES: usize = 20;
const SAMPLE_FIELDS: usize = 6;
const SAMPLE_BYTES: usize = SAMPLE_FIELDS * size_of::<f64>();
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Metadata {
    schema_version: u32,
    id: String,
    page_id: String,
    strokes: Vec<StrokeMetadata>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StrokeMetadata {
    id: String,
    z_index: i32,
    tool: StrokeTool,
    color: String,
    width_pt: f64,
    pattern: StrokePattern,
    /// Codec 2 only; its presence in codec-1 metadata is rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pattern_spacing: Option<f64>,
    pressure: bool,
    taper: f64,
    opacity: f64,
    group_id: Option<String>,
    point_count: u32,
    transform: TransformMetadata,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransformMetadata {
    translate_x: f64,
    translate_y: f64,
    scale_x: f64,
    scale_y: f64,
    rotation: f64,
}

/// Returns the exact codec-v1 byte length after applying the same validation as [`encode`].
pub fn encoded_len(layer: &InkLayer) -> Result<usize, StorageError> {
    let (metadata, point_count) = metadata_bytes(layer)?;
    wire_len(metadata.len(), point_count)
}

/// Encodes one ink layer without reading or writing the filesystem.
pub fn encode(layer: &InkLayer) -> Result<Vec<u8>, StorageError> {
    let (metadata, point_count) = metadata_bytes(layer)?;
    let length = wire_len(metadata.len(), point_count)?;
    let metadata_len = u32::try_from(metadata.len())
        .map_err(|_| invalid_error("binary ink metadata length exceeds u32"))?;
    let point_count = u32::try_from(point_count)
        .map_err(|_| invalid_error("binary ink sample count exceeds u32"))?;

    let mut bytes = Vec::with_capacity(length);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&codec_version(layer).to_le_bytes());
    bytes.extend_from_slice(&metadata_len.to_le_bytes());
    bytes.extend_from_slice(&point_count.to_le_bytes());
    bytes.extend_from_slice(&metadata);
    for stroke in &layer.strokes {
        for point in &stroke.points {
            for value in [
                point.x,
                point.y,
                point.pressure,
                point.time_ms,
                point.tilt_x,
                point.tilt_y,
            ] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    debug_assert_eq!(bytes.len(), length);
    Ok(bytes)
}

/// Decodes one complete codec-v1 ink layer without filesystem side effects.
pub fn decode(bytes: &[u8]) -> Result<InkLayer, StorageError> {
    if bytes.len() > MAX_INK_BYTES {
        return invalid(format!(
            "binary ink is {} bytes; maximum is {MAX_INK_BYTES}",
            bytes.len()
        ));
    }
    if bytes.len() < HEADER_BYTES {
        return invalid("binary ink header is truncated");
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return invalid("binary ink magic is invalid");
    }

    let version = header_u32(bytes, 8)?;
    if version != CODEC_VERSION && version != CODEC_VERSION_SPACING {
        return invalid(format!("unsupported binary ink codec version {version}"));
    }
    let metadata_len = usize::try_from(header_u32(bytes, 12)?)
        .map_err(|_| invalid_error("binary ink metadata length is unsupported"))?;
    let point_count = usize::try_from(header_u32(bytes, 16)?)
        .map_err(|_| invalid_error("binary ink sample count is unsupported"))?;
    if metadata_len > MAX_METADATA_BYTES {
        return invalid(format!(
            "binary ink metadata is {metadata_len} bytes; maximum is {MAX_METADATA_BYTES}"
        ));
    }
    if point_count > MAX_INK_POINTS_PER_LAYER {
        return invalid(format!(
            "binary ink holds {point_count} samples; maximum is {MAX_INK_POINTS_PER_LAYER}"
        ));
    }
    let expected_len = wire_len(metadata_len, point_count)?;
    if bytes.len() != expected_len {
        return invalid(format!(
            "binary ink length is {}; header requires {expected_len}",
            bytes.len()
        ));
    }

    let metadata_end = HEADER_BYTES
        .checked_add(metadata_len)
        .ok_or_else(|| invalid_error("binary ink metadata offset overflow"))?;
    let metadata: Metadata = serde_json::from_slice(&bytes[HEADER_BYTES..metadata_end])?;
    if metadata.strokes.len() > MAX_INK_STROKES_PER_LAYER {
        return invalid(format!(
            "binary ink holds {} strokes; maximum is {MAX_INK_STROKES_PER_LAYER}",
            metadata.strokes.len()
        ));
    }
    let described_points = metadata.strokes.iter().try_fold(0usize, |total, stroke| {
        let count = usize::try_from(stroke.point_count)
            .map_err(|_| invalid_error("binary ink stroke sample count is unsupported"))?;
        if count == 0 {
            return invalid("binary ink strokes must contain at least one sample");
        }
        total
            .checked_add(count)
            .ok_or_else(|| invalid_error("binary ink stroke sample count overflow"))
    })?;
    if described_points != point_count {
        return invalid(format!(
            "binary ink metadata describes {described_points} samples; header declares {point_count}"
        ));
    }
    if version == CODEC_VERSION
        && metadata
            .strokes
            .iter()
            .any(|stroke| stroke.pattern_spacing.is_some())
    {
        return invalid("codec-1 binary ink cannot carry patternSpacing");
    }

    let mut offset = metadata_end;
    let mut strokes = Vec::with_capacity(metadata.strokes.len());
    for stroke in metadata.strokes {
        let mut points = Vec::with_capacity(stroke.point_count as usize);
        for _ in 0..stroke.point_count {
            points.push(StrokePoint {
                x: take_f64(bytes, &mut offset)?,
                y: take_f64(bytes, &mut offset)?,
                pressure: take_f64(bytes, &mut offset)?,
                time_ms: take_f64(bytes, &mut offset)?,
                tilt_x: take_f64(bytes, &mut offset)?,
                tilt_y: take_f64(bytes, &mut offset)?,
            });
        }
        strokes.push(Stroke {
            id: stroke.id,
            z_index: stroke.z_index,
            tool: stroke.tool,
            color: stroke.color,
            width_pt: stroke.width_pt,
            pattern: stroke.pattern,
            pattern_spacing: stroke.pattern_spacing.unwrap_or(1.0),
            pressure: stroke.pressure,
            taper: stroke.taper,
            opacity: stroke.opacity,
            group_id: stroke.group_id,
            points,
            transform: Transform {
                translate_x: stroke.transform.translate_x,
                translate_y: stroke.transform.translate_y,
                scale_x: stroke.transform.scale_x,
                scale_y: stroke.transform.scale_y,
                rotation: stroke.transform.rotation,
            },
        });
    }
    debug_assert_eq!(offset, bytes.len());
    let layer = InkLayer {
        schema_version: metadata.schema_version,
        id: metadata.id,
        page_id: metadata.page_id,
        strokes,
    };
    validate_codec_layer(&layer)?;
    Ok(layer)
}

fn metadata_bytes(layer: &InkLayer) -> Result<(Vec<u8>, usize), StorageError> {
    validate_codec_layer(layer)?;
    let point_count = layer.strokes.iter().try_fold(0usize, |total, stroke| {
        total
            .checked_add(stroke.points.len())
            .ok_or_else(|| invalid_error("binary ink sample count overflow"))
    })?;
    let metadata = Metadata {
        schema_version: layer.schema_version,
        id: layer.id.clone(),
        page_id: layer.page_id.clone(),
        strokes: layer
            .strokes
            .iter()
            .map(|stroke| {
                Ok(StrokeMetadata {
                    id: stroke.id.clone(),
                    z_index: stroke.z_index,
                    tool: stroke.tool.clone(),
                    color: stroke.color.clone(),
                    width_pt: stroke.width_pt,
                    pattern: stroke.pattern,
                    pattern_spacing: (!crate::is_unit_spacing(&stroke.pattern_spacing))
                        .then_some(stroke.pattern_spacing),
                    pressure: stroke.pressure,
                    taper: stroke.taper,
                    opacity: stroke.opacity,
                    group_id: stroke.group_id.clone(),
                    point_count: u32::try_from(stroke.points.len())
                        .map_err(|_| invalid_error("binary ink stroke sample count exceeds u32"))?,
                    transform: TransformMetadata {
                        translate_x: stroke.transform.translate_x,
                        translate_y: stroke.transform.translate_y,
                        scale_x: stroke.transform.scale_x,
                        scale_y: stroke.transform.scale_y,
                        rotation: stroke.transform.rotation,
                    },
                })
            })
            .collect::<Result<_, StorageError>>()?,
    };
    let bytes = serde_json::to_vec(&metadata)?;
    if bytes.len() > MAX_METADATA_BYTES {
        return invalid(format!(
            "binary ink metadata is {} bytes; maximum is {MAX_METADATA_BYTES}",
            bytes.len()
        ));
    }
    Ok((bytes, point_count))
}

fn wire_len(metadata_len: usize, point_count: usize) -> Result<usize, StorageError> {
    let payload_len = point_count
        .checked_mul(SAMPLE_BYTES)
        .ok_or_else(|| invalid_error("binary ink payload length overflow"))?;
    let length = HEADER_BYTES
        .checked_add(metadata_len)
        .and_then(|value| value.checked_add(payload_len))
        .ok_or_else(|| invalid_error("binary ink length overflow"))?;
    if length > MAX_INK_BYTES {
        return invalid(format!(
            "binary ink is {length} bytes; maximum is {MAX_INK_BYTES}"
        ));
    }
    Ok(length)
}

fn validate_codec_layer(layer: &InkLayer) -> Result<(), StorageError> {
    if !(MIN_SUPPORTED_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&layer.schema_version) {
        return invalid(format!(
            "unsupported ink schema version {}",
            layer.schema_version
        ));
    }
    validate_ink_layer(layer)
}

fn header_u32(bytes: &[u8], offset: usize) -> Result<u32, StorageError> {
    let end = offset
        .checked_add(size_of::<u32>())
        .ok_or_else(|| invalid_error("binary ink header offset overflow"))?;
    let raw = bytes
        .get(offset..end)
        .ok_or_else(|| invalid_error("binary ink header is truncated"))?;
    Ok(u32::from_le_bytes(raw.try_into().map_err(|_| {
        invalid_error("binary ink header field width is invalid")
    })?))
}

fn take_f64(bytes: &[u8], offset: &mut usize) -> Result<f64, StorageError> {
    let end = offset
        .checked_add(size_of::<f64>())
        .ok_or_else(|| invalid_error("binary ink sample offset overflow"))?;
    let raw = bytes
        .get(*offset..end)
        .ok_or_else(|| invalid_error("binary ink sample payload is truncated"))?;
    *offset = end;
    Ok(f64::from_le_bytes(raw.try_into().map_err(|_| {
        invalid_error("binary ink sample field width is invalid")
    })?))
}

fn invalid<T>(message: impl Into<String>) -> Result<T, StorageError> {
    Err(StorageError::InvalidNotebook(message.into()))
}

fn invalid_error(message: impl Into<String>) -> StorageError {
    StorageError::InvalidNotebook(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer() -> InkLayer {
        InkLayer {
            schema_version: 3,
            id: "layer-1".into(),
            page_id: "page-1".into(),
            strokes: vec![Stroke {
                id: "stroke-1".into(),
                z_index: -17,
                tool: StrokeTool::Pen,
                color: "#123456".into(),
                width_pt: f64::from_bits(1),
                pattern: StrokePattern::Dashed,
                pattern_spacing: 1.0,
                pressure: true,
                taper: f64::from_bits(0x3fc9_9999_9999_999a),
                opacity: f64::from_bits(0x3fe0_0000_0000_0001),
                group_id: Some("group-1".into()),
                points: vec![
                    StrokePoint {
                        x: -0.0,
                        y: f64::from_bits(0x4009_21fb_5444_2d18),
                        pressure: f64::from_bits(0x3fe0_0000_0000_0001),
                        time_ms: f64::from_bits(1),
                        tilt_x: -89.999_999_999_999_99,
                        tilt_y: 89.999_999_999_999_99,
                    },
                    StrokePoint {
                        x: f64::MAX,
                        y: f64::MIN,
                        pressure: 1.0,
                        time_ms: f64::MAX,
                        tilt_x: -0.0,
                        tilt_y: 0.0,
                    },
                ],
                transform: Transform {
                    translate_x: -0.0,
                    translate_y: f64::MAX,
                    scale_x: f64::from_bits(1),
                    scale_y: f64::from_bits(0x3fe6_a09e_667f_3bcc),
                    rotation: f64::from_bits(0xc005_bf0a_8b14_5769),
                },
            }],
        }
    }

    fn assert_f64(left: f64, right: f64) {
        assert_eq!(left.to_bits(), right.to_bits());
    }

    #[test]
    fn exact_round_trip_preserves_sample_and_metadata_f64_bits() {
        let original = layer();
        let encoded = encode(&original).unwrap();
        assert_eq!(encoded.len(), encoded_len(&original).unwrap());
        let decoded = decode(&encoded).unwrap();
        let before = &original.strokes[0];
        let after = &decoded.strokes[0];

        assert_f64(before.width_pt, after.width_pt);
        assert_f64(before.taper, after.taper);
        assert_f64(before.opacity, after.opacity);
        assert_f64(before.transform.translate_x, after.transform.translate_x);
        assert_f64(before.transform.translate_y, after.transform.translate_y);
        assert_f64(before.transform.scale_x, after.transform.scale_x);
        assert_f64(before.transform.scale_y, after.transform.scale_y);
        assert_f64(before.transform.rotation, after.transform.rotation);
        for (before, after) in before.points.iter().zip(&after.points) {
            assert_f64(before.x, after.x);
            assert_f64(before.y, after.y);
            assert_f64(before.pressure, after.pressure);
            assert_f64(before.time_ms, after.time_ms);
            assert_f64(before.tilt_x, after.tilt_x);
            assert_f64(before.tilt_y, after.tilt_y);
        }
    }

    #[test]
    fn pattern_spacing_round_trips_and_is_absent_at_its_default() {
        let plain = encode(&layer()).unwrap();
        assert_eq!(header_u32(&plain, 8).unwrap(), CODEC_VERSION);
        assert!(!String::from_utf8_lossy(&plain).contains("patternSpacing"));
        assert_eq!(decode(&plain).unwrap().strokes[0].pattern_spacing, 1.0);

        let mut spaced = layer();
        spaced.strokes[0].pattern_spacing = 2.25;
        let encoded = encode(&spaced).unwrap();
        assert_eq!(header_u32(&encoded, 8).unwrap(), CODEC_VERSION_SPACING);
        assert_eq!(encoded.len(), encoded_len(&spaced).unwrap());
        assert_eq!(decode(&encoded).unwrap().strokes[0].pattern_spacing, 2.25);

        // Codec 1 stays a closed schema: the same metadata under a version-1 header is refused.
        let mut relabelled = encoded.clone();
        relabelled[8..12].copy_from_slice(&CODEC_VERSION.to_le_bytes());
        assert!(decode(&relabelled).is_err());
    }

    #[test]
    fn metadata_json_round_trips_deterministic_finite_binary64_values() {
        let mut state = 0x4d59_5df4_d0f3_3173_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };
        for _ in 0..1_000 {
            let positive =
                |bits: u64| f64::from_bits((bits & ((1_u64 << 52) - 1)) | (1022_u64 << 52));
            let signed = |bits: u64| {
                let sign = bits & (1_u64 << 63);
                let exponent = ((bits >> 52) & 0x7fe).max(1) << 52;
                f64::from_bits(sign | exponent | (bits & ((1_u64 << 52) - 1)))
            };
            let mut original = layer();
            {
                let stroke = &mut original.strokes[0];
                stroke.width_pt = positive(next());
                stroke.taper = positive(next());
                stroke.opacity = positive(next());
                stroke.transform.translate_x = signed(next());
                stroke.transform.translate_y = signed(next());
                stroke.transform.scale_x = positive(next());
                stroke.transform.scale_y = positive(next());
                stroke.transform.rotation = signed(next());
            }

            let decoded = decode(&encode(&original).unwrap()).unwrap();
            let stroke = &original.strokes[0];
            let after = &decoded.strokes[0];
            assert_f64(stroke.width_pt, after.width_pt);
            assert_f64(stroke.taper, after.taper);
            assert_f64(stroke.opacity, after.opacity);
            assert_f64(stroke.transform.translate_x, after.transform.translate_x);
            assert_f64(stroke.transform.translate_y, after.transform.translate_y);
            assert_f64(stroke.transform.scale_x, after.transform.scale_x);
            assert_f64(stroke.transform.scale_y, after.transform.scale_y);
            assert_f64(stroke.transform.rotation, after.transform.rotation);
        }
    }

    #[test]
    fn frozen_wire_vector_fixes_header_metadata_and_sample_order() {
        const METADATA: &[u8] = br##"{"schemaVersion":3,"id":"l","pageId":"p","strokes":[{"id":"s","zIndex":7,"tool":"pen","color":"#000000","widthPt":1.0,"pattern":"solid","pressure":false,"taper":0.0,"opacity":1.0,"groupId":null,"pointCount":1,"transform":{"translateX":0.0,"translateY":0.0,"scaleX":1.0,"scaleY":1.0,"rotation":0.0}}]}"##;
        const SAMPLE: [u8; 48] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, // x = 1
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, // y = -2
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xe0, 0x3f, // pressure = 0.5
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x40, // timeMs = 8
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xbf, // tiltX = -1
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // tiltY = 0
        ];
        assert_eq!(METADATA.len(), 300);
        let mut wire = vec![
            0x47, 0x54, 0x49, 0x4e, 0x4b, 0x00, 0x0d, 0x0a, // magic
            0x01, 0x00, 0x00, 0x00, // codec version
            0x2c, 0x01, 0x00, 0x00, // metadata length = 300
            0x01, 0x00, 0x00, 0x00, // total point count = 1
        ];
        wire.extend_from_slice(METADATA);
        wire.extend_from_slice(&SAMPLE);

        let decoded = decode(&wire).unwrap();
        assert_eq!(decoded.schema_version, 3);
        assert_eq!(decoded.id, "l");
        assert_eq!(decoded.page_id, "p");
        assert_eq!(decoded.strokes.len(), 1);
        let stroke = &decoded.strokes[0];
        assert_eq!(stroke.id, "s");
        assert_eq!(stroke.z_index, 7);
        assert_eq!(stroke.points.len(), 1);
        let point = &stroke.points[0];
        assert_eq!(
            [
                point.x,
                point.y,
                point.pressure,
                point.time_ms,
                point.tilt_x,
                point.tilt_y
            ],
            [1.0, -2.0, 0.5, 8.0, -1.0, 0.0]
        );
        assert_eq!(encode(&decoded).unwrap(), wire);
    }

    #[test]
    fn empty_layer_round_trips() {
        let mut empty = layer();
        empty.strokes.clear();
        let encoded = encode(&empty).unwrap();
        assert_eq!(header_u32(&encoded, 16).unwrap(), 0);
        let decoded = decode(&encoded).unwrap();
        assert!(decoded.strokes.is_empty());
    }

    #[test]
    fn rejects_truncation_trailing_bytes_and_unsupported_versions() {
        let encoded = encode(&layer()).unwrap();
        for end in 0..encoded.len() {
            assert!(
                decode(&encoded[..end]).is_err(),
                "accepted truncation at {end}"
            );
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());
        let mut wrong_magic = encoded.clone();
        wrong_magic[0] ^= 0xff;
        assert!(decode(&wrong_magic).is_err());
        let mut future = encoded;
        future[8..12].copy_from_slice(&(CODEC_VERSION_SPACING + 1).to_le_bytes());
        assert!(decode(&future).is_err());
    }

    #[test]
    fn rejects_claimed_counts_before_allocating_samples() {
        let mut encoded = encode(&layer()).unwrap();
        encoded[16..20].copy_from_slice(&(MAX_INK_POINTS_PER_LAYER as u32 + 1).to_le_bytes());
        assert!(decode(&encoded).is_err());

        let mut encoded = encode(&layer()).unwrap();
        encoded[12..16].copy_from_slice(&(MAX_METADATA_BYTES as u32 + 1).to_le_bytes());
        assert!(decode(&encoded).is_err());
    }

    #[test]
    fn rejects_unknown_metadata_fields() {
        let encoded = encode(&layer()).unwrap();
        let metadata_len = header_u32(&encoded, 12).unwrap() as usize;
        let mut value: serde_json::Value =
            serde_json::from_slice(&encoded[HEADER_BYTES..HEADER_BYTES + metadata_len]).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("future".into(), true.into());
        let metadata = serde_json::to_vec(&value).unwrap();
        let mut changed = Vec::new();
        changed.extend_from_slice(MAGIC);
        changed.extend_from_slice(&CODEC_VERSION.to_le_bytes());
        changed.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        changed.extend_from_slice(&2_u32.to_le_bytes());
        changed.extend_from_slice(&metadata);
        changed.extend_from_slice(&encoded[HEADER_BYTES + metadata_len..]);
        assert!(decode(&changed).is_err());

        let encoded = encode(&layer()).unwrap();
        let metadata_len = header_u32(&encoded, 12).unwrap() as usize;
        let mut value: serde_json::Value =
            serde_json::from_slice(&encoded[HEADER_BYTES..HEADER_BYTES + metadata_len]).unwrap();
        value["strokes"][0]["transform"]
            .as_object_mut()
            .unwrap()
            .insert("skewX".into(), 0.into());
        let metadata = serde_json::to_vec(&value).unwrap();
        let mut changed = Vec::new();
        changed.extend_from_slice(MAGIC);
        changed.extend_from_slice(&CODEC_VERSION.to_le_bytes());
        changed.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        changed.extend_from_slice(&2_u32.to_le_bytes());
        changed.extend_from_slice(&metadata);
        changed.extend_from_slice(&encoded[HEADER_BYTES + metadata_len..]);
        assert!(decode(&changed).is_err());
    }

    #[test]
    fn rejects_malformed_metadata_and_inconsistent_point_counts() {
        let mut malformed = encode(&layer()).unwrap();
        malformed[HEADER_BYTES] = 0xff;
        assert!(decode(&malformed).is_err());

        let mut inconsistent = encode(&layer()).unwrap();
        let metadata_len = header_u32(&inconsistent, 12).unwrap() as usize;
        let metadata = &mut inconsistent[HEADER_BYTES..HEADER_BYTES + metadata_len];
        let count = metadata
            .windows(b"\"pointCount\":2".len())
            .position(|window| window == b"\"pointCount\":2")
            .expect("fixture metadata contains its point count");
        metadata[count + b"\"pointCount\":".len()] = b'1';
        assert!(decode(&inconsistent).is_err());

        let mut invalid_enum = encode(&layer()).unwrap();
        let metadata_len = header_u32(&invalid_enum, 12).unwrap() as usize;
        let metadata = &mut invalid_enum[HEADER_BYTES..HEADER_BYTES + metadata_len];
        let tool = metadata
            .windows(b"\"tool\":\"pen\"".len())
            .position(|window| window == b"\"tool\":\"pen\"")
            .expect("fixture metadata contains its tool");
        metadata[tool + b"\"tool\":\"".len()..tool + b"\"tool\":\"".len() + 3]
            .copy_from_slice(b"bad");
        assert!(decode(&invalid_enum).is_err());
    }

    #[test]
    fn rejects_unsupported_schema_versions() {
        for schema_version in [0, SCHEMA_VERSION + 1] {
            let mut invalid = layer();
            invalid.schema_version = schema_version;
            assert!(encode(&invalid).is_err());
        }

        let mut future = encode(&layer()).unwrap();
        let metadata_len = header_u32(&future, 12).unwrap() as usize;
        let metadata = &mut future[HEADER_BYTES..HEADER_BYTES + metadata_len];
        let schema = metadata
            .windows(b"\"schemaVersion\":3".len())
            .position(|window| window == b"\"schemaVersion\":3")
            .expect("fixture metadata contains its schema version");
        metadata[schema + b"\"schemaVersion\":".len()] = b'4';
        assert!(decode(&future).is_err());
    }

    #[test]
    fn rejects_non_finite_and_out_of_range_samples() {
        let mut encoded = encode(&layer()).unwrap();
        let metadata_len = header_u32(&encoded, 12).unwrap() as usize;
        let payload = HEADER_BYTES + metadata_len;
        encoded[payload..payload + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&encoded).is_err());

        let mut invalid_pressure = layer();
        invalid_pressure.strokes[0].points[0].pressure = 1.01;
        assert!(encode(&invalid_pressure).is_err());
    }
}
