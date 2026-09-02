//! Sync desktop adapter.
//!
//! Tauri's job here is to open a native macOS window, host the statically
//! exported frontend, and expose a thin typed command layer over the domain
//! crates under `crates/`.
//!
//! Rules for future work in this crate:
//!
//! * Domain logic belongs in separate Rust modules or crates that compile and
//!   run without Tauri, so it stays testable from plain `cargo test`.
//!   `crates/sync-memory` is the first of them: it owns everything about the
//!   memory engine, and this crate only parses input and maps results.
//! * Tauri commands must remain thin, typed adapters: parse input, call a
//!   domain function, map the result. No branching, no I/O policy, no state
//!   machines in the command layer.
//! * Every capability or plugin added here widens the app's attack surface, so
//!   it is added only when a shipped feature needs it.

pub mod attending;
pub mod connect;
#[cfg(target_os = "macos")]
pub mod dock;
pub mod extensions;
pub mod handlers;
pub mod memory;
pub mod project;
pub mod remote;
pub mod schedule;
pub mod server;
pub mod sessions;
pub mod settings;
pub mod tray;
pub mod updates;
pub mod vault;
pub mod voice;
pub mod windows;
pub mod work;
pub mod worktree;

pub fn run() {
    tauri::Builder::default()
        // The scheme an unpacked extension's files are served under, and the
        // one thing a packaged build had to be asked rather than reasoned
        // about — see `extensions.rs` and `docs/extensions.md` §5.
        .register_uri_scheme_protocol(extensions::SCHEME, extensions::serve)
        // Choosing a folder is the one thing the window cannot do for itself:
        // the open panel is a native dialog, and only `dialog:allow-open` is
        // granted for it — the plugin's save, message and ask dialogs are not.
        .plugin(tauri_plugin_dialog::init())
        // Optional, off until somebody turns it on, and the reason it exists at
        // all: agents reach Sync through a port, so an agent working while Sync
        // is closed reaches nothing. Offered where that becomes true — when an
        // agent is first connected — rather than at install, where it would be
        // a demand made before any of this has been useful.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // A link to the web belongs to the browser. Only `open-url` is granted,
        // and only over the schemes the plugin's own default scope names —
        // opening a *path* is a different command and is not granted, so a
        // record cannot ask this window to launch something on the disk.
        .plugin(tauri_plugin_opener::init())
        // Nothing of this is granted to the webview. The plugin is here for
        // `updates::in_the_background`, which is Rust and runs once at launch.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(memory::MemorySessions::default())
        .manage(server::RunningServer::default())
        .manage(schedule::ScheduleFile::default())
        .manage(work::WorkFile::default())
        // Agent sessions outlive the screen that opened one, so they are held
        // by the application rather than by a window or by an extension.
        .manage(sessions::live::Sessions::default())
        // The server comes up with the application, before any window does.
        // Nothing about it waits for a person: an agent may be running against
        // a project of theirs while every window is closed, and "is Sync
        // serving" should not have the answer "only if you opened something".
        .setup(|app| {
            use tauri::Manager as _;
            let handle = app.handle().clone();
            if let Err(error) = server::start(&handle, app.state::<server::RunningServer>().inner())
            {
                // Reported where a person will look for it rather than fatal.
                // A window that refused to open because a port was taken would
                // be a product held hostage by whatever else is on that port.
                eprintln!("the MCP server did not start: {}", error.message);
            }
            // Held from here rather than from the first window: an agent
            // calls an extension's tool whether or not anybody has a project
            // open, and the engine has nowhere to knock.
            attending::attend(&handle);
            if let Err(error) = tray::install(app) {
                eprintln!("the menu bar item could not be created: {error}");
            }
            // The clock, in the process that survives every window being
            // closed. It is started here rather than beside a window because
            // that is the whole point of it: a project ticks whether or not
            // anybody has it open, and a machine sitting in the menu bar is
            // exactly the case it exists for.
            schedule::start(&handle);
            windows::follow_the_windows(&handle);
            // The Dock icon's own menu, which is the only place a second window
            // can be asked for without Sync being the active application first.
            #[cfg(target_os = "macos")]
            dock::install(&handle);
            // Last, and not awaited: a launch must not wait on a network
            // request, and an offline machine must cost it nothing.
            updates::in_the_background(&handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the last window puts Sync away rather than stopping it:
            // the server behind it is what agents talk to, and they are not
            // looking at any window.
            if matches!(event, tauri::WindowEvent::Destroyed) {
                windows::follow_the_windows(&tauri::Manager::app_handle(window).clone());
            }
        })
        .invoke_handler(tauri::generate_handler![
            sessions::session_catalog,
            sessions::agent_adapters,
            sessions::agent_adapters_prepare,
            sessions::agent_adapters_forget,
            sessions::session_live,
            sessions::session_open,
            sessions::session_subscribe,
            sessions::session_backlog,
            sessions::session_remembered,
            sessions::session_resume,
            sessions::session_forget_remembered,
            sessions::session_kept_as,
            sessions::session_for_record,
            sessions::session_unsubscribe,
            sessions::session_prompt,
            sessions::session_rename,
            sessions::session_image,
            sessions::session_image_save,
            sessions::session_cancel,
            sessions::session_permission_respond,
            sessions::session_set_option,
            sessions::session_set_mode,
            sessions::session_close,
            sessions::session_forget,
            worktree::worktree_location,
            worktree::worktree_set_location,
            worktree::worktree_list,
            worktree::worktree_adopt,
            worktree::worktree_discard,
            connect::agents_list,
            connect::agent_connect,
            connect::agent_disconnect,
            extensions::extension_install_file,
            extensions::extension_install_folder,
            extensions::extension_list,
            extensions::extension_forget,
            extensions::extension_install_registry,
            extensions::extension_repoint,
            extensions::extension_fetch,
            extensions::registry_index,
            extensions::registry_cached_index,
            extensions::registry_ledger,
            handlers::extension_handler_call,
            project::project_probe,
            project::project_remote,
            project::project_initialize_repository,
            server::server_status,
            server::server_restart,
            server::server_set_port,
            server::server_new_token,
            remote::remote_status,
            remote::remote_enable,
            remote::remote_pair,
            remote::remote_revoke,
            project::project_settings_load,
            project::project_identifier_suggest,
            project::project_settings_save,
            schedule::schedule_remember,
            schedule::schedule_switched_off,
            schedule::schedule_switch,
            project::project_view_load,
            project::project_view_save,
            project::recent_projects_load,
            project::projects_registered,
            project::project_register,
            project::project_forget,
            project::recent_projects_record,
            memory::memory_open,
            memory::memory_status,
            memory::memory_types,
            memory::memory_type_create,
            memory::memory_extension_types_publish,
            memory::memory_type_update,
            memory::memory_type_delete,
            memory::memory_folder_attach,
            memory::memory_scan,
            memory::memory_unmatched_resolve,
            memory::memory_folders,
            memory::memory_folder_create,
            memory::memory_folder_describe,
            memory::memory_folder_delete,
            memory::memory_folder_toll,
            memory::memory_folder_rename,
            memory::memory_document_move,
            memory::memory_records,
            memory::memory_document,
            memory::memory_content,
            memory::memory_file_create,
            memory::memory_document_update,
            memory::memory_document_create,
            memory::memory_document_delete,
            memory::memory_document_dependents,
            memory::memory_list,
            memory::memory_search,
            memory::memory_get,
            memory::memory_save,
            memory::memory_delete,
            memory::memory_sync_state,
            memory::memory_rewind,
            memory::memory_presence,
            memory::memory_remote_set,
            memory::memory_remote_remove,
            memory::memory_fetch,
            memory::memory_push,
            memory::memory_reindex,
            memory::memory_reconcile,
            settings::settings_open,
            vault::vault_entries,
            vault::vault_write,
            vault::vault_forget,
            vault::vault_storage,
            vault::extension_secret_read,
            vault::extension_secret_write,
            vault::extension_secret_forget,
            voice::voice_status,
            voice::voice_choose,
            voice::voice_speak,
            voice::voice_stop,
            windows::window_new,
            windows::window_named,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Sync")
        .run(|app, event| {
            // Closing the last window is not quitting. Sync serves agents from a
            // port, and they are not looking at any window — an application
            // that ended with its last one would take their memory with it. The
            // menu bar item is what stays behind, and `Quit Sync` is what ends
            // this process.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event
                && code.is_none()
            {
                api.prevent_exit();
            }

            // Opening Sync again is asking for Sync, and with every window
            // closed there is nothing on screen to be given. This is the whole
            // of "it only starts once": the second launch, the click on a Dock
            // icon somebody kept, and the click on the icon of an application
            // whose windows are all away all arrive here, and without an answer
            // they were a launch that appeared to do nothing.
            if let tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } = &event
                && !has_visible_windows
            {
                windows::show(app);
            }

            // An agent is a child process, and one whose parent is gone is
            // nobody's: it would go on running, go on spending, and never be
            // listed anywhere again. Closing a window does not end one — a
            // session outliving its screen is the whole point of where it
            // lives — but the application ending does.
            //
            // The server is the same kind of child, and ends here for a second
            // reason: it holds a port. Left behind, it goes on answering agents
            // for a Sync that is closed, and the next start finds its own port
            // taken — by itself, from the last run, with whatever code it was
            // built from then.
            if matches!(event, tauri::RunEvent::Exit) {
                use tauri::Manager as _;
                let sessions = app.state::<sessions::live::Sessions>();
                tauri::async_runtime::block_on(sessions::close_all(sessions.inner()));
                server::stop(app.state::<server::RunningServer>().inner());
            }
        });
}
