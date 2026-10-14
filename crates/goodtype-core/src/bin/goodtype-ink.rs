use std::{
    env, fs,
    io::{self, Read, Write},
    path::Path,
};

const MAX_INK_BYTES: u64 = 64 * 1024 * 1024;

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

    let bytes = read_bounded(Path::new(&input))?;
    let layer = goodtype_core::ink_codec::decode(&bytes).map_err(|error| error.to_string())?;

    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, &layer).map_err(|error| error.to_string())?;
    output.write_all(b"\n").map_err(|error| error.to_string())?;
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.len() > MAX_INK_BYTES {
        return Err(format!(
            "{} is {} bytes; maximum is {MAX_INK_BYTES}",
            path.display(),
            metadata.len()
        ));
    }

    let capacity = usize::try_from(metadata.len()).map_err(|_| "input size is unsupported")?;
    let file =
        fs::File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(MAX_INK_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_INK_BYTES {
        return Err(format!(
            "{} exceeds the {MAX_INK_BYTES}-byte maximum",
            path.display()
        ));
    }
    Ok(bytes)
}

fn usage() -> String {
    "usage: goodtype-ink decode INPUT".into()
}
