#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod export;
mod notebook;
mod typst;
mod workspace;

fn main() {
    tauri::Builder::default()
        .manage(notebook::NotebookHistories::default())
        .invoke_handler(tauri::generate_handler![
            workspace::phase0_notebook_root,
            workspace::write_phase0_metrics,
            notebook::create_notebook,
            notebook::open_notebook,
            notebook::open_page,
            notebook::create_page,
            notebook::commit_notebook,
            notebook::undo_notebook,
            notebook::redo_notebook,
            notebook::store_pasted_image,
            typst::compile_typst,
            export::export_pdf,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Goodtype");
}
