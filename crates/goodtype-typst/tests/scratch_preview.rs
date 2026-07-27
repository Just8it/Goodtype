use goodtype_typst::{CompileRequest, compile_block};

#[test]
fn dump() {
    let root = std::env::temp_dir();
    for (name, source) in [
        ("plain", "The quick brown fox jumps over the lazy dog."),
        (
            "math",
            "#set text(size: 12pt)\n\nThe displacement is $x(t) = x_0 + v_0 t + 1/2 a t^2$.",
        ),
        ("heading", "= A heading\n\nSome body text below it."),
        ("block-math", "$ sum_(i=1)^n i = (n(n+1))/2 $"),
    ] {
        let result = compile_block(
            &root,
            &CompileRequest {
                source: source.to_owned(),
                width_pt: 240.0,
                generation: 1,
                allow_remote_packages: false,
            },
        )
        .unwrap();
        let svg = result.svg.unwrap();
        let head: String = svg.chars().take(400).collect();
        println!(
            "=== {name} w={:?} h={:?}\n{head}\n",
            result.width_pt, result.height_pt
        );
        std::fs::write(
            std::env::temp_dir().join(format!("goodtype-{name}.svg")),
            &svg,
        )
        .unwrap();
    }
}
