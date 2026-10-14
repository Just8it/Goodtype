use goodtype_typst::{CompileRequest, compile_block};

#[test]
fn editor_colour_syntax_renders_text_and_math_offline() {
    let root = tempfile::tempdir().unwrap();
    for source in [
        r##"#text(fill: rgb("#4c8df0"))[Grüße]"##,
        r##"$text(fill: #rgb("#4c8df0"), x^2) + y$"##,
        "$\n  text(fill: #rgb(\"#4c8df0\"), frac(x^2, 2)) + y\n$",
        r##"#text(fill: rgb("#4c8df0"))[$x^2$]"##,
    ] {
        let result = compile_block(
            root.path(),
            &CompileRequest {
                source: source.to_owned(),
                width_pt: 240.0,
                generation: 1,
                allow_remote_packages: false,
            },
        )
        .unwrap();
        let svg = result
            .svg
            .unwrap_or_else(|| panic!("{source}: {:?}", result.diagnostics));
        assert!(svg.contains("#4c8df0"), "chosen colour missing: {source}");
    }
}
