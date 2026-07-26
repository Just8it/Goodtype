#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod export;
mod library;
mod notebook;
mod settings;
mod typst;
mod workspace;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(notebook::NotebookHistories::default())
        .manage(workspace::AllowedRoots::default())
        .manage(settings::RemotePackages::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let policy = app.state::<settings::RemotePackages>();
            settings::seed_remote_packages(&handle, &policy);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            library::library_root,
            library::pick_library_root,
            library::list_library,
            library::open_library_notebook,
            workspace::phase0_notebook_root,
            workspace::pick_notebook_root,
            workspace::pick_new_notebook_root,
            workspace::open_recent_root,
            workspace::record_notebook_opened,
            workspace::write_phase0_metrics,
            settings::load_app_settings,
            settings::save_app_settings,
            settings::list_recent_notebooks,
            settings::set_notebook_pinned,
            settings::remove_recent_notebook,
            notebook::create_notebook,
            notebook::open_notebook,
            notebook::open_page,
            notebook::focus_page,
            notebook::create_page,
            notebook::commit_notebook,
            notebook::undo_notebook,
            notebook::redo_notebook,
            notebook::duplicate_page,
            notebook::delete_page,
            notebook::reorder_pages,
            notebook::search_notebook,
            notebook::list_recovery_candidates,
            notebook::restore_recovery_candidate,
            notebook::discard_recovery_candidate,
            notebook::store_pasted_image,
            typst::compile_typst,
            typst::complete_typst,
            export::export_notebook_pdf,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Goodtype");
}
