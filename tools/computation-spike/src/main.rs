//! Windows-only development probe. No notebook APIs and no application IPC.
#[cfg(windows)]
mod windows;

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--native-smoke") {
        println!("native AppContainer entry reached");
        return;
    }
    #[cfg(windows)]
    if let Err(error) = windows::run() {
        eprintln!("Computation probe failed: {error}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    {
        eprintln!("This AppContainer experiment requires Windows.");
        std::process::exit(1);
    }
}
