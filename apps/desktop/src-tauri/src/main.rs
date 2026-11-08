#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod computation;
mod export;
mod helper;
mod importer;
mod library;
mod notebook;
mod preset;
mod recognition;
mod settings;
mod store;
mod tinymist;
mod typst;
mod workspace;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(notebook::NotebookHistories::default())
        .manage(workspace::AllowedRoots::default())
        .manage(settings::RemotePackages::default())
        .manage(computation::RuntimeState::default())
        .manage(helper::Helper::default())
        .manage(tinymist::Tinymist::default())
        .manage(importer::Importers::default())
        .manage(recognition::math_process::MathProcess::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let policy = app.state::<settings::RemotePackages>();
            settings::seed_remote_packages(&handle, &policy);
            if let Ok(settings) = settings::load_app_settings(handle.clone()) {
                app.state::<computation::RuntimeState>().downloads.store(
                    settings.python_package_downloads,
                    std::sync::atomic::Ordering::SeqCst,
                );
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            helper::codex::helper_codex_status,
            helper::codex::helper_codex_run,
            helper::codex::helper_codex_stop,
            helper::codex::helper_codex_close,
            helper::helper_status,
            helper::proposals::helper_proposals,
            helper::preset_install::helper_install_preset,
            helper::proposals::helper_dismiss_proposal,
            helper::proposals::helper_request_change,
            helper::proposals::helper_edit_proposal,
            notebook::insert_helper_proposal,
            notebook::apply_helper_cell_proposal,
            notebook::apply_helper_page_proposal,
            helper::helper_install_relay,
            helper::helper_share,
            helper::helper_unshare,
            helper::helper_revoke_attachment,
            helper::helper_revoke_notebook,
            helper::helper_context_reply,
            helper::helper_answer_connection,
            helper::helper_answer_scope,
            helper::helper_revoke,
            helper::helper_activity,
            library::library_root,
            library::pick_library_root,
            library::list_library,
            library::open_library_notebook,
            library::create_library_folder,
            library::create_library_notebook,
            library::rename_library_entry,
            library::move_library_entries,
            library::delete_library_entry,
            library::write_notebook_cover,
            library::library_cover,
            library::library_favourites,
            library::set_library_favourite,
            library::list_library_favourites,
            importer::importer_status,
            importer::importer_install,
            importer::importer_check_update,
            importer::importer_update,
            importer::importer_cancel,
            importer::importer_dismiss,
            importer::import_pick_source,
            importer::import_start,
            workspace::phase0_notebook_root,
            workspace::pick_notebook_root,
            workspace::open_recent_root,
            workspace::resume_notebook_root,
            workspace::resume_notebook_session,
            workspace::record_notebook_opened,
            workspace::record_notebook_page,
            workspace::record_notebook_session,
            helper::helper_close_notebook_session,
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
            notebook::create_calculation_page,
            notebook::import_calculation_notebook,
            computation::python_runtime_status,
            computation::python_runtime_install,
            computation::python_runtime_cancel,
            computation::python_runtime_forget_local,
            computation::local::python_runtime_discover,
            computation::local::python_runtime_probe,
            computation::run_calculation_cell,
            computation::end_calculation_session,
            notebook::read_calculation,
            notebook::layout_calculation,
            notebook::calculation_execution_plan,
            notebook::edit_calculation,
            notebook::calculation_history,
            notebook::pick_pdf_reference,
            notebook::read_pdf_reference,
            notebook::import_pdf_pages,
            notebook::commit_notebook,
            notebook::undo_notebook,
            notebook::redo_notebook,
            notebook::duplicate_page,
            notebook::delete_page,
            notebook::reorder_pages,
            notebook::undo_page_structure,
            notebook::redo_page_structure,
            notebook::search_notebook,
            notebook::list_recovery_candidates,
            notebook::restore_recovery_candidate,
            notebook::discard_recovery_candidate,
            notebook::store_pasted_image,
            preset::list_typst_presets,
            preset::pick_typst_preset,
            preset::validate_typst_preset,
            preset::set_default_typst_preset,
            preset::install_page_typst_preset,
            preset::create_typst_preset,
            preset::read_typst_preset,
            preset::save_typst_preset,
            typst::compile_typst,
            typst::typst_font_families,
            typst::complete_typst,
            typst::hover_typst,
            typst::format_typst,
            typst::analyze_typst,
            export::export_notebook_pdf,
            export::render_notebook_cover,
            recognition::recognition_capabilities,
            recognition::recognize_ink_text,
            recognition::recognize_ink_math,
            recognition::prepare_math_recognition,
            importer::remove_math_recognizer,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Goodtype")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<helper::Helper>().stop();
            }
        });
}
