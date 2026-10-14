use std::{
    env, fs,
    hint::black_box,
    time::{Duration, Instant},
};

use goodtype_core::{
    DEFAULT_INK_Z_INDEX, InkLayer, InkLayerReference, NotebookManifest, Page, PageBackground,
    PageDefaults, PageGeometry, PageReference, SCHEMA_VERSION, Stroke, StrokePattern, StrokePoint,
    StrokeTool, Transform, ink_codec, layout,
    storage::{
        NotebookHistory, NotebookSnapshot, PagePosition, commit_notebook, create_notebook,
        create_page, observe_notebook, observe_page, open_notebook, open_page, redo_notebook,
        undo_notebook,
    },
};
use miniz_oxide::{deflate::compress_to_vec_zlib, inflate::decompress_to_vec_zlib};
use serde::Serialize;
use tempfile::TempDir;

const STANDARD_STROKES: usize = 5_000;
const STANDARD_POINTS: usize = 60;

#[derive(Clone, Copy)]
struct Config {
    strokes: usize,
    points: usize,
    pages: usize,
    warmup: usize,
    repeats: usize,
    compression_level: u8,
    codecs: bool,
    storage: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            strokes: STANDARD_STROKES,
            points: STANDARD_POINTS,
            pages: 3,
            warmup: 1,
            repeats: 7,
            compression_level: 6,
            codecs: true,
            storage: true,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryIntent<'a> {
    version: u32,
    previous: &'a NotebookSnapshot,
    candidate: &'a NotebookSnapshot,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BinaryRecoverySnapshot {
    snapshot: NotebookSnapshot,
    ink_layer_lengths: Vec<u32>,
}

#[derive(Serialize)]
struct BinaryRecoveryMetadata {
    previous: BinaryRecoverySnapshot,
    candidate: BinaryRecoverySnapshot,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ink storage benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_config()?;
    let total_points = config
        .strokes
        .checked_mul(config.points)
        .ok_or("stroke × point count overflow")?;
    if config.strokes == 0 || config.points == 0 || config.pages == 0 || config.repeats == 0 {
        return Err("strokes, points, pages, and repeats must be greater than zero".into());
    }
    if config.strokes > 20_000 || total_points > 750_000 {
        return Err(
            "fixture exceeds the current 20,000-stroke or 750,000-point layer limit".into(),
        );
    }

    println!("Goodtype ink-storage benchmark");
    println!(
        "profile: {} strokes × {} points = {} points; {} pages; {} warmup; {} measured repeats",
        config.strokes, config.points, total_points, config.pages, config.warmup, config.repeats
    );
    println!(
        "mode: release={} (run with `cargo bench -p goodtype-core --bench ink_storage` for valid timings)",
        !cfg!(debug_assertions)
    );
    println!(
        "timings: wall-clock on this machine; no pen, renderer, export, or memory measurement\n"
    );

    let layer = handwriting_layer("page-001", config.strokes, config.points);
    if config.codecs {
        codec_benchmarks(&layer, config)?;
    }
    if config.storage {
        storage_benchmarks(&layer, config)?;
    }
    Ok(())
}

fn codec_benchmarks(layer: &InkLayer, config: Config) -> Result<(), String> {
    println!("codec microbenchmarks (wall clock; allocations included; no notebook I/O)");
    let json = serde_json::to_vec(layer).map_err(|error| error.to_string())?;
    let binary = ink_codec::encode(layer).map_err(|error| error.to_string())?;
    let compressed = compress_to_vec_zlib(&json, config.compression_level);

    verify_json(&json)?;
    verify_binary(&json, &binary)?;
    verify_compressed(&json, &compressed)?;
    if ink_codec::encoded_len(layer).map_err(|error| error.to_string())? != binary.len() {
        return Err("production codec encoded_len disagrees with encode".into());
    }

    println!("  encoded bytes");
    println!("    compact JSON:       {:>12}  (100.0%)", json.len());
    println!(
        "    production .ink:   {:>12}  ({:>5.1}%)",
        binary.len(),
        percent(binary.len(), json.len())
    );
    println!(
        "    DEFLATE JSON (L{}): {:>12}  ({:>5.1}%)",
        config.compression_level,
        compressed.len(),
        percent(compressed.len(), json.len())
    );

    report(
        "compact JSON encode",
        sample(config, || {
            black_box(serde_json::to_vec(black_box(layer)).unwrap());
        }),
    );
    report(
        "compact JSON decode",
        sample(config, || {
            black_box(serde_json::from_slice::<InkLayer>(black_box(&json)).unwrap());
        }),
    );
    report(
        "production .ink encode",
        sample(config, || {
            black_box(ink_codec::encode(black_box(layer)).unwrap());
        }),
    );
    report(
        "production .ink decode",
        sample(config, || {
            black_box(ink_codec::decode(black_box(&binary)).unwrap());
        }),
    );
    report(
        "DEFLATE existing JSON",
        sample(config, || {
            black_box(compress_to_vec_zlib(
                black_box(&json),
                config.compression_level,
            ));
        }),
    );
    report(
        "inflate + JSON decode",
        sample(config, || {
            let bytes = decompress_to_vec_zlib(black_box(&compressed)).unwrap();
            black_box(serde_json::from_slice::<InkLayer>(&bytes).unwrap());
        }),
    );
    println!();
    Ok(())
}

fn storage_benchmarks(layer: &InkLayer, config: Config) -> Result<(), String> {
    println!("current Rust storage path (real temporary notebook I/O)");
    println!(
        "  commit includes validation, current binary recovery-intent write/removal, canonical writes, fingerprinting, and undo snapshot accounting"
    );

    let create_roots = prepare_empty_roots(config.warmup + config.repeats)?;
    let mut create_index = 0;
    report_result(
        "create/save dense notebook",
        sample_result(config, || {
            let result = create_notebook(
                &create_roots[create_index].path().join("notebook"),
                &snapshot(layer.clone()),
            );
            create_index += 1;
            result
        }),
    )?;

    let opened = tempfile::tempdir().map_err(|error| error.to_string())?;
    let opened_root = opened.path().join("notebook");
    create_notebook(&opened_root, &snapshot(layer.clone())).map_err(|error| error.to_string())?;
    report_result(
        "open dense page",
        sample_result(config, || open_notebook(&opened_root)),
    )?;

    let committed = tempfile::tempdir().map_err(|error| error.to_string())?;
    let committed_root = committed.path().join("notebook");
    create_notebook(&committed_root, &snapshot(layer.clone()))
        .map_err(|error| error.to_string())?;
    let mut history = NotebookHistory::default();
    let mut current =
        observe_notebook(&committed_root, &mut history).map_err(|error| error.to_string())?;
    let mut toggle = false;
    let commit_samples = sample_result(config, || -> Result<(), String> {
        current.ink_layers[0].points_first_mut()?.x = if toggle { 1.25 } else { 1.5 };
        toggle = !toggle;
        let result = commit_notebook(&committed_root, &mut history, current.clone())
            .map_err(|error| error.to_string())?;
        current = result.snapshot;
        Ok(())
    });
    report_result("commit dense page", commit_samples)?;

    let history_root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let history_path = history_root.path().join("notebook");
    create_notebook(&history_path, &snapshot(layer.clone())).map_err(|error| error.to_string())?;
    let mut undo_history = NotebookHistory::default();
    let mut changed =
        observe_notebook(&history_path, &mut undo_history).map_err(|error| error.to_string())?;
    changed.ink_layers[0]
        .points_first_mut()
        .map_err(|error| error.to_string())?
        .x += 0.25;
    commit_notebook(&history_path, &mut undo_history, changed)
        .map_err(|error| error.to_string())?;
    report_result(
        "undo dense page",
        sample_result(
            config,
            || -> Result<(), goodtype_core::storage::StorageError> {
                let result = undo_notebook(&history_path, &mut undo_history)?;
                black_box(result.snapshot);
                let result = redo_notebook(&history_path, &mut undo_history)?;
                black_box(result.snapshot);
                Ok(())
            },
        )
        .map(|mut pairs| {
            for duration in &mut pairs {
                *duration /= 2;
            }
            pairs
        }),
    )?;
    println!(
        "    note: undo samples are half of an undo+redo pair; redo uses the same current save path"
    );

    let recovery_roots = prepare_recovery_notebooks(layer, config.warmup + config.repeats, false)?;
    let mut recovery_index = 0;
    let recovery_samples = sample_result(
        config,
        || -> Result<NotebookSnapshot, goodtype_core::storage::StorageError> {
            let recovered = open_notebook(&recovery_roots[recovery_index].path().join("notebook"))?;
            recovery_index += 1;
            Ok(recovered)
        },
    );
    report_result("legacy JSON intent recovery", recovery_samples)?;
    let recovery_roots = prepare_recovery_notebooks(layer, config.warmup + config.repeats, true)?;
    let mut recovery_index = 0;
    let recovery_samples = sample_result(
        config,
        || -> Result<NotebookSnapshot, goodtype_core::storage::StorageError> {
            let recovered = open_notebook(&recovery_roots[recovery_index].path().join("notebook"))?;
            recovery_index += 1;
            Ok(recovered)
        },
    );
    report_result("current binary intent recovery", recovery_samples)?;
    println!(
        "    note: exercises the real public open path restoring the previous snapshot and archiving the candidate"
    );

    let multi = prepare_multi_page(layer, config.pages)?;
    let page_ids: Vec<String> = (0..config.pages)
        .map(|index| format!("page-{:03}", index + 1))
        .collect();
    report_result(
        "open all pages (one sweep)",
        sample_result(
            config,
            || -> Result<(), goodtype_core::storage::StorageError> {
                for page_id in &page_ids {
                    black_box(open_page(&multi.1, page_id)?);
                }
                Ok(())
            },
        ),
    )?;
    println!(
        "    note: {} pages × {} strokes; each page is loaded independently through open_page",
        config.pages, config.strokes
    );
    println!();
    Ok(())
}

trait FirstPoint {
    fn points_first_mut(&mut self) -> Result<&mut StrokePoint, &'static str>;
}

impl FirstPoint for InkLayer {
    fn points_first_mut(&mut self) -> Result<&mut StrokePoint, &'static str> {
        self.strokes
            .first_mut()
            .and_then(|stroke| stroke.points.first_mut())
            .ok_or("fixture has no point")
    }
}

fn prepare_recovery_notebooks(
    layer: &InkLayer,
    count: usize,
    binary: bool,
) -> Result<Vec<TempDir>, String> {
    let mut roots = Vec::with_capacity(count);
    for _ in 0..count {
        let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot(layer.clone())).map_err(|error| error.to_string())?;
        let previous = open_notebook(&root).map_err(|error| error.to_string())?;
        let mut candidate = previous.clone();
        candidate.page.revision += 1;
        candidate.ink_layers[0].points_first_mut()?.x += 0.25;
        let bytes = if binary {
            encode_recovery_intent(&previous, &candidate)?
        } else {
            serde_json::to_vec(&RecoveryIntent {
                version: 1,
                previous: &previous,
                candidate: &candidate,
            })
            .map_err(|error| error.to_string())?
        };
        let metadata = root.join(".goodtype");
        fs::create_dir_all(&metadata).map_err(|error| error.to_string())?;
        fs::write(
            metadata.join(if binary {
                "pending-transaction.bin"
            } else {
                "pending-transaction.json"
            }),
            bytes,
        )
        .map_err(|error| error.to_string())?;
        roots.push(temporary);
    }
    Ok(roots)
}

fn encode_recovery_intent(
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<Vec<u8>, String> {
    fn metadata(snapshot: &NotebookSnapshot) -> Result<BinaryRecoverySnapshot, String> {
        let lengths = snapshot
            .ink_layers
            .iter()
            .map(|layer| {
                u32::try_from(ink_codec::encoded_len(layer).map_err(|error| error.to_string())?)
                    .map_err(|_| "encoded ink layer exceeds recovery length field".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(BinaryRecoverySnapshot {
            snapshot: NotebookSnapshot {
                manifest: snapshot.manifest.clone(),
                page: snapshot.page.clone(),
                blocks: snapshot.blocks.clone(),
                assets: Vec::new(),
                ink_layers: Vec::new(),
            },
            ink_layer_lengths: lengths,
        })
    }

    let json = serde_json::to_vec(&BinaryRecoveryMetadata {
        previous: metadata(previous)?,
        candidate: metadata(candidate)?,
    })
    .map_err(|error| error.to_string())?;
    let mut bytes = Vec::with_capacity(16 + json.len());
    bytes.extend_from_slice(b"GTREC\0\r\n");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(json.len())
            .map_err(|_| "recovery metadata exceeds length field")?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&json);
    for layer in previous.ink_layers.iter().chain(&candidate.ink_layers) {
        bytes.extend_from_slice(&ink_codec::encode(layer).map_err(|error| error.to_string())?);
    }
    Ok(bytes)
}

fn prepare_empty_roots(count: usize) -> Result<Vec<TempDir>, String> {
    (0..count)
        .map(|_| tempfile::tempdir().map_err(|error| error.to_string()))
        .collect()
}

fn prepare_multi_page(
    layer: &InkLayer,
    pages: usize,
) -> Result<(TempDir, std::path::PathBuf), String> {
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = temporary.path().join("notebook");
    create_notebook(&root, &snapshot(layer.clone())).map_err(|error| error.to_string())?;
    for index in 1..pages {
        let page = create_page(
            &root,
            "2026-09-07T00:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
        let page_id = format!("page-{:03}", index + 1);
        if page.page.id != page_id {
            return Err(format!("unexpected generated page id: {}", page.page.id));
        }
        let mut history = NotebookHistory::default();
        let mut observed =
            observe_page(&root, &page_id, &mut history).map_err(|error| error.to_string())?;
        let mut page_layer =
            handwriting_layer(&page_id, layer.strokes.len(), layer.strokes[0].points.len());
        page_layer.id = observed.ink_layers[0].id.clone();
        observed.ink_layers = vec![page_layer];
        commit_notebook(&root, &mut history, observed).map_err(|error| error.to_string())?;
    }
    Ok((temporary, root))
}

fn sample(mut config: Config, mut operation: impl FnMut()) -> Vec<Duration> {
    for _ in 0..config.warmup {
        operation();
    }
    config.warmup = 0;
    (0..config.repeats)
        .map(|_| {
            let start = Instant::now();
            operation();
            start.elapsed()
        })
        .collect()
}

fn sample_result<T, E>(
    config: Config,
    mut operation: impl FnMut() -> Result<T, E>,
) -> Result<Vec<Duration>, E> {
    for _ in 0..config.warmup {
        black_box(operation()?);
    }
    let mut samples = Vec::with_capacity(config.repeats);
    for _ in 0..config.repeats {
        let start = Instant::now();
        black_box(operation()?);
        samples.push(start.elapsed());
    }
    Ok(samples)
}

fn report_result<E: ToString>(name: &str, samples: Result<Vec<Duration>, E>) -> Result<(), String> {
    report(name, samples.map_err(|error| error.to_string())?);
    Ok(())
}

fn report(name: &str, mut samples: Vec<Duration>) {
    samples.sort_unstable();
    let middle = samples.len() / 2;
    let median_ms = if samples.len().is_multiple_of(2) {
        (ms(samples[middle - 1]) + ms(samples[middle])) / 2.0
    } else {
        ms(samples[middle])
    };
    let p95 = samples[((samples.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)];
    let worst = samples[samples.len() - 1];
    println!(
        "  {name:<29} median {:>9.3} ms  p95 {:>9.3} ms  worst {:>9.3} ms",
        median_ms,
        ms(p95),
        ms(worst)
    );
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}
fn percent(value: usize, baseline: usize) -> f64 {
    value as f64 / baseline as f64 * 100.0
}

fn parse_config() -> Result<Config, String> {
    let mut config = Config::default();
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        let value = |args: &mut std::iter::Skip<std::env::Args>| {
            args.next()
                .ok_or_else(|| format!("missing value after {argument}"))
        };
        match argument.as_str() {
            "--strokes" => {
                config.strokes = value(&mut args)?.parse().map_err(|_| "invalid --strokes")?
            }
            "--points" => {
                config.points = value(&mut args)?.parse().map_err(|_| "invalid --points")?
            }
            "--pages" => config.pages = value(&mut args)?.parse().map_err(|_| "invalid --pages")?,
            "--warmup" => {
                config.warmup = value(&mut args)?.parse().map_err(|_| "invalid --warmup")?
            }
            "--repeats" => {
                config.repeats = value(&mut args)?.parse().map_err(|_| "invalid --repeats")?
            }
            "--compression-level" => {
                config.compression_level = value(&mut args)?
                    .parse()
                    .map_err(|_| "invalid --compression-level")?
            }
            "--codec-only" => config.storage = false,
            "--storage-only" => config.codecs = false,
            // Cargo supplies this marker to harness-free benchmark executables.
            "--bench" => {}
            "--quick" => {
                config.strokes = 100;
                config.points = 20;
                config.pages = 2;
                config.warmup = 0;
                config.repeats = 2;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    if config.compression_level > 10 {
        return Err("--compression-level must be 0..=10".into());
    }
    Ok(config)
}

fn print_help() {
    println!(
        "Usage: cargo bench -p goodtype-core --bench ink_storage -- [options]\n\nOptions:\n  --strokes N            strokes per dense layer (default 5000)\n  --points N             points per stroke (default 60)\n  --pages N              pages in the open sweep (default 3)\n  --warmup N             unreported warmups (default 1)\n  --repeats N            measured repeats (default 7)\n  --compression-level N  miniz DEFLATE level 0..10 (default 6)\n  --codec-only           skip current storage path\n  --storage-only         skip codec microbenchmarks\n  --quick                100×20, 2 pages, no warmup, 2 repeats\n"
    );
}

fn snapshot(layer: InkLayer) -> NotebookSnapshot {
    NotebookSnapshot {
        manifest: NotebookManifest {
            schema_version: SCHEMA_VERSION,
            id: "benchmark-notebook".into(),
            title: "Ink storage benchmark".into(),
            pages: vec![PageReference {
                id: "page-001".into(),
                path: layout::page_path("page-001"),
                geometry: geometry(),
            }],
            default_page: PageDefaults {
                geometry: geometry(),
                background: PageBackground::Plain {
                    color: "#ffffff".into(),
                },
            },
            shared_style_path: None,
            created_at: "2026-09-07T00:00:00Z".into(),
            modified_at: "2026-09-07T00:00:00Z".into(),
        },
        page: Page {
            schema_version: SCHEMA_VERSION,
            id: "page-001".into(),
            revision: 1,
            geometry: geometry(),
            background: PageBackground::Plain {
                color: "#ffffff".into(),
            },
            objects: Vec::new(),
            reading_order: Vec::new(),
            ink_layers: vec![InkLayerReference {
                id: "ink-layer-page-001".into(),
                path: layout::ink_layer_path("page-001", 1),
            }],
        },
        blocks: Vec::new(),
        assets: Vec::new(),
        ink_layers: vec![layer],
    }
}

fn geometry() -> PageGeometry {
    PageGeometry {
        width_pt: 595.0,
        height_pt: 842.0,
    }
}

fn handwriting_layer(page_id: &str, strokes: usize, points_per_stroke: usize) -> InkLayer {
    InkLayer {
        schema_version: SCHEMA_VERSION,
        id: format!("ink-layer-{page_id}"),
        page_id: page_id.into(),
        strokes: (0..strokes)
            .map(|stroke| Stroke {
                id: format!("{page_id}-stroke-{stroke:06}"),
                z_index: DEFAULT_INK_Z_INDEX,
                tool: if stroke % 19 == 0 {
                    StrokeTool::Highlighter
                } else {
                    StrokeTool::Pen
                },
                color: match stroke % 4 {
                    0 => "#101418",
                    1 => "#1f5fbf",
                    2 => "#9b2c2c",
                    _ => "#2f6b3f",
                }
                .into(),
                width_pt: if stroke % 19 == 0 { 9.0 } else { 1.6 },
                pattern: match stroke % 29 {
                    0 => StrokePattern::Dotted,
                    1 => StrokePattern::Dashed,
                    _ => StrokePattern::Solid,
                },
                pattern_spacing: 1.0,
                pressure: stroke % 7 != 0,
                taper: if stroke % 11 == 0 { 0.12 } else { 0.0 },
                opacity: if stroke % 19 == 0 { 0.32 } else { 1.0 },
                group_id: None,
                points: (0..points_per_stroke)
                    .map(|point| {
                        let step = (stroke * points_per_stroke + point) as f64;
                        StrokePoint {
                            x: ((step * 7.31) % 59500.0).round() / 100.0,
                            y: ((step * 11.17) % 84200.0).round() / 100.0,
                            pressure: ((step % 1000.0) / 1000.0 * 1000.0).round() / 1000.0,
                            time_ms: (step * 83.0).round() / 10.0,
                            tilt_x: ((step % 900.0) / 10.0 - 45.0).round(),
                            tilt_y: ((step % 700.0) / 10.0 - 35.0).round(),
                        }
                    })
                    .collect(),
                transform: Transform {
                    translate_x: 0.0,
                    translate_y: 0.0,
                    scale_x: 1.0,
                    scale_y: 1.0,
                    rotation: 0.0,
                },
            })
            .collect(),
    }
}

fn verify_json(bytes: &[u8]) -> Result<(), String> {
    let decoded: InkLayer = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let roundtrip = serde_json::to_vec(&decoded).map_err(|error| error.to_string())?;
    if roundtrip == bytes {
        Ok(())
    } else {
        Err("compact JSON changed values on round trip".into())
    }
}

fn verify_binary(json: &[u8], bytes: &[u8]) -> Result<(), String> {
    let decoded = ink_codec::decode(bytes).map_err(|error| error.to_string())?;
    if serde_json::to_vec(&decoded).map_err(|error| error.to_string())? == json {
        Ok(())
    } else {
        Err("production .ink codec changed values on round trip".into())
    }
}

fn verify_compressed(json: &[u8], bytes: &[u8]) -> Result<(), String> {
    let inflated = decompress_to_vec_zlib(bytes)
        .map_err(|error| format!("DEFLATE decode failed: {error:?}"))?;
    if inflated != json {
        return Err("compressed JSON changed bytes on inflate".into());
    }
    verify_json(&inflated)
}
