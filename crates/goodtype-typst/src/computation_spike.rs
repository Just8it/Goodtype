//! Stage 0 experiment only. Uses the existing World, fonts and PDF backend offline.
use super::{BlockWorld, diagnostic_messages};
use std::fmt::Write;
use std::path::Path;
use std::time::Instant;
use typst_layout::PagedDocument;

const A4_WIDTH: f64 = 210.0 * 72.0 / 25.4;

fn document(root: &Path, source: &str) -> PagedDocument {
    let world = BlockWorld::for_main(
        "main.typ",
        source.to_owned(),
        root.canonicalize().unwrap(),
        false,
    );
    let compiled = typst::compile::<PagedDocument>(&world);
    assert!(
        compiled.warnings.is_empty(),
        "{}",
        diagnostic_messages(&compiled.warnings)
    );
    compiled
        .output
        .unwrap_or_else(|errors| panic!("{}", diagnostic_messages(&errors)))
}

fn source(height: &str, content: &str) -> String {
    format!(
        "#set page(width: 210mm, height: {height}, margin: 18pt)\n\
         #set text(font: \"DejaVu Sans Mono\", size: 10pt)\n\
         #set par(leading: 3pt)\n{content}"
    )
}

fn code_cell(index: usize, lines: usize) -> String {
    let mut result =
        format!("#block(breakable: false)[\n#text(size: 8pt)[Cell {index}]\n#v(4pt)\n");
    let code = (0..lines)
        .map(|line| format!("value_{line} = {line} * 2"))
        .collect::<Vec<_>>()
        .join("\n");
    // ASCII fixture only. Do not reuse Rust Debug escaping for untrusted production source.
    writeln!(
        result,
        "#raw({code:?}, block: true, lang: \"python\")\n#v(8pt)\n]"
    )
    .unwrap();
    result
}

fn media_box(pdf: &[u8]) -> String {
    let text = String::from_utf8_lossy(pdf);
    text.find("/MediaBox")
        .map(|start| text[start..].chars().take(100).collect())
        .unwrap_or_else(|| "not visible in plain PDF objects".to_owned())
}

#[test]
fn content_height_is_short_deterministic_and_a4_width() {
    let root = tempfile::tempdir().unwrap();
    let text = source("auto", &code_cell(1, 1));
    let first = document(root.path(), &text);
    let second = document(root.path(), &text);
    assert_eq!(first.pages().len(), 1);
    let size = first.pages()[0].frame.size();
    assert!((size.x.to_pt() - A4_WIDTH).abs() < 0.001);
    assert!(size.y.to_pt() > 36.0 && size.y.to_pt() < 200.0);
    assert_eq!(size, second.pages()[0].frame.size());
}

#[test]
#[ignore = "Stage 0 artifact experiment; set GOODTYPE_COMPUTATION_LAYOUT_OUTPUT to a fresh directory"]
fn computational_layout_artifacts() {
    let destination = std::env::var_os("GOODTYPE_COMPUTATION_LAYOUT_OUTPUT")
        .expect("Set GOODTYPE_COMPUTATION_LAYOUT_OUTPUT");
    let destination = std::path::PathBuf::from(destination);
    assert!(
        !destination.exists(),
        "Never overwrite existing experiment evidence"
    );
    std::fs::create_dir_all(&destination).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut report =
        String::from("case\tpages\twidth_pt\theight_pt\tlayout_ms\tpdf_bytes\tmedia_box\n");
    let many = (1..=30)
        .map(|index| code_cell(index, 4))
        .collect::<String>();
    let stream = format!(
        "#raw({:?}, block: true)",
        (0..2000)
            .map(|i| format!("line {i}: saved output\n"))
            .collect::<String>()
    );
    let table = "#table(columns: (1fr, 1fr, 1fr), table.header([x], [y], [value]), ..range(120).map(i => [#i]))";
    let mut cases = vec![
        ("short", source("auto", &code_cell(1, 1))),
        ("thirty-cells", source("auto", &many)),
        ("long-stream", source("auto", &stream)),
        ("table", source("auto", table)),
        ("a4-split", source("297mm", &many)),
        ("height-5000", source("5000pt", "PDF height probe")),
        ("height-14400", source("14400pt", "PDF height probe")),
        ("height-20000", source("20000pt", "PDF height probe")),
        ("height-50000", source("50000pt", "PDF height probe")),
    ];
    if let Some(plot) = std::env::var_os("GOODTYPE_COMPUTATION_PLOT") {
        std::fs::copy(plot, root.path().join("sine.svg")).unwrap();
        cases.push(("plot", source("auto", "#image(\"sine.svg\", width: 100%)")));
    }
    for (name, text) in cases {
        let started = Instant::now();
        let compiled = document(root.path(), &text);
        let elapsed = started.elapsed().as_millis();
        let page = &compiled.pages()[0];
        let width = page.frame.width().to_pt();
        let height = page.frame.height().to_pt();
        assert!((width - A4_WIDTH).abs() < 0.001);
        if name == "short" {
            assert!(height < 200.0);
        }
        if name == "thirty-cells" {
            assert_eq!(compiled.pages().len(), 1);
            assert!(height > 841.89);
        }
        if name == "a4-split" {
            assert!(compiled.pages().len() > 1);
        }
        let pdf = typst_pdf::pdf(&compiled, &typst_pdf::PdfOptions::default())
            .unwrap_or_else(|errors| panic!("{name}: {}", diagnostic_messages(&errors)));
        std::fs::write(destination.join(format!("{name}.typ")), text).unwrap();
        std::fs::write(destination.join(format!("{name}.pdf")), &pdf).unwrap();
        if name == "short" {
            let svg = typst_svg::svg(page, &typst_svg::SvgOptions::default());
            std::fs::write(destination.join("short.svg"), svg).unwrap();
        }
        writeln!(
            report,
            "{name}\t{}\t{width:.6}\t{height:.6}\t{elapsed}\t{}\t{}",
            compiled.pages().len(),
            pdf.len(),
            media_box(&pdf).replace(['\n', '\r', '\t'], " ")
        )
        .unwrap();
    }
    std::fs::write(destination.join("layout.tsv"), &report).unwrap();
    println!("{report}");
}
