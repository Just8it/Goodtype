use std::{
    env,
    io::{self, Write},
    path::Path,
};

use goodtype_core::storage::{MAX_INK_BYTES, files::read_bounded};

fn main() {
    if let Err(error) = run() {
        eprintln!("goodtype-ink: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let command = arguments.next().ok_or_else(usage)?;
    let input = arguments.next().ok_or_else(usage)?;
    if command != "decode" || arguments.next().is_some() {
        return Err(usage());
    }

    let path = Path::new(&input);
    let bytes = read_bounded(path, MAX_INK_BYTES)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let layer = goodtype_core::ink_codec::decode(&bytes).map_err(|error| error.to_string())?;

    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, &layer).map_err(|error| error.to_string())?;
    output.write_all(b"\n").map_err(|error| error.to_string())?;
    Ok(())
}

fn usage() -> String {
    "usage: goodtype-ink decode INPUT".into()
}
