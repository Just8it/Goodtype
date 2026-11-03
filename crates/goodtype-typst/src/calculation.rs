//! One static layout authority for computational-page display and export (ADR 0042).
//!
//! Saved cells and saved outputs become generated Typst content: code and text stay literal
//! strings, never Typst markup, and nothing here evaluates notebook code. Display and export
//! use the same prepared content, the same fonts and the same measured cell positions, so a
//! preview shows exactly what the PDF contains.
use std::path::Path;

use base64::{Engine, engine::general_purpose::STANDARD};
use goodtype_core::calculation::{A4_WIDTH_PT, Calculation, Cell, source_hash};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use serde::Serialize;
use serde_json::Value;
use typst_layout::PagedDocument;
use unicode_width::UnicodeWidthChar;

use crate::embedded::{BlockWorld, diagnostic_messages};
use crate::export::typst_string;

/// Page padding on every side, in points. The display surface must not add its own.
pub const PADDING_PT: f64 = 18.0;
/// Vertical space between consecutive cells. There is no gap after the last cell.
pub const CELL_GAP_PT: f64 = 14.0;
/// Content height of a part that has no cells.
pub const EMPTY_CONTENT_PT: f64 = 24.0;
pub const CONTENT_WIDTH_PT: f64 = A4_WIDTH_PT - 2.0 * PADDING_PT;
pub const A4_HEIGHT_PT: f64 = 297.0 * 72.0 / 25.4;
/// Conservative portable single-page height guard for preserved-height export. It equals the
/// PDF 1.x default-user-space page-size ceiling of 200 inches. It is not a measured universal
/// viewer limit; taller pages need explicit A4 splitting.
pub const PORTABLE_PAGE_HEIGHT_PT: f64 = 14_400.0;

const MAX_LAYOUT_HEIGHT_PT: f64 = 100_000.0;
const MAX_SVG_BYTES: usize = 32 * 1024 * 1024;
/// Displayed source and output lines per part, after wrapping, checked before compilation.
const MAX_DISPLAY_LINES: usize = 10_000;
const WRAP_COLUMNS: usize = 82;
const OUTPUT_INSET_X_PT: f64 = 10.0;
const OUTPUT_INSET_Y_PT: f64 = 6.0;
/// Space between consecutive outputs of one cell (printed text, a value, a plot). Without it
/// the global zero block spacing lets one output's last line touch the next one's first.
const OUTPUT_GAP_PT: f64 = 6.0;
/// Export colours of a failed cell's output. The dark display theme maps both by value.
const ERROR_FILL: &str = "#fbeeed";
const ERROR_TEXT: &str = "#a43b36";
const MAX_PNG_BYTES: usize = 10 * 1024 * 1024;
const MAX_PNG_SIDE: u32 = 16_384;
const MAX_PNG_DECODED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PART_DECODED_BYTES: u64 = 192 * 1024 * 1024;
/// Saved PNG pixels are CSS pixels, as Jupyter front ends display them: 96 per inch.
const PT_PER_CSS_PX: f64 = 0.75;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellLayout {
    pub id: String,
    /// Page-point offset of the cell's top edge from the page top, padding included.
    pub top_pt: f64,
    pub source_svg: String,
    pub output_svg: Option<String>,
    pub source_height_pt: f64,
    pub output_height_pt: f64,
    pub stale: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculationLayout {
    pub revision: u64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub padding_pt: f64,
    pub cell_gap_pt: f64,
    pub cells: Vec<CellLayout>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportCalculation {
    pub document: Calculation,
    pub part_id: String,
    pub german: bool,
    pub split_a4: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalculationError {
    /// Preserved-height export would exceed [`PORTABLE_PAGE_HEIGHT_PT`]. Select A4 splitting.
    TooTall {
        height_pt: f64,
        limit_pt: f64,
    },
    Invalid(String),
}

impl std::fmt::Display for CalculationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooTall {
                height_pt,
                limit_pt,
            } => write!(
                formatter,
                "calculation page is {height_pt:.0} pt tall; preserved-height export supports {limit_pt:.0} pt. Select A4 splitting"
            ),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl From<String> for CalculationError {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}

impl From<&str> for CalculationError {
    fn from(message: &str) -> Self {
        Self::Invalid(message.to_owned())
    }
}

fn literal(text: &str) -> String {
    format!("\"{}\"", typst_string(&visible(text)))
}

/// Show invisible control and direction-override characters instead of letting them act.
fn visible(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\n' | '\t' => result.push(character),
            '\u{0}'..='\u{1f}' => {
                result.push(char::from_u32(0x2400 + character as u32).unwrap_or('\u{fffd}'))
            }
            '\u{7f}' => result.push('\u{2421}'),
            '\u{80}'..='\u{9f}' => result.push('\u{fffd}'),
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => {
                result.push_str(&format!("[U+{:04X}]", character as u32));
            }
            _ => result.push(character),
        }
    }
    result
}

/// Terminal text as a terminal shows it: escape sequences removed, carriage returns
/// overwrite from the line start. Tracebacks and progress output use both.
fn terminal_text(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            plain.push(character);
            continue;
        }
        match characters.next() {
            Some('[') => {
                while let Some(&next) = characters.peek() {
                    characters.next();
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(next) = characters.next() {
                    if next == '\u{7}'
                        || (next == '\u{1b}' && characters.next_if_eq(&'\\').is_some())
                    {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    plain
        .replace("\r\n", "\n")
        .split('\n')
        .map(|line| line.rsplit('\r').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wrap at a fixed column count with a visible continuation marker. Display only; the saved
/// source bytes never change. Returns the wrapped text and its displayed line count.
fn wrapped(text: &str) -> (String, usize) {
    let mut result = String::with_capacity(text.len());
    let mut lines = 1;
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            result.push('\n');
            lines += 1;
        }
        let mut width = 0;
        for character in line.chars() {
            let columns = if character == '\t' {
                4 - width % 4
            } else {
                character.width().unwrap_or(1)
            };
            if width + columns > WRAP_COLUMNS {
                result.push_str("\n\u{21aa} ");
                lines += 1;
                width = 2;
            }
            if character == '\t' {
                result.push_str(&" ".repeat(columns));
            } else {
                result.push(character);
            }
            width += columns;
        }
    }
    (result, lines)
}

/// Prose has no fixed columns, and Typst does not break inside a word. Give every long
/// unbroken token a zero-width break opportunity so it wraps instead of leaving the page.
fn breakable(text: &str) -> String {
    const RUN: usize = 40;
    let mut result = String::with_capacity(text.len());
    let mut run = 0;
    for character in text.chars() {
        if character.is_whitespace() {
            run = 0;
        } else if run == RUN {
            result.push('\u{200b}');
            run = 0;
        }
        if !character.is_whitespace() {
            run += 1;
        }
        result.push(character);
    }
    result
}

fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

struct PngInfo {
    width: u32,
    height: u32,
    decoded_bytes: u64,
}

/// Read the PNG header and bound the decoded size before any decoder sees the data.
fn png_info(bytes: &[u8]) -> Result<PngInfo, String> {
    if bytes.len() < 33
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || &bytes[8..16] != b"\0\0\0\x0dIHDR"
    {
        return Err("saved image is not a valid PNG".into());
    }
    let word = |offset: usize| u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let (width, height, depth, color) = (word(16), word(20), bytes[24], bytes[25]);
    let valid_depth = match color {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        2 | 4 | 6 => matches!(depth, 8 | 16),
        _ => false,
    };
    if !valid_depth || width == 0 || height == 0 {
        return Err("saved PNG has an invalid header".into());
    }
    if width > MAX_PNG_SIDE || height > MAX_PNG_SIDE {
        return Err(format!(
            "saved PNG is {width} x {height} pixels; the limit is {MAX_PNG_SIDE} per side"
        ));
    }
    // Upper bound: decoders may expand every pixel to RGBA at the sample depth.
    let decoded_bytes = u64::from(width) * u64::from(height) * 4 * if depth == 16 { 2 } else { 1 };
    if decoded_bytes > MAX_PNG_DECODED_BYTES {
        return Err("saved PNG exceeds the decoded image size limit".into());
    }
    Ok(PngInfo {
        width,
        height,
        decoded_bytes,
    })
}

/// Per-part limits, applied while generating content and before compilation.
#[derive(Default)]
struct Budget {
    lines: usize,
    decoded_bytes: u64,
}

impl Budget {
    fn lines(&mut self, count: usize) -> Result<(), String> {
        self.lines += count;
        if self.lines > MAX_DISPLAY_LINES {
            return Err(format!(
                "calculation part has more than {MAX_DISPLAY_LINES} displayed lines; it stays saved but cannot be laid out"
            ));
        }
        Ok(())
    }

    fn raw(&mut self, text: &str, language: &str) -> Result<String, String> {
        let (text, lines) = wrapped(text);
        self.lines(lines)?;
        Ok(format!(
            "#raw({}, block: true, lang: {})\n",
            literal(&text),
            literal(language)
        ))
    }
}

fn markdown(text: &str, budget: &mut Budget) -> Result<String, String> {
    let mut result = String::new();
    // Code and HTML blocks collect their text and become one literal block.
    let mut literal_block: Option<(String, String)> = None;
    for event in Parser::new_ext(text, Options::ENABLE_MATH) {
        if let Some((language, buffer)) = &mut literal_block {
            match event {
                Event::Text(text) | Event::Html(text) => buffer.push_str(&text),
                Event::End(TagEnd::CodeBlock | TagEnd::HtmlBlock) => {
                    let block = budget.raw(buffer.trim_end_matches('\n'), language)?;
                    result.push_str(&format!("#block(width: 100%, inset: 6pt, fill: rgb(\"#f7f8f9\"), below: 6pt)[{block}]\n"));
                    literal_block = None;
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                // The language selects highlighting only; restrict it to a plain token.
                let language = if language.len() <= 32
                    && language
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"+-_#.".contains(&byte))
                {
                    language
                } else {
                    String::new()
                };
                literal_block = Some((language, String::new()));
            }
            Event::Start(Tag::HtmlBlock) => literal_block = Some((String::new(), String::new())),
            Event::Start(tag) => result.push_str(&match tag {
                Tag::Paragraph => "#block(breakable: true, below: 6pt)[".into(),
                Tag::Heading { level, .. } => {
                    format!("#block(below: 6pt)[#heading(level: {})[", level as u8)
                }
                Tag::Emphasis => "#emph[".into(),
                Tag::Strong => "#strong[".into(),
                Tag::BlockQuote(_) => "#quote(block: true)[".into(),
                Tag::List(Some(start)) => format!("#block(below: 6pt)[#enum(start: {start},"),
                Tag::List(None) => "#block(below: 6pt)[#list(".into(),
                Tag::Item => "[".into(),
                _ => String::new(),
            }),
            Event::End(tag) => result.push_str(match tag {
                TagEnd::Paragraph | TagEnd::BlockQuote(_) => "]\n",
                TagEnd::Heading(_) => "]]\n",
                TagEnd::Emphasis | TagEnd::Strong => "]",
                TagEnd::List(_) => ")]\n",
                TagEnd::Item => "],",
                _ => "",
            }),
            Event::Text(text) => {
                budget.lines(text.matches('\n').count())?;
                result.push_str(&format!("#text({})", literal(&breakable(&text))));
            }
            Event::Code(text) => result.push_str(&format!("#raw({})", literal(&breakable(&text)))),
            Event::SoftBreak => result.push(' '),
            Event::HardBreak => {
                budget.lines(1)?;
                result.push_str("#linebreak()");
            }
            Event::Rule => result.push_str("#block(below: 6pt)[#line(length: 100%)]\n"),
            // No LaTeX math renderer is accepted. Math stays literal, visible source.
            Event::InlineMath(text) => result.push_str(&format!(
                "#raw({})",
                literal(&breakable(&format!("${text}$")))
            )),
            Event::DisplayMath(text) => result.push_str(&budget.raw(&format!("$${text}$$"), "")?),
            // Inline HTML stays inert, inspectable source text.
            Event::InlineHtml(text) => {
                result.push_str(&format!("#raw({})", literal(&breakable(&text))))
            }
            _ => {}
        }
    }
    Ok(result)
}

struct Labels {
    no_static: &'static str,
    stale: &'static str,
    copied: &'static str,
    last_success: &'static str,
    scaled: &'static str,
}

fn labels(german: bool) -> Labels {
    if german {
        Labels {
            no_static: "Keine unterstützte statische Darstellung.",
            stale: "Code nach dieser Ausgabe geändert.",
            copied: "Kopiert. Hier noch nicht ausgeführt.",
            last_success: "Letzte erfolgreiche Ausgabe:",
            scaled: "Skaliert auf",
        }
    } else {
        Labels {
            no_static: "No supported static display.",
            stale: "Code or earlier cells changed after this output.",
            copied: "Copied. Not run here.",
            last_success: "Last successful output:",
            scaled: "Scaled to",
        }
    }
}

struct Images<'a> {
    prefix: &'a str,
    files: Vec<(String, Vec<u8>)>,
    /// Rendered heights, which A4 splitting must keep on one sheet.
    heights: Vec<f64>,
    max_height: Option<f64>,
}

fn output_items(
    outputs: &[Value],
    labels: &Labels,
    budget: &mut Budget,
    images: &mut Images,
) -> Result<String, String> {
    let mut output = String::new();
    for item in outputs {
        let data = item.get("data");
        if let Some(encoded) = data.and_then(|data| data.get("image/png")) {
            let mut encoded = text(Some(encoded));
            if encoded.len() > MAX_PNG_BYTES / 3 * 4 + 4096 {
                return Err("saved PNG exceeds the image size limit".into());
            }
            encoded.retain(|character| !character.is_ascii_whitespace());
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| "saved PNG has invalid base64 encoding")?;
            if bytes.len() > MAX_PNG_BYTES {
                return Err("saved PNG exceeds the image size limit".into());
            }
            let info = png_info(&bytes)?;
            budget.decoded_bytes += info.decoded_bytes;
            if budget.decoded_bytes > MAX_PART_DECODED_BYTES {
                return Err("calculation part exceeds the decoded image size limit".into());
            }
            // Jupyter metadata can request a smaller display width (high-DPI figures). Never
            // display wider than the native pixels or the content width.
            let requested = item
                .get("metadata")
                .and_then(|metadata| metadata.get("image/png"))
                .and_then(|metadata| metadata.get("width"))
                .and_then(Value::as_f64)
                .filter(|width| width.is_finite() && *width > 0.0);
            let pixels = requested.map_or(f64::from(info.width), |width| {
                width.min(f64::from(info.width))
            });
            let mut width =
                (pixels * PT_PER_CSS_PX).min(CONTENT_WIDTH_PT - 2.0 * OUTPUT_INSET_X_PT);
            let mut height = width * f64::from(info.height) / f64::from(info.width);
            let scale = images
                .max_height
                .map_or(1.0, |limit| (limit / height).min(1.0));
            width *= scale;
            height *= scale;
            let note = if scale < 1.0 {
                format!(
                    "#linebreak()#text(size: 8pt, {})",
                    literal(&format!("{} {:.0}%", labels.scaled, scale * 100.0))
                )
            } else {
                String::new()
            };
            let name = format!("{}-{}.png", images.prefix, images.files.len());
            output.push_str(&format!(
                "#block(breakable: false)[#image({}, width: {width}pt, height: {height}pt){note}]\n",
                literal(&name)
            ));
            images.files.push((name, bytes));
            images.heights.push(height);
        } else if item.get("output_type").and_then(Value::as_str) == Some("error") {
            let traceback = item
                .get("traceback")
                .and_then(Value::as_array)
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let error = if traceback.is_empty() {
                format!("{}: {}", text(item.get("ename")), text(item.get("evalue")))
            } else {
                traceback
            };
            // A tinted band marks the failure. Its fill grows outward into the output inset, so
            // the error text stays aligned with the code and with ordinary output.
            output.push_str(&format!(
                "#block(width: 100%, fill: rgb(\"{ERROR_FILL}\"), radius: 3pt, inset: (y: 4pt), outset: (x: {OUTPUT_INSET_X_PT}pt))[#text(fill: rgb(\"{ERROR_TEXT}\"))[{}]]\n",
                budget.raw(&terminal_text(&error), "")?
            ));
        } else {
            let plain = if item.get("text").is_some() {
                text(item.get("text"))
            } else {
                text(data.and_then(|data| data.get("text/plain")))
            };
            let plain = terminal_text(plain.strip_suffix('\n').unwrap_or(&plain));
            if plain.is_empty() {
                output.push_str(&format!(
                    "#text(size: 8pt, fill: rgb(\"#64717c\"), {})\n",
                    literal(labels.no_static)
                ));
            } else {
                output.push_str(&budget.raw(&plain, "")?);
            }
        }
    }
    Ok(output)
}

pub(crate) struct CellSource {
    pub code: String,
    pub output: String,
    pub stale: bool,
}

fn cell_source(
    cell: &Cell,
    german: bool,
    budget: &mut Budget,
    images: &mut Images,
) -> Result<CellSource, String> {
    let labels = labels(german);
    let code = match cell.cell_type.as_str() {
        "markdown" if cell.source.trim().is_empty() => {
            format!("#block(height: {EMPTY_CONTENT_PT}pt)[]")
        }
        "markdown" => markdown(&cell.source, budget)?,
        kind => format!(
            "#block(width: 100%, inset: 10pt, fill: rgb(\"#f7f8f9\"), breakable: true)[{}]",
            budget.raw(
                if cell.source.is_empty() {
                    " "
                } else {
                    &cell.source
                },
                if kind == "code" { "python" } else { "" }
            )?
        ),
    };
    let namespace = cell.goodtype();
    let mut output = output_items(&cell.outputs, &labels, budget, images)?;
    let execution = namespace.and_then(|meta| meta.get("execution"));
    let stale = cell.cell_type == "code"
        && (namespace
            .and_then(|meta| meta.get("earlierCellsChanged"))
            .and_then(Value::as_bool)
            == Some(true)
            || execution
                .and_then(|value| value.get("sourceHash"))
                .and_then(Value::as_str)
                .is_some_and(|hash| hash != source_hash(&cell.source)));
    if stale {
        output.push_str(&format!(
            "#block(above: 6pt, below: 6pt)[#text(size: 8pt, fill: rgb(\"#885717\"), {})]\n",
            literal(labels.stale)
        ));
    }
    if let Some(last) = namespace
        .and_then(|meta| meta.get("lastSuccess"))
        .and_then(|last| last.get("outputs"))
        .and_then(Value::as_array)
        .filter(|outputs| !outputs.is_empty())
    {
        output.push_str(&format!(
            "#block(above: 6pt, below: 6pt)[#text(size: 8pt, fill: rgb(\"#64717c\"), {})]\n",
            literal(labels.last_success)
        ));
        output.push_str(&output_items(last, &labels, budget, images)?);
    }
    if namespace.and_then(|meta| meta.get("copiedFrom")).is_some() {
        output.push_str(&format!("#text(size: 8pt, {})\n", literal(labels.copied)));
    }
    if !output.is_empty() {
        output = format!(
            "#block(width: 100%, inset: (x: {OUTPUT_INSET_X_PT}pt, y: {OUTPUT_INSET_Y_PT}pt), breakable: true)[#set block(spacing: {OUTPUT_GAP_PT}pt)\n{output}]"
        );
    }
    Ok(CellSource {
        code,
        output,
        stale,
    })
}

/// Typography shared by display and export. Bundled fonts only, so a layout does not depend
/// on the fonts installed on one computer.
const TEXT_RULES: &str = "#set text(font: \"Libertinus Serif\", size: 11pt)\n\
#show raw: set text(font: \"DejaVu Sans Mono\", size: 10pt)\n\
#set par(leading: 3pt, spacing: 0pt, justify: false)\n\
#set block(above: 0pt, below: 0pt)\n";

struct Prepared {
    revision: u64,
    ids: Vec<String>,
    sources: Vec<CellSource>,
    files: Vec<(String, Vec<u8>)>,
    image_heights: Vec<f64>,
}

fn prepare(
    document: &Calculation,
    part_id: &str,
    german: bool,
    prefix: &str,
    max_image_height: Option<f64>,
) -> Result<Prepared, String> {
    document.validate().map_err(|error| error.to_string())?;
    if !document
        .parts()
        .map_err(|error| error.to_string())?
        .iter()
        .any(|part| part.id == part_id)
    {
        return Err("calculation part is unknown".into());
    }
    let mut budget = Budget::default();
    let mut images = Images {
        prefix,
        files: Vec::new(),
        heights: Vec::new(),
        max_height: max_image_height,
    };
    let mut ids = Vec::new();
    let mut sources = Vec::new();
    for cell in document
        .cells
        .iter()
        .filter(|cell| cell.part_id() == Some(part_id))
    {
        sources.push(cell_source(cell, german, &mut budget, &mut images)?);
        ids.push(cell.id.clone());
    }
    Ok(Prepared {
        revision: document.revision(),
        ids,
        sources,
        files: images.files,
        image_heights: images.heights,
    })
}

struct Measured {
    prepared: Prepared,
    document: PagedDocument,
    tops: Vec<f64>,
    source_heights: Vec<f64>,
    output_heights: Vec<f64>,
    height_pt: f64,
}

/// Compile every cell's source and output as separate auto-height pages at the content width,
/// then derive page-point positions. This is the only place where heights are decided.
fn measure(
    root: &Path,
    document: &Calculation,
    part_id: &str,
    german: bool,
    prefix: &str,
) -> Result<Measured, String> {
    measure_with_image_limit(root, document, part_id, german, prefix, None)
}

fn measure_with_image_limit(
    root: &Path,
    document: &Calculation,
    part_id: &str,
    german: bool,
    prefix: &str,
    max_image_height: Option<f64>,
) -> Result<Measured, String> {
    let prepared = prepare(document, part_id, german, prefix, max_image_height)?;
    let mut main =
        format!("#set page(width: {CONTENT_WIDTH_PT}pt, height: auto, margin: 0pt)\n{TEXT_RULES}");
    for (index, source) in prepared.sources.iter().enumerate() {
        if index > 0 {
            main.push_str("#pagebreak()\n");
        }
        main.push_str(&source.code);
        main.push_str("\n#pagebreak()\n");
        main.push_str(if source.output.is_empty() {
            "#box(height: 0pt)"
        } else {
            &source.output
        });
        main.push('\n');
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let mut world = BlockWorld::for_main("calculation.typ", main, root, false);
    for (name, bytes) in &prepared.files {
        world.insert_binary(name, bytes.clone());
    }
    let compiled = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| diagnostic_messages(&errors))?;
    let cells = prepared.sources.len();
    if cells > 0 && compiled.pages().len() != cells * 2 {
        return Err("calculation layout produced unexpected page boundaries".into());
    }
    let mut tops = Vec::with_capacity(cells);
    let mut source_heights = Vec::with_capacity(cells);
    let mut output_heights = Vec::with_capacity(cells);
    let mut y = PADDING_PT;
    for (index, source) in prepared.sources.iter().enumerate() {
        if index > 0 {
            y += CELL_GAP_PT;
        }
        let code = compiled.pages()[index * 2].frame.height().to_pt();
        let output = if source.output.is_empty() {
            0.0
        } else {
            compiled.pages()[index * 2 + 1].frame.height().to_pt()
        };
        tops.push(y);
        source_heights.push(code);
        output_heights.push(output);
        y += code + output;
        if y > MAX_LAYOUT_HEIGHT_PT {
            return Err("calculation exceeds the display layout limit".into());
        }
    }
    let height_pt = if cells == 0 {
        2.0 * PADDING_PT + EMPTY_CONTENT_PT
    } else {
        y + PADDING_PT
    };
    Ok(Measured {
        prepared,
        document: compiled,
        tops,
        source_heights,
        output_heights,
        height_pt,
    })
}

pub fn layout(
    root: &Path,
    document: &Calculation,
    part_id: &str,
    german: bool,
) -> Result<CalculationLayout, String> {
    let measured = measure(root, document, part_id, german, "calc")?;
    let mut cells = Vec::with_capacity(measured.tops.len());
    let mut svg_bytes = 0;
    for (index, (id, source)) in measured
        .prepared
        .ids
        .iter()
        .zip(&measured.prepared.sources)
        .enumerate()
    {
        let source_svg = typst_svg::svg(
            &measured.document.pages()[index * 2],
            &typst_svg::SvgOptions::default(),
        );
        let output_svg = (!source.output.is_empty()).then(|| {
            typst_svg::svg(
                &measured.document.pages()[index * 2 + 1],
                &typst_svg::SvgOptions::default(),
            )
        });
        svg_bytes += source_svg.len() + output_svg.as_ref().map_or(0, String::len);
        if svg_bytes > MAX_SVG_BYTES {
            return Err("calculation exceeds the display layout limit".into());
        }
        cells.push(CellLayout {
            id: id.clone(),
            top_pt: measured.tops[index],
            source_svg,
            output_svg,
            source_height_pt: measured.source_heights[index],
            output_height_pt: measured.output_heights[index],
            stale: source.stale,
        });
    }
    Ok(CalculationLayout {
        revision: measured.prepared.revision,
        width_pt: A4_WIDTH_PT,
        height_pt: measured.height_pt,
        padding_pt: PADDING_PT,
        cell_gap_pt: CELL_GAP_PT,
        cells,
    })
}

/// Generated Typst for one computational page in a combined export, with its in-memory images.
/// Text rules stay scoped, so neighbouring normal pages keep their own typography.
pub(crate) fn export_source(
    root: &Path,
    value: &ExportCalculation,
    prefix: &str,
) -> Result<crate::export::ExportSource, CalculationError> {
    let image_limit = value
        .split_a4
        .then_some(A4_HEIGHT_PT - 2.0 * PADDING_PT - 2.0 * OUTPUT_INSET_Y_PT - 20.0);
    let measured = measure_with_image_limit(
        root,
        &value.document,
        &value.part_id,
        value.german,
        prefix,
        image_limit,
    )?;
    let mut source = String::new();
    if value.split_a4 {
        let usable = A4_HEIGHT_PT - 2.0 * PADDING_PT;
        debug_assert!(
            measured
                .prepared
                .image_heights
                .iter()
                .all(|height| *height <= usable)
        );
        source.push_str(&format!("#set page(width: {A4_WIDTH_PT}pt, height: {A4_HEIGHT_PT}pt, margin: {PADDING_PT}pt, fill: rgb(\"#ffffff\"))\n#[{TEXT_RULES}"));
        for (index, cell) in measured.prepared.sources.iter().enumerate() {
            if index > 0 {
                source.push_str(&format!("#v({CELL_GAP_PT}pt, weak: true)\n"));
            }
            // A cell that fits on one sheet moves whole; a taller one breaks between lines.
            let fits = measured.source_heights[index] + measured.output_heights[index] <= usable;
            source.push_str(&format!(
                "#block(width: 100%, breakable: {})[{}\n{}]\n",
                !fits, cell.code, cell.output
            ));
        }
        if measured.prepared.sources.is_empty() {
            source.push_str(&format!("#box(height: {EMPTY_CONTENT_PT}pt)\n"));
        }
    } else {
        if measured.height_pt > PORTABLE_PAGE_HEIGHT_PT {
            return Err(CalculationError::TooTall {
                height_pt: measured.height_pt,
                limit_pt: PORTABLE_PAGE_HEIGHT_PT,
            });
        }
        // Each part sits at its measured page-point position, so PDF boundaries equal the display.
        source.push_str(&format!("#set page(width: {A4_WIDTH_PT}pt, height: {}pt, margin: 0pt, fill: rgb(\"#ffffff\"))\n#[{TEXT_RULES}", measured.height_pt));
        for (index, cell) in measured.prepared.sources.iter().enumerate() {
            let top = measured.tops[index];
            source.push_str(&format!("#place(top + left, dx: {PADDING_PT}pt, dy: {top}pt)[#block(width: {CONTENT_WIDTH_PT}pt)[{}\n]]\n", cell.code));
            if !cell.output.is_empty() {
                let output_top = top + measured.source_heights[index];
                source.push_str(&format!("#place(top + left, dx: {PADDING_PT}pt, dy: {output_top}pt)[#block(width: {CONTENT_WIDTH_PT}pt)[{}\n]]\n", cell.output));
            }
        }
        source.push_str("#box()\n");
    }
    source.push_str("]\n");
    Ok((source, measured.prepared.files))
}

#[cfg(test)]
mod tests {
    use super::*;
    use goodtype_core::calculation::Cell;
    use serde_json::json;
    use typst::layout::{Frame, FrameItem};

    fn frame_text(frame: &Frame, into: &mut String) {
        for (_, item) in frame.items() {
            match item {
                FrameItem::Group(group) => frame_text(&group.frame, into),
                FrameItem::Text(text) => into.push_str(text.text.as_str()),
                _ => {}
            }
        }
    }

    pub(crate) fn document_text(document: &PagedDocument) -> String {
        let mut text = String::new();
        for page in document.pages() {
            frame_text(&page.frame, &mut text);
            text.push('\u{c}');
        }
        text
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// A valid RGBA PNG with stored (uncompressed) zlib data. Test fixture only.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut raw = Vec::new();
        for _ in 0..height {
            raw.push(0);
            raw.extend(std::iter::repeat_n(128u8, width as usize * 4));
        }
        let mut zlib = vec![0x78, 0x01];
        let chunks = raw.chunks(65_535).collect::<Vec<_>>();
        for (index, chunk) in chunks.iter().enumerate() {
            let length = chunk.len() as u16;
            zlib.push(u8::from(index + 1 == chunks.len()));
            zlib.extend(length.to_le_bytes());
            zlib.extend((!length).to_le_bytes());
            zlib.extend_from_slice(chunk);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for byte in &raw {
            a = (a + u32::from(*byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        zlib.extend(((b << 16) | a).to_be_bytes());
        let mut header = Vec::new();
        header.extend(width.to_be_bytes());
        header.extend(height.to_be_bytes());
        header.extend([8, 6, 0, 0, 0]);
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, data) in [(b"IHDR", header), (b"IDAT", zlib), (b"IEND", Vec::new())] {
            bytes.extend((data.len() as u32).to_be_bytes());
            let mut typed = kind.to_vec();
            typed.extend(&data);
            bytes.extend(&typed);
            bytes.extend(crc32(&typed).to_be_bytes());
        }
        bytes
    }

    fn image_output(bytes: &[u8]) -> Value {
        json!({"output_type":"display_data","data":{"image/png":STANDARD.encode(bytes),"text/plain":"<Figure>"},"metadata":{}})
    }

    fn calculation(cells: &[(&str, &str, Vec<Value>)]) -> Calculation {
        let mut document = Calculation::new("test", "part", "Task A");
        document.cells = cells
            .iter()
            .enumerate()
            .map(|(index, (kind, source, outputs))| {
                let mut cell = Cell::new(format!("cell-{index}"), "part", kind);
                cell.source = (*source).into();
                cell.outputs = outputs.clone();
                cell
            })
            .collect();
        document
    }

    fn compile(root: &Path, source: String, files: Vec<(String, Vec<u8>)>) -> PagedDocument {
        let mut world =
            BlockWorld::for_main("main.typ", source, root.canonicalize().unwrap(), false);
        for (name, bytes) in files {
            world.insert_binary(&name, bytes);
        }
        let compiled = typst::compile::<PagedDocument>(&world);
        compiled
            .output
            .unwrap_or_else(|errors| panic!("{}", diagnostic_messages(&errors)))
    }

    #[test]
    fn layout_uses_literal_source_and_content_height_without_running_code() {
        let temp = tempfile::tempdir().unwrap();
        let mut document = calculation(&[(
            "code",
            "#read(\"private.txt\")\nx = 2\nprint(x)",
            Vec::new(),
        )]);
        let first = layout(temp.path(), &document, "part", false).unwrap();
        assert_eq!(first.width_pt, A4_WIDTH_PT);
        assert!(
            first.height_pt > 60.0 && first.height_pt < 200.0,
            "{}",
            first.height_pt
        );
        assert_eq!(
            first.height_pt,
            layout(temp.path(), &document, "part", false)
                .unwrap()
                .height_pt
        );
        assert!(first.cells[0].source_svg.contains("<svg"));
        assert_eq!(first.cells[0].top_pt, PADDING_PT);
        assert_eq!(
            first.height_pt,
            2.0 * PADDING_PT + first.cells[0].source_height_pt
        );
        document.cells[0].source = "# Heading\n\n**Strong** and _emphasis_.\n\n- One\n- Two\n\n```python\nprint('#x')\n```\n\n$x^2$".into();
        document.cells[0].cell_type = "markdown".into();
        layout(temp.path(), &document, "part", false).unwrap();
    }

    #[test]
    fn layout_positions_follow_one_padding_and_gap_rule() {
        let temp = tempfile::tempdir().unwrap();
        let output = json!({"output_type":"stream","name":"stdout","text":"4\n"});
        let document = calculation(&[
            ("code", "2 + 2", vec![output]),
            ("markdown", "Text", Vec::new()),
            ("code", "", Vec::new()),
        ]);
        let result = layout(temp.path(), &document, "part", false).unwrap();
        let mut y = PADDING_PT;
        for (index, cell) in result.cells.iter().enumerate() {
            if index > 0 {
                y += CELL_GAP_PT;
            }
            assert_eq!(cell.top_pt, y);
            y += cell.source_height_pt + cell.output_height_pt;
        }
        assert_eq!(result.height_pt, y + PADDING_PT);
        assert!(result.cells[0].output_height_pt > 0.0 && result.cells[0].output_svg.is_some());
        assert!(result.cells[1].output_svg.is_none());
        let empty = layout(temp.path(), &calculation(&[]), "part", false).unwrap();
        assert_eq!(empty.height_pt, 2.0 * PADDING_PT + EMPTY_CONTENT_PT);
        assert!(layout(temp.path(), &calculation(&[]), "missing", false).is_err());
    }

    #[test]
    fn source_markdown_and_outputs_stay_literal_text() {
        let temp = tempfile::tempdir().unwrap();
        let hostile = "\"] #panic(\"x\") \\ #read(\"/etc/passwd\") <script>alert(1)</script>";
        let outputs = vec![
            json!({"output_type":"stream","name":"stdout","text":["\u{1b}[31mred\u{1b}[0m\n", "10%\r100%\n"]}),
            json!({"output_type":"error","ename":"ValueError","evalue":"bad","traceback":["\u{1b}[0;31mValueError\u{1b}[0m: bad \u{1b}]8;;http://x\u{7}link\u{1b}]8;;\u{7}"]}),
            json!({"output_type":"display_data","data":{"text/html":"<img src=http://remote>"},"metadata":{}}),
        ];
        let document = calculation(&[
            ("code", hostile, outputs),
            (
                "markdown",
                &format!(
                    "{hostile}\n\n<div onclick=x>{hostile}</div>\n\n![remote](http://remote/x.png)\n\n$$\\int x$$"
                ),
                Vec::new(),
            ),
            ("raw", "abc\u{202e}def\u{0}", Vec::new()),
        ]);
        let (source, files) = export_source(
            temp.path(),
            &ExportCalculation {
                document,
                part_id: "part".into(),
                german: false,
                split_a4: false,
            },
            "c",
        )
        .unwrap();
        let text = document_text(&compile(temp.path(), source, files));
        assert!(text.contains("#panic"), "{text}");
        assert!(text.contains("/etc/passwd") && text.contains("<script>"));
        assert!(text.contains("red") && !text.contains("[31m") && !text.contains("[0m"));
        assert!(text.contains("100%") && !text.contains("10%1"));
        assert!(text.contains("ValueError: bad link") && !text.contains("8;;"));
        assert!(text.contains("No supported static display."));
        assert!(text.contains("$$\\int x$$"));
        assert!(text.contains("[U+202E]") && text.contains('\u{2400}'));
    }

    #[test]
    fn long_lines_wrap_with_a_visible_marker_without_changing_source() {
        let temp = tempfile::tempdir().unwrap();
        let long = format!("value = '{}'", "a".repeat(200));
        let document = calculation(&[("code", &long, Vec::new())]);
        let (source, files) = export_source(
            temp.path(),
            &ExportCalculation {
                document: document.clone(),
                part_id: "part".into(),
                german: false,
                split_a4: false,
            },
            "c",
        )
        .unwrap();
        let compiled = compile(temp.path(), source, files);
        let text = document_text(&compiled);
        assert_eq!(text.matches('\u{21aa}').count(), 2, "{text}");
        assert_eq!(document.cells[0].source, long);
        assert!(
            widest(&compiled.pages()[0].frame, typst::layout::Point::zero())
                <= A4_WIDTH_PT - PADDING_PT + 0.01
        );
    }

    fn widest(frame: &Frame, origin: typst::layout::Point) -> f64 {
        let mut right: f64 = 0.0;
        for (position, item) in frame.items() {
            let at = origin + *position;
            match item {
                FrameItem::Group(group) => right = right.max(widest(&group.frame, at)),
                FrameItem::Text(text) => right = right.max((at.x + text.width()).to_pt()),
                _ => {}
            }
        }
        right
    }

    #[test]
    fn png_outputs_are_bounded_and_never_upscaled() {
        let temp = tempfile::tempdir().unwrap();
        let small = calculation(&[("code", "plot()", vec![image_output(&png(40, 20))])]);
        let measured = measure(temp.path(), &small, "part", false, "c").unwrap();
        assert_eq!(measured.prepared.image_heights, vec![15.0]);
        let mut retina = image_output(&png(400, 200));
        retina["metadata"] = json!({"image/png":{"width":100}});
        let measured = measure(
            temp.path(),
            &calculation(&[("code", "", vec![retina])]),
            "part",
            false,
            "c",
        )
        .unwrap();
        assert_eq!(measured.prepared.image_heights, vec![37.5]);
        let wide = measure(
            temp.path(),
            &calculation(&[("code", "", vec![image_output(&png(1000, 500))])]),
            "part",
            false,
            "c",
        )
        .unwrap();
        assert!((wide.prepared.image_heights[0] - (CONTENT_WIDTH_PT - 20.0) / 2.0).abs() < 1e-9);

        // A header that claims a huge decoded size is refused before decoding.
        let mut huge = png(1, 1);
        huge[16..20].copy_from_slice(&16_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&16_000u32.to_be_bytes());
        let mut forged = huge.clone();
        forged[16..20].copy_from_slice(&20_000u32.to_be_bytes());
        for bytes in [
            huge,
            forged,
            b"\x89PNG\r\n\x1a\nnot a png at all, really".to_vec(),
            b"GIF89a".to_vec(),
        ] {
            let document = calculation(&[("code", "", vec![image_output(&bytes)])]);
            assert!(layout(temp.path(), &document, "part", false).is_err());
        }
        let bad_base64 =
            json!({"output_type":"display_data","data":{"image/png":"***"},"metadata":{}});
        assert!(
            layout(
                temp.path(),
                &calculation(&[("code", "", vec![bad_base64])]),
                "part",
                false
            )
            .is_err()
        );
        let mut claimed = png(1, 1);
        claimed[16..20].copy_from_slice(&2048u32.to_be_bytes());
        claimed[20..24].copy_from_slice(&2048u32.to_be_bytes());
        let total = (0..13).map(|_| image_output(&claimed)).collect::<Vec<_>>();
        let error = prepare(
            &calculation(&[("code", "", total)]),
            "part",
            false,
            "c",
            None,
        )
        .err()
        .unwrap();
        assert!(error.contains("decoded image size"), "{error}");
        // Line-wrapped base64, as some writers store it, decodes.
        let mut wrapped = image_output(&png(10, 10));
        let encoded = wrapped["data"]["image/png"].as_str().unwrap().to_owned();
        wrapped["data"]["image/png"] = json!(
            encoded
                .as_bytes()
                .chunks(20)
                .map(|line| format!("{}\n", std::str::from_utf8(line).unwrap()))
                .collect::<Vec<_>>()
        );
        layout(
            temp.path(),
            &calculation(&[("code", "", vec![wrapped])]),
            "part",
            false,
        )
        .unwrap();
    }

    #[test]
    fn display_line_budget_refuses_instead_of_omitting() {
        let temp = tempfile::tempdir().unwrap();
        let flood = json!({"output_type":"stream","name":"stdout","text":"line\n".repeat(MAX_DISPLAY_LINES + 1)});
        let error = layout(
            temp.path(),
            &calculation(&[("code", "", vec![flood])]),
            "part",
            false,
        )
        .unwrap_err();
        assert!(error.contains("displayed lines"), "{error}");
    }

    #[test]
    fn markers_follow_provenance_and_the_last_success_stays_visible() {
        let temp = tempfile::tempdir().unwrap();
        let success = json!({"output_type":"stream","name":"stdout","text":"previous result\n"});
        let failure = json!({"output_type":"error","ename":"ZeroDivisionError","evalue":"division by zero","traceback":[]});
        let mut document = calculation(&[("code", "1 / 0", vec![failure])]);
        let namespace = document.cells[0].goodtype_mut();
        namespace.insert(
            "execution".into(),
            json!({"sourceHash":source_hash("older")}),
        );
        namespace.insert("lastSuccess".into(), json!({"outputs":[success]}));
        namespace.insert(
            "copiedFrom".into(),
            json!({"calculationId":"x","partId":"y"}),
        );
        let result = layout(temp.path(), &document, "part", true).unwrap();
        assert!(result.cells[0].stale);
        let (source, files) = export_source(
            temp.path(),
            &ExportCalculation {
                document,
                part_id: "part".into(),
                german: true,
                split_a4: false,
            },
            "c",
        )
        .unwrap();
        let text = document_text(&compile(temp.path(), source, files));
        for expected in [
            "ZeroDivisionError: division by zero",
            "Code nach dieser Ausgabe geändert.",
            "Letzte erfolgreiche Ausgabe:",
            "previous result",
            "Kopiert. Hier noch nicht ausgeführt.",
        ] {
            assert!(text.contains(expected), "{expected} missing from {text}");
        }
    }

    #[test]
    fn preserved_export_matches_layout_and_a4_split_never_clips() {
        let temp = tempfile::tempdir().unwrap();
        let tall = (0..120)
            .map(|line| format!("value_{line} = {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let document = calculation(&[
            ("code", "short = 1", Vec::new()),
            ("code", &tall, Vec::new()),
            ("markdown", "Closing *note*", Vec::new()),
        ]);
        let expected = layout(temp.path(), &document, "part", false).unwrap();
        let preserved = ExportCalculation {
            document: document.clone(),
            part_id: "part".into(),
            german: false,
            split_a4: false,
        };
        let (source, files) = export_source(temp.path(), &preserved, "c").unwrap();
        let compiled = compile(temp.path(), source, files);
        assert_eq!(compiled.pages().len(), 1);
        let size = compiled.pages()[0].frame.size();
        assert!((size.x.to_pt() - A4_WIDTH_PT).abs() < 1e-6);
        assert!((size.y.to_pt() - expected.height_pt).abs() < 1e-6);
        assert!(expected.height_pt > A4_HEIGHT_PT);

        let split = ExportCalculation {
            split_a4: true,
            ..preserved
        };
        let (source, files) = export_source(temp.path(), &split, "c").unwrap();
        let compiled = compile(temp.path(), source, files);
        assert!(compiled.pages().len() >= 2);
        for page in compiled.pages() {
            assert!((page.frame.height().to_pt() - A4_HEIGHT_PT).abs() < 1e-6);
            assert!(
                lowest(&page.frame, typst::layout::Point::zero())
                    <= A4_HEIGHT_PT - PADDING_PT + 0.01
            );
        }
        let text = document_text(&compiled);
        for line in 0..120 {
            assert!(
                text.contains(&format!("value_{line} = {line}")),
                "line {line} missing"
            );
        }
        assert!(text.contains("short = 1") && text.contains("note"));
    }

    fn lowest(frame: &Frame, origin: typst::layout::Point) -> f64 {
        let mut bottom: f64 = 0.0;
        for (position, item) in frame.items() {
            let at = origin + *position;
            match item {
                FrameItem::Group(group) => bottom = bottom.max(lowest(&group.frame, at)),
                FrameItem::Text(text) => {
                    bottom = bottom.max(at.y.to_pt() + text.size.to_pt() * 0.3)
                }
                FrameItem::Image(_, size, _) => {
                    assert!(at.x.to_pt() >= PADDING_PT - 0.01);
                    assert!(at.x.to_pt() + size.x.to_pt() <= A4_WIDTH_PT - PADDING_PT + 0.01);
                    bottom = bottom.max(at.y.to_pt() + size.y.to_pt());
                }
                _ => {}
            }
        }
        bottom
    }

    #[test]
    fn oversized_pages_require_splitting_and_tall_images_scale_with_a_label() {
        let temp = tempfile::tempdir().unwrap();
        let tall = (0..2000)
            .map(|line| format!("v{line} = {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let document = calculation(&[("code", &tall, Vec::new())]);
        let error = export_source(
            temp.path(),
            &ExportCalculation {
                document: document.clone(),
                part_id: "part".into(),
                german: false,
                split_a4: false,
            },
            "c",
        )
        .unwrap_err();
        assert!(matches!(error, CalculationError::TooTall { .. }), "{error}");
        export_source(
            temp.path(),
            &ExportCalculation {
                document,
                part_id: "part".into(),
                german: false,
                split_a4: true,
            },
            "c",
        )
        .unwrap();

        let plot = calculation(&[("code", "", vec![image_output(&png(400, 1600))])]);
        let (source, files) = export_source(
            temp.path(),
            &ExportCalculation {
                document: plot.clone(),
                part_id: "part".into(),
                german: false,
                split_a4: true,
            },
            "c",
        )
        .unwrap();
        assert!(source.contains("Scaled to 64%"));
        let exported = compile(temp.path(), source, files);
        assert!(!exported.pages().is_empty());
        assert!(
            exported
                .pages()
                .iter()
                .all(|page| (page.frame.height().to_pt() - A4_HEIGHT_PT).abs() < 0.001)
        );
        for page in exported.pages() {
            assert!(
                lowest(&page.frame, typst::layout::Point::zero())
                    <= A4_HEIGHT_PT - PADDING_PT + 0.01
            );
        }
        assert!(document_text(&exported).contains("Scaled to 64%"));
        export_source(
            temp.path(),
            &ExportCalculation {
                document: plot,
                part_id: "part".into(),
                german: false,
                split_a4: false,
            },
            "c",
        )
        .unwrap();
    }

    fn positioned_text(
        frame: &Frame,
        origin: typst::layout::Point,
        into: &mut Vec<(String, f64, f64)>,
    ) {
        for (position, item) in frame.items() {
            let at = origin + *position;
            match item {
                FrameItem::Group(group) => {
                    // Generated calculation content never scales or rotates.
                    assert!(group.transform.is_identity());
                    positioned_text(&group.frame, at, into);
                }
                FrameItem::Text(text) => {
                    into.push((text.text.to_string(), at.x.to_pt(), at.y.to_pt()))
                }
                _ => {}
            }
        }
    }

    #[test]
    fn preview_and_preserved_export_place_every_text_run_identically() {
        let temp = tempfile::tempdir().unwrap();
        let output = json!({"output_type":"stream","name":"stdout","text":"first\nsecond\n"});
        let document = calculation(&[
            (
                "markdown",
                "# Title\n\nSome *prose* with `code`.\n\n- one\n- two",
                Vec::new(),
            ),
            ("code", "for i in range(2):\n    print(i)", vec![output]),
            ("raw", "raw text", Vec::new()),
        ]);
        let measured = measure(temp.path(), &document, "part", false, "c").unwrap();
        let mut expected = Vec::new();
        for (index, top) in measured.tops.iter().enumerate() {
            let source = typst::layout::Point::new(
                typst::layout::Abs::pt(PADDING_PT),
                typst::layout::Abs::pt(*top),
            );
            positioned_text(
                &measured.document.pages()[index * 2].frame,
                source,
                &mut expected,
            );
            let output = source
                + typst::layout::Point::with_y(typst::layout::Abs::pt(
                    measured.source_heights[index],
                ));
            positioned_text(
                &measured.document.pages()[index * 2 + 1].frame,
                output,
                &mut expected,
            );
        }
        let value = ExportCalculation {
            document,
            part_id: "part".into(),
            german: false,
            split_a4: false,
        };
        let (source, files) = export_source(temp.path(), &value, "c").unwrap();
        let exported = compile(temp.path(), source, files);
        let mut actual = Vec::new();
        positioned_text(
            &exported.pages()[0].frame,
            typst::layout::Point::zero(),
            &mut actual,
        );
        assert_eq!(actual.len(), expected.len());
        for ((text, x, y), (expected_text, expected_x, expected_y)) in actual.iter().zip(&expected)
        {
            assert_eq!(text, expected_text);
            assert!(
                (x - expected_x).abs() <= 0.5 && (y - expected_y).abs() <= 0.5,
                "{text}: {x},{y} vs {expected_x},{expected_y}"
            );
        }
    }

    #[test]
    fn long_markdown_words_stay_inside_the_page() {
        let temp = tempfile::tempdir().unwrap();
        let word = "x".repeat(400);
        let document = calculation(&[("markdown", &format!("See {word} here."), Vec::new())]);
        let value = ExportCalculation {
            document,
            part_id: "part".into(),
            german: false,
            split_a4: false,
        };
        let (source, files) = export_source(temp.path(), &value, "c").unwrap();
        let compiled = compile(temp.path(), source, files);
        // Break opportunities are zero-width; the word itself stays complete text.
        assert!(
            document_text(&compiled)
                .replace('\u{200b}', "")
                .contains(&word)
        );
        assert!(
            widest(&compiled.pages()[0].frame, typst::layout::Point::zero())
                <= A4_WIDTH_PT - PADDING_PT + 0.01
        );
    }

    #[test]
    fn export_leaves_canonical_cells_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let document = calculation(&[(
            "code",
            "x\t= 1\r\n",
            vec![json!({"output_type":"stream","name":"stdout","text":"1\n"})],
        )]);
        let before = document.bytes().unwrap();
        let value = ExportCalculation {
            document,
            part_id: "part".into(),
            german: false,
            split_a4: true,
        };
        export_source(temp.path(), &value, "c").unwrap();
        assert_eq!(value.document.bytes().unwrap(), before);
    }
}
