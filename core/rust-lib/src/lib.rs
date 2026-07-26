//! `inspector-rust-core` — shared, OS-independent app logic for Inspector Rust.

mod app_launcher;
mod audio;
mod audio_swap;
mod auto_expand;
mod media_trim;
mod social_dl;
mod backup;
mod brightness;
mod edr;
mod bruno;
mod cli_dispatch;
mod cleaner;
mod clipboard_watcher;
mod commands;
mod crypto;
mod cutout;
mod cutout_ml;
mod db;
#[cfg(target_os = "linux")]
mod desktop_shortcuts;
mod expander;
mod faker;
mod figlet;
mod hotkey;
mod hue;
#[cfg(target_os = "macos")]
mod snitch;
mod sync;
mod shazam;
mod translate;
mod mic_capture;
mod image_ops;
mod logging;
mod md_to_pdf;
mod meme;
mod models;
mod notes;
mod ocr;
mod totp_import;
mod totp_store;
mod paste;
mod recolor;
mod input_lock;
#[cfg(target_os = "macos")]
mod esc_watch;
mod region_picker;
mod screen_picker;
mod screen_record;
mod sec;
mod screen_recording;
mod frontmost_app;
mod screenshot_editor;
mod screenshot_preview;
mod seed;
mod settings;
mod snippet_template;
mod snippets;
mod sound;
mod status_toast;
mod alarm;
mod tracking;
mod color_loupe;
mod gestures;
mod keepalive;
mod window_snap;
mod window_palette;
mod system_commands;
mod system_stats;
mod stats_history;
mod boom;
mod finder_selection;
#[cfg(target_os = "macos")]
mod osascript_util;
mod text_field;
mod timer;
mod ui_state;
mod wakelock;

pub use ui_state::UiState;

use std::sync::atomic::Ordering;

use tauri::{
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager, WindowEvent, Wry,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::clipboard_watcher::WatcherState;

pub fn run(context: tauri::Context<Wry>) {
    if cli_dispatch::exit_if_help_requested() {
        return;
    }

    // Persistent logging: stderr + a daily-rolling file in the data dir, plus a
    // panic hook that captures crashes. `_log_guard` MUST stay alive for the
    // whole process (its drop flushes the async writer) — it lives until `run`
    // returns, i.e. until the blocking `.run(...)` below exits.
    let _log_guard = logging::init();
    logging::install_panic_hook();
    tracing::info!(
        "Inspector Rust v{} starting (logs: {:?})",
        env!("CARGO_PKG_VERSION"),
        logging::log_dir()
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(action) = cli_dispatch::parse_args(argv) {
                tracing::info!("CLI action (second instance): {action:?}");
                cli_dispatch::dispatch(app, action);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let db_path = db::default_db_path()?;
            tracing::info!("db at {}", db_path.display());

            // Initialise at-rest encryption *before* opening the DB so
            // every subsequent insert/select runs through the cipher.
            // The data dir is the same parent as the DB file.
            if let Some(data_dir) = db_path.parent() {
                if let Err(e) = crypto::init(data_dir) {
                    tracing::warn!("crypto init failed: {e:#} — DB will be plaintext");
                }
            }

            let db_handle = db::open(&db_path)?;

            snippets::init_table(&db_handle)?;
            notes::init_table(&db_handle)?;
            settings::init_table(&db_handle)?;
            totp_store::init_table(&db_handle)?;

            // One-shot migration: rewrite any pre-encryption rows in place so
            // the next read paths through the cipher cleanly. Idempotent —
            // already-encrypted rows are skipped. Runs on a WORKER, not the
            // setup path (v0.84.228): the scan is a per-row/per-column N+1
            // over entries+snippets+notes and used to block tray + hotkey
            // readiness on every launch; nothing downstream needs it done
            // synchronously because `crypto::decrypt` is permissive toward
            // legacy plaintext. A settings flag skips the scan entirely once
            // a pass completed cleanly (rows can only be legacy-plaintext if
            // written by a pre-v0.47 build — every current write encrypts).
            {
                let db = db_handle.clone();
                std::thread::spawn(move || {
                    const FLAG: &str = "crypto.migrated_v1";
                    if settings::get_bool(&db, FLAG, false).unwrap_or(false) {
                        return;
                    }
                    let mut total = 0usize;
                    let mut clean = true;
                    for (table, cols) in &[
                        ("entries", &["content_text", "content_data"][..]),
                        ("snippets", &["body"][..]),
                        ("notes", &["content_text", "content_data"][..]),
                    ] {
                        // Lock per table so the scan never monopolises the DB
                        // mutex against interactive use.
                        let conn = db.lock();
                        match crypto::migrate_table(&conn, table, cols) {
                            Ok(n) => total += n,
                            Err(e) => {
                                clean = false;
                                tracing::warn!("crypto migrate {table}: {e:#}");
                            }
                        }
                    }
                    if total > 0 {
                        tracing::info!("encrypted {total} legacy plaintext field(s)");
                    }
                    if clean {
                        let _ = settings::set(&db, FLAG, "true");
                    }
                });
            }

            // First-run: seed the curated default AI-prompt snippets.
            // Idempotent — runs once per database lifetime, then the
            // settings flag prevents re-import. User-deleted snippets
            // stay deleted.
            if let Err(e) = seed::maybe_seed_defaults(&db_handle) {
                tracing::warn!("default snippet seed failed: {e:#}");
            }

            let watcher_state = WatcherState::new();
            let paused = watcher_state.paused.clone();
            let self_written = watcher_state.self_written.clone();

            let ui_state = UiState::default();
            let suppress_hide = ui_state.suppress_hide.clone();
            let close_on_blur = ui_state.close_on_blur.clone();
            // Seed the persisted click-outside preference (default: close).
            close_on_blur.store(
                settings::get_bool(&db_handle, "popup.close_on_blur", true).unwrap_or(true),
                Ordering::SeqCst,
            );

            let expander_state = hotkey::ExpanderShortcutState::default();
            let popup_state = hotkey::PopupShortcutState::default();

            app.manage(db_handle.clone());
            app.manage(watcher_state);
            app.manage(ui_state);
            app.manage(expander_state);
            app.manage(popup_state);
            app.manage(screenshot_preview::PendingScreenshot::default());
            app.manage(wakelock::WakelockState::default());
            app.manage(cleaner::PlanStore::default());
            app.manage(screen_record::RecordState::default());
            app.manage(color_loupe::LoupeState::default());
            app.manage(alarm::AlarmState::default());
            app.manage(commands::MicCaptureState::default());
            app.manage(tracking::TrackerState::default());
            // Restore the last tracking state: if a session was still active
            // when the app last closed (crash / quit / update), resume it so
            // recording continues across the restart.
            {
                let ts = app.state::<tracking::TrackerState>();
                tracking::resume_if_active(app.handle(), &db_handle, ts.inner());
            }
            // Seed the curated default private-app filter list on first run
            // (music/messengers/streaming/… excluded from slots + exports).
            tracking::seed_private_apps(&db_handle);
            app.manage(commands::AudioSwapState::default());
            // Reap any caffeinate orphaned by a pre-v0.78.0 crash/reinstall so
            // a stale keep-awake assertion can't outlive the app that set it.
            wakelock::cleanup_orphans();
            // Reap any recording ffmpeg orphaned by a crash / failed stop — it
            // keeps capturing at high CPU otherwise.
            screen_record::cleanup_orphans();
            app.manage(timer::TimerRegistry::default());
            app.manage(status_toast::LatestToast::default());
            app.manage(auto_expand::AutoExpandState::default());
            app.manage(gestures::GestureState::default());
            app.manage(window_snap::WindowSnapState);
            app.manage(window_palette::WindowPaletteState);
            app.manage(hotkey::ActionShortcutState::default());

            // App-launcher cache. Manage an empty index immediately, then fill
            // it from a background thread — scanning ~200-400 .app bundles (or
            // walking the XDG / Start-Menu dirs) is ~20-100 ms and was blocking
            // the whole `setup` closure (and therefore the first hotkey's
            // readiness). The list is briefly empty after launch; the Settings
            // → Apps Refresh button re-triggers `refresh_apps` if needed.
            {
                app.manage(app_launcher::AppIndex::default());
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let apps = app_launcher::scan();
                    if let Some(index) = handle.try_state::<app_launcher::AppIndex>() {
                        let n = apps.len();
                        *index.apps.lock() = apps;
                        tracing::info!("app launcher: indexed {n} apps");
                    }
                });
            }

            // Apply the persisted popup-size preset so the window opens at
            // the user's chosen size from the first show (default: medium,
            // the 700×500 it ships with). Resize is queued on the main
            // thread; the window is hidden at this point, so the next
            // show_and_position recentres it with the new dimensions.
            commands::apply_window_size(app.handle(), &db_handle);

            // Seed the in-process feedback-sound toggle from settings (default
            // on) so the hot path never has to read the DB.
            sound::set_enabled(settings::get_bool(&db_handle, "sound.enabled", true).unwrap_or(true));

            if let Err(e) = hotkey::register(app.handle()) {
                tracing::warn!(
                    "global shortcut registration failed: {e:#} — use tray menu or CLI flags (linux/README.md)"
                );
            }

            // Popup hotkey — read user-configured string from settings,
            // fall back to default (Ctrl+Space). Separate from
            // `hotkey::register` because it's user-configurable + needs
            // re-registration at runtime from the settings panel. A one-time
            // migration bumps the pre-0.67 `Ctrl+Shift+V` default to
            // `Ctrl+Space` for un-customised installs (idempotent).
            {
                let stored = hotkey::migrate_legacy_popup_default(&db_handle);
                let popup_state = app.state::<hotkey::PopupShortcutState>();
                if let Err(e) = hotkey::register_popup(app.handle(), &popup_state, &stored) {
                    tracing::warn!(
                        "popup hotkey {stored:?} register failed: {e:#} — \
                         falling back to default {default}",
                        default = hotkey::DEFAULT_POPUP_HOTKEY,
                    );
                    // Best-effort fallback so the user can still open the popup.
                    let _ = hotkey::register_popup(
                        app.handle(),
                        &popup_state,
                        hotkey::DEFAULT_POPUP_HOTKEY,
                    );
                }
                // Second, optional clipboard-history hotkey (default Ctrl+Shift+V).
                let hist = settings::get_or(
                    &db_handle,
                    hotkey::KEY_HISTORY_HOTKEY,
                    hotkey::DEFAULT_HISTORY_HOTKEY,
                )
                .unwrap_or_else(|_| hotkey::DEFAULT_HISTORY_HOTKEY.to_string());
                if let Err(e) =
                    hotkey::register_history_hotkey(app.handle(), &popup_state, &hist)
                {
                    tracing::warn!("clipboard-history hotkey {hist:?} register failed: {e:#}");
                }
            }

            // Restore the expander hotkey from settings if it was enabled
            // last time the app ran. Default is disabled — opt-in. One-time
            // migration bumps the pre-0.12 `Alt+Backquote` default (broken
            // on German ISO Macs) to the layout-stable `Alt+Digit1`.
            {
                let enabled = settings::get_bool(&db_handle, expander::KEY_ENABLED, false)
                    .unwrap_or(false);
                let hotkey_str = expander::migrate_legacy_default(&db_handle);
                let state = app
                    .state::<hotkey::ExpanderShortcutState>();
                if let Err(e) = hotkey::register_expander(
                    app.handle(),
                    &state,
                    &hotkey_str,
                    enabled,
                ) {
                    tracing::warn!("expander hotkey register failed at startup: {e:#}");
                }

                // Direct hotkey→snippet slots (independent of the
                // abbreviation expander; the only mode that works in
                // terminals, since it pastes without reading anything).
                //
                // Before reading + arming the slots, sweep any whose
                // referenced snippet has been deleted — otherwise the
                // hotkey would silently no-op + spam the log. (v0.35.0+)
                match expander::prune_stale_direct_slots(&db_handle) {
                    Ok(n) if n > 0 => tracing::info!("pruned {n} stale direct slot(s) at startup"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("direct-slot prune at startup: {e:#}"),
                }
                match expander::get_direct_slots(&db_handle) {
                    Ok(slots) if !slots.is_empty() => {
                        if let Err(e) =
                            hotkey::register_direct_slots(app.handle(), &state, &slots)
                        {
                            tracing::warn!("direct-slot register failed at startup: {e:#}");
                        }
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("reading direct slots at startup: {e:#}"),
                }
            }

            // Passive auto-expansion (aText-style). Loads its config + builds
            // the abbreviation table; arms the platform key monitor only when
            // the setting is on (and, on macOS, Accessibility is granted —
            // otherwise it waits until the user enables it from Settings).
            {
                let ae_state = app.state::<auto_expand::AutoExpandState>();
                auto_expand::apply(app.handle(), &db_handle, &ae_state);
            }

            // Touchpad gestures (opt-in; off by default). Starts the OS capture
            // source only when enabled + supported on this platform.
            {
                let g_state = app.state::<gestures::GestureState>();
                gestures::migrate_tiptap_optin(&db_handle);
                gestures::migrate_volume_step_default(&db_handle);
                gestures::apply(app.handle(), &db_handle, g_state.inner());
                gestures::spawn_wake_watchdog(app.handle());
            }

            // Seed the faker expander's default locale from settings.
            faker::init_process_default(&db_handle);

            // Window snapping (opt-in; off by default). macOS-only monitor.
            {
                let ws = app.state::<window_snap::WindowSnapState>();
                window_snap::apply(app.handle(), &db_handle, ws.inner());
            }

            // System-stats history: always-on lightweight background sampler so
            // the Stats panel can show a "last hours / days" view.
            stats_history::start_collector(db_handle.clone());

            // Cloud sync with cue (snippets, opt-in via Settings).
            sync::start(app.handle().clone(), db_handle.clone());

            // Re-apply the last-chosen brightness/EDR levels (gamma dies with
            // the process; without this every restart resets to 100 %).
            brightness::restore_saved(app.handle(), &db_handle);

            // Window palette (opt-in; off by default). macOS-only hover monitor.
            {
                let wp = app.state::<window_palette::WindowPaletteState>();
                window_palette::apply(app.handle(), &db_handle, wp.inner());
            }

            // boom audio engine — (re)start if it was left enabled (off by default).
            // The handle first: the idle gate talks to the webview's warm
            // AudioContext via the warm-audio-suspend/-resume events.
            boom::set_app_handle(app.handle());
            boom::apply(&db_handle);

            clipboard_watcher::spawn(
                app.handle().clone(),
                db_handle.clone(),
                paused,
                self_written,
            );

            build_tray(app.handle())?;

            #[cfg(target_os = "linux")]
            {
                cli_dispatch::log_wayland_shortcut_hint();
                if let Err(e) = desktop_shortcuts::try_auto_install(&db_handle) {
                    tracing::warn!("desktop shortcut auto-setup: {e:#}");
                }
            }

            if let Some(action) = cli_dispatch::parse_args(std::env::args()) {
                tracing::info!("CLI action (startup): {action:?}");
                cli_dispatch::dispatch(app.handle(), action);
            }

            // Hide from macOS Dock — Inspector Rust is a tray-only background app.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            if let Some(window) = app.get_webview_window(hotkey::POPUP_LABEL) {
                let app_handle = app.handle().clone();
                #[cfg(target_os = "macos")]
                let win_for_events = window.clone();
                window.on_window_event(move |ev| {
                    match ev {
                        WindowEvent::Focused(true) => {
                            // Focus regained → the unfocused-Esc watcher is
                            // no longer needed.
                            #[cfg(target_os = "macos")]
                            esc_watch::disarm();
                            // Record that the popup genuinely received focus.
                            // The auto-hide guard uses this to distinguish a
                            // real click-away (Focused(false) after Focused(true))
                            // from a spurious OS message that arrives before the
                            // window ever had focus (Windows SetForegroundWindow
                            // race / z-order perturbation).
                            hotkey::mark_popup_focused();
                        }
                        WindowEvent::Focused(false) => {
                            // Windows: never decide synchronously — the WebView2
                            // focus bounce makes the instant state unreliable
                            // (it fires Focused(false) 700–900 ms post-show, past
                            // any fixed grace; the foreground PID snapshot is racy).
                            // Instead **confirm after a short settle**: a transient
                            // bounce resolves (foreground returns to us / Focused
                            // (true) re-fires, bumping SHOW_GEN) and cancels the
                            // hide; a real click-away keeps a foreign window
                            // foreground past the settle and proceeds. A
                            // Focused(false) before the first Focused(true) is the
                            // SetForegroundWindow-failed show-race → ignored.
                            // User preference (Settings → Popup behavior):
                            // click-outside must NOT close → skip every
                            // auto-hide path; Esc / the toggle hotkey remain
                            // the only ways to dismiss.
                            if !close_on_blur.load(Ordering::Relaxed) {
                                // The popup stays open while unfocused — the
                                // webview gets no key events now, so a
                                // listen-only global tap watches for Esc
                                // (macOS; consumes nothing).
                                #[cfg(target_os = "macos")]
                                if win_for_events.is_visible().unwrap_or(false) {
                                    esc_watch::arm(&app_handle);
                                }
                                return;
                            }
                            #[cfg(target_os = "windows")]
                            {
                                // `suppress_hide` is used on the macOS/Linux arm
                                // below (cfg'd out here) + read inside the settle
                                // re-check from state; touch it so the capture
                                // isn't flagged unused on Windows.
                                let _ = &suppress_hide;
                                if hotkey::popup_was_focused() {
                                    hotkey::schedule_settle_hide(&app_handle);
                                }
                            }
                            // macOS / Linux: no focus bounce — keep the immediate
                            // hide, gated by the modal-suppress flag + post-show
                            // grace window.
                            #[cfg(not(target_os = "windows"))]
                            #[allow(clippy::collapsible_match)]
                            if !suppress_hide.load(Ordering::Relaxed)
                                && !hotkey::within_show_grace()
                            {
                                hotkey::hide_popup(&app_handle);
                            }
                        }
                        _ => {}
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_history,
            commands::get_clip,
            commands::search_history,
            commands::paste_entry,
            commands::paste_entry_formatted,
            commands::get_paste_plain_text_only,
            commands::get_lineage_highlight,
            commands::set_lineage_highlight,
            commands::set_paste_plain_text_only,
            commands::get_ocr_save_source_image,
            commands::set_ocr_save_source_image,
            commands::get_input_lock_chord,
            commands::set_input_lock_chord,
            commands::start_input_lock,
            commands::delete_entry,
            commands::set_clip_pinned,
            commands::set_clip_note,
            commands::clear_history,
            commands::toggle_capture,
            commands::get_capture_state,
            commands::hide_popup,
            commands::paste_text,
            commands::list_snippets,
            commands::find_snippets,
            commands::upsert_snippet,
            commands::list_snippet_categories,
            commands::create_snippet_category,
            commands::rename_snippet_category,
            commands::delete_snippet_category,
            commands::reorder_snippet_categories,
            commands::set_snippet_category,
            commands::delete_snippet,
            commands::paste_snippet,
            commands::paste_note_formatted,
            commands::import_snippets,
            commands::import_snippets_from_file,
            commands::export_snippets_to_file,
            commands::restore_default_prompts,
            commands::set_suppress_hide,
            commands::get_popup_close_on_blur,
            commands::set_popup_close_on_blur,
            commands::list_notes,
            commands::list_note_categories,
            commands::save_clip_as_note,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            commands::clear_notes,
            commands::paste_note,
            commands::export_backup,
            commands::save_backup_to_file,
            commands::import_backup,
            commands::is_backup_encrypted,
            commands::get_expander_config,
            commands::set_expander_config,
            commands::get_auto_expand_config,
            commands::set_auto_expand_config,
            commands::get_sync_config,
            commands::set_sync_config,
            commands::get_sync_status,
            commands::sync_now,
            commands::sync_test_connection,
            commands::get_gesture_config,
            commands::set_gesture_config,
            commands::get_keepalive_enabled,
            commands::set_keepalive_enabled,
            commands::get_window_snap_config,
            commands::set_window_snap_config,
            commands::get_window_palette_config,
            commands::set_window_palette_config,
            commands::window_palette_context,
            commands::window_palette_apply,
            commands::window_palette_cancel,
            commands::window_palette_preview,
            commands::window_palette_preview_hide,
            commands::boom_available,
            commands::boom_presets,
            commands::boom_driver_installed,
            commands::boom_install_driver,
            commands::boom_uninstall_driver,
            commands::boom_levels,
            commands::get_boom_config,
            commands::set_boom_config,
            commands::list_action_hotkeys,
            commands::set_action_hotkey,
            commands::reset_action_hotkey,
            commands::get_popup_hotkey,
            commands::set_popup_hotkey,
            commands::get_popup_hotkey_default,
            commands::get_history_hotkey,
            commands::set_history_hotkey,
            commands::get_history_hotkey_default,
            commands::totp_list,
            commands::totp_add,
            commands::totp_delete,
            commands::totp_current_code,
            commands::totp_current_codes_all,
            commands::totp_import,
            commands::totp_import_file,
            commands::totp_set_order,
            commands::totp_update,
            commands::totp_remove_duplicates,
            commands::totp_delete_all,
            commands::totp_export,
            commands::trigger_expand_at_cursor,
            commands::diagnose_expand_at_cursor,
            commands::get_direct_slots,
            commands::set_direct_slots,
            commands::get_accessibility_status,
            commands::request_accessibility_grant,
            commands::open_accessibility_settings,
            commands::force_reset_and_request_grant,
            commands::quit_app,
            commands::relaunch_app,
            commands::get_autostart_enabled,
            commands::set_autostart_enabled,
            commands::pick_screen_color,
            commands::recolor_image_entry,
            commands::image_chromaticity,
            commands::qr_copy_png,
            commands::figlet_copy_png,
            commands::figlet_save_png,
            commands::cut_out_image_entry,
            commands::cut_out_image_file,
            commands::save_image_entry_to_downloads,
            commands::ocr_region,
            commands::screenshot_region,
            commands::screenshot_capture,
            commands::screenshot_repeat_last,
            screenshot_preview::get_pending_screenshot_path,
            screenshot_preview::get_pending_screenshot_info,
            screenshot_preview::get_pending_screenshot_data_url,
            screenshot_preview::set_screenshot_pinned,
            screenshot_preview::screenshot_preview_save,
            screenshot_preview::screenshot_preview_copy,
            screenshot_preview::screenshot_preview_discard,
            screenshot_preview::screenshot_preview_edit,
            screenshot_preview::reposition_preview_to_cursor,
            screenshot_preview::pin_current_screenshot,
            screenshot_preview::get_pin_image,
            screenshot_preview::close_pin,
            screenshot_editor::editor_save,
            screenshot_editor::editor_copy,
            screenshot_editor::editor_cancel,
            screenshot_editor::set_editor_size,
            commands::eyedropper_to_clipboard,
            commands::resize_clipboard_image,
            commands::optimize_clipboard_image,
            commands::remove_vowels_to_clipboard,
            commands::list_processes,
            commands::kill_process,
            commands::system_reboot,
            commands::system_shutdown,
            commands::system_lock,
            commands::adjust_volume,
            commands::get_system_volume,
            commands::set_system_volume,
            commands::toggle_mute,
            commands::wakelock_set,
            commands::wakelock_get,
            commands::bruno_get_defaults,
            commands::bruno_set_defaults,
            commands::faker_catalog,
            commands::faker_generate,
            commands::faker_locales,
            commands::faker_get_defaults,
            commands::faker_set_defaults,
            commands::figlet_fonts,
            commands::figlet_render,
            commands::figlet_gallery,
            commands::figlet_get_defaults,
            commands::figlet_set_defaults,
            commands::paste_generated,
            commands::sec_catalog,
            commands::sec_get_defaults,
            commands::sec_set_defaults,
            commands::sec_open_in_terminal,
            commands::sec_path_exists,
            commands::list_apps,
            commands::refresh_apps,
            commands::launch_app,
            commands::get_app_icon,
            commands::start_timer,
            commands::cancel_timer,
            commands::list_timers,
            commands::get_finder_selection,
            commands::resize_file,
            commands::optimize_file,
            commands::finder_touch,
            commands::finder_mkdir,
            commands::finder_open_terminal,
            commands::md_to_pdf_run,
            commands::show_status_toast,
            commands::get_finder_automation_status,
            commands::open_finder_automation_settings,
            commands::force_reset_finder_automation_grant,
            commands::commit_transformed_text,
            commands::get_theme_preference,
            commands::set_theme_preference,
            commands::get_sound_enabled,
            commands::set_sound_enabled,
            commands::get_clipboard_privacy,
            commands::set_clipboard_privacy,
            commands::get_window_size_preference,
            commands::set_window_size_preference,
            commands::get_status_toast,
            commands::hide_status_toast,
            commands::cleaner_scan,
            commands::cleaner_execute,
            commands::cleaner_categories,
            commands::get_cleaner_config,
            commands::set_cleaner_config,
            #[cfg(target_os = "macos")]
            commands::snitch_list_apps,
            #[cfg(target_os = "macos")]
            commands::snitch_connections,
            #[cfg(target_os = "macos")]
            commands::snitch_geolocate,
            #[cfg(target_os = "macos")]
            commands::snitch_activity,
            #[cfg(target_os = "macos")]
            commands::snitch_home,
            #[cfg(target_os = "macos")]
            commands::snitch_set_blocked,
            #[cfg(target_os = "macos")]
            commands::snitch_is_armed,
            #[cfg(target_os = "macos")]
            commands::snitch_arm,
            #[cfg(target_os = "macos")]
            commands::snitch_disarm,
            commands::mic_capture_start,
            commands::mic_capture_stop,
            commands::shazam_recognize,
            commands::shazam_listen,
            commands::shazam_history_list,
            commands::shazam_history_delete,
            commands::shazam_history_clear,
            commands::shazam_lyrics,
            commands::shazam_lyrics_translated,
            commands::translate_text,
            commands::list_memes,
            commands::copy_meme,
            commands::get_meme_dir,
            commands::set_meme_dir,
            commands::list_brightness_monitors,
            commands::get_monitor_brightness,
            commands::set_monitor_brightness,
            commands::set_edr_level,
            commands::list_audio_outputs,
            commands::set_audio_output,
            commands::get_system_stats,
            commands::get_stats_history,
            commands::get_uptime_secs,
            commands::color_loupe_data,
            commands::color_loupe_pick,
            commands::color_loupe_cancel,
            commands::get_alarm_style,
            commands::set_alarm_style,
            commands::alarm_overlay_label,
            commands::stop_alarm,
            commands::track_start,
            commands::track_stop,
            commands::track_status,
            commands::track_set_paused,
            commands::track_get_day,
            commands::track_get_range,
            commands::track_slots,
            commands::track_bcsbook_preview,
            commands::track_push_bcsbook,
            commands::track_slots_range,
            commands::get_slot_config,
            commands::set_slot_config,
            commands::track_update_event,
            commands::track_delete_event,
            commands::track_merge_events,
            commands::track_set_category,
            commands::track_category_rules,
            commands::track_delete_category_rule,
            commands::track_distinct_categories,
            commands::track_set_project,
            commands::track_distinct_projects,
            commands::track_add_event,
            commands::track_cleanup_day,
            commands::track_cleanup_range,
            commands::track_clear_all,
            commands::track_export,
            commands::track_export_projects,
            commands::track_bridge_info,
            commands::track_bridge_regenerate,
            commands::track_export_extension,
            commands::get_timesheet_config,
            commands::set_timesheet_config,
            commands::hue_status,
            commands::hue_discover,
            commands::hue_set_bridge_ip,
            commands::hue_pair,
            commands::hue_forget,
            commands::hue_list_lights,
            commands::hue_set_light,
            commands::hue_set_all,
            commands::screen_record_open_overlay,
            commands::cancel_record_overlay,
            commands::start_screen_record,
            commands::pause_screen_record,
            commands::resume_screen_record,
            commands::stop_screen_record,
            commands::is_recording,
            commands::open_audio_swap_overlay,
            commands::audio_swap_get_selected_video,
            commands::audio_swap_probe,
            commands::audio_swap_ytdlp_available,
            commands::audio_swap_download_youtube,
            commands::audio_swap_apply,
            commands::audio_swap_cancel_overlay,
            commands::social_ytdlp_available,
            commands::social_download,
            commands::trim_open_overlay,
            commands::trim_cancel_overlay,
            commands::trim_file_info,
            commands::trim_apply,
            commands::brightness_open,
            commands::brightness_close,
            commands::get_screen_recording_status,
            commands::request_screen_recording_grant,
            commands::open_screen_recording_settings,
            commands::force_reset_screen_recording_grant,
            #[cfg(target_os = "linux")]
            commands::linux_scan_desktop_shortcuts,
            #[cfg(target_os = "linux")]
            commands::linux_apply_desktop_shortcuts,
            #[cfg(target_os = "linux")]
            commands::linux_web_hotkey_to_gsettings,
        ])
        .build(context)
        .expect("error while building Inspector Rust")
        .run(|_app, event| {
            // Tear the boom audio engine down on quit so the system output is
            // never left muted (muted tap = our IOProc is the only path).
            if matches!(event, tauri::RunEvent::Exit) {
                boom::shutdown();
            }
        });
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let open_item = MenuItemBuilder::with_id("open", "Open (Ctrl+Space)").build(app)?;
    let settings_item = MenuItemBuilder::with_id("settings", "Settings…").build(app)?;
    let snippets_item = MenuItemBuilder::with_id("snippets", "Manage Snippets").build(app)?;
    let notes_item = MenuItemBuilder::with_id("notes", "Manage Notes").build(app)?;
    let timesheet_item = MenuItemBuilder::with_id("timesheet", "Timesheet").build(app)?;
    // Both global shortcuts use literal Control on every OS since
    // v0.14.1 — the macOS glyph for Control is ⌃ (not ⌘).
    let ocr_label = if cfg!(target_os = "macos") {
        "OCR Region (⌃⇧O)"
    } else {
        "OCR Region (Ctrl+Shift+O)"
    };
    let ocr_item = MenuItemBuilder::with_id("ocr", ocr_label).build(app)?;
    let screenshot_label = if cfg!(target_os = "macos") {
        "Screenshot Region (⌃⇧S)"
    } else {
        "Screenshot Region (Ctrl+Shift+S)"
    };
    let screenshot_item = MenuItemBuilder::with_id("screenshot", screenshot_label).build(app)?;
    let color_label = if cfg!(target_os = "macos") {
        "Pick Color (⌃⇧C)"
    } else {
        "Pick Color (Ctrl+Shift+C)"
    };
    let color_item = MenuItemBuilder::with_id("color", color_label).build(app)?;
    let record_label = if cfg!(target_os = "macos") {
        "Screen Recording (⌃⇧⌥S)"
    } else {
        "Screen Recording (Ctrl+Shift+Alt+S)"
    };
    let record_item = MenuItemBuilder::with_id("record", record_label).build(app)?;
    // Finder/Explorer selection actions — macOS only (Automation-backed).
    #[cfg(target_os = "macos")]
    let finder_item = MenuItemBuilder::with_id("finder", "Finder Selection (⌃⇧F)").build(app)?;
    let pause_item = MenuItemBuilder::with_id("pause", "Pause Capture").build(app)?;
    let clear_item = MenuItemBuilder::with_id("clear", "Clear History…").build(app)?;
    let autostart_label = if cfg!(target_os = "windows") {
        "Start with Windows"
    } else {
        "Start at Login"
    };
    // Probe the current state on tray build so the checkmark reflects
    // reality at launch — including the case where the plist / registry
    // entry was created outside the app.
    let autostart_state = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart_item = CheckMenuItemBuilder::with_id("autostart", autostart_label)
        .checked(autostart_state)
        .build(app)?;
    // The click handler needs to update the checkmark after toggling, so
    // keep a clone for the closure. `CheckMenuItem<R>` is a cheap handle.
    let autostart_item_for_handler = autostart_item.clone();
    let sep = PredefinedMenuItem::separator(app)?;
    let sep_ocr = PredefinedMenuItem::separator(app)?;
    let sep_manage = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit Inspector Rust").build(app)?;

    let menu = MenuBuilder::new(app);
    // Top: open + navigate to tabs.
    let menu = menu.items(&[
        &open_item,
        &settings_item,
        &sep_manage,
        &snippets_item,
        &notes_item,
        &timesheet_item,
        &sep_ocr,
        // Capture / one-shot actions.
        &ocr_item,
        &screenshot_item,
        &color_item,
        &record_item,
    ]);
    #[cfg(target_os = "macos")]
    let menu = menu.item(&finder_item);
    let menu = menu
        .items(&[
            &sep,
            &pause_item,
            &autostart_item,
            &clear_item,
            &sep2,
            &quit_item,
        ])
        .build()?;

    let mut tray_builder = TrayIconBuilder::with_id("main")
        .tooltip("Inspector Rust")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => cli_dispatch::dispatch(app, cli_dispatch::CliAction::TogglePopup),
            "settings" => {
                if let Err(e) = hotkey::show_popup(app) {
                    tracing::warn!("show popup for settings: {e:#}");
                }
                let _ = app.emit("open-settings-tab", ());
            }
            "timesheet" => hotkey::dispatch_action(app, hotkey::ActionId::Timesheet),
            "record" => hotkey::dispatch_action(app, hotkey::ActionId::Record),
            "finder" => hotkey::dispatch_action(app, hotkey::ActionId::Finder),
            "snippets" => {
                if let Err(e) = hotkey::show_popup(app) {
                    tracing::warn!("show popup for snippets: {e:#}");
                }
                let _ = app.emit("open-snippets-tab", ());
            }
            "notes" => {
                if let Err(e) = hotkey::show_popup(app) {
                    tracing::warn!("show popup for notes: {e:#}");
                }
                let _ = app.emit("open-notes-tab", ());
            }
            "ocr" => cli_dispatch::dispatch(app, cli_dispatch::CliAction::Ocr),
            "screenshot" => cli_dispatch::dispatch(app, cli_dispatch::CliAction::Screenshot),
            "color" => cli_dispatch::dispatch(app, cli_dispatch::CliAction::PickColor),
            "pause" => {
                if let Some(state) = app.try_state::<WatcherState>() {
                    let now = state.paused.load(Ordering::Relaxed);
                    state.paused.store(!now, Ordering::Relaxed);
                    let _ = app.emit("capture-state-changed", !now);
                }
            }
            "clear" => {
                // Native confirm — the tray menu has no UI surface of its own,
                // so we can't reuse the popup's `window.confirm` flow without
                // first showing the popup. A modal dialog keeps the user where
                // they are; the OK button is destructive on macOS / labeled
                // "Yes" on Windows.
                let app2 = app.clone();
                app.dialog()
                    .message("Delete all clipboard history? This cannot be undone.")
                    .title("Inspector Rust")
                    .kind(MessageDialogKind::Warning)
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        "Delete".to_string(),
                        "Cancel".to_string(),
                    ))
                    .show(move |confirmed| {
                        if !confirmed {
                            return;
                        }
                        if let Some(db) = app2.try_state::<db::DbHandle>() {
                            if let Err(e) = db::clear(&db) {
                                tracing::warn!("clear: {e:#}");
                            }
                            let _ = app2.emit("clipboard-changed", ());
                        }
                    });
            }
            "autostart" => {
                let am = app.autolaunch();
                let was_enabled = am.is_enabled().unwrap_or(false);
                let res = if was_enabled {
                    am.disable()
                } else {
                    am.enable()
                };
                match res {
                    Ok(()) => {
                        // Read back what the OS now reports (rather than
                        // trusting `!was_enabled` — guards against the
                        // toggle silently failing without an Err return).
                        let now = am.is_enabled().unwrap_or(!was_enabled);
                        let _ = autostart_item_for_handler.set_checked(now);
                        let _ = app.emit("autostart-changed", now);
                    }
                    Err(e) => tracing::warn!("autostart toggle: {e:#}"),
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        });
    // Monochrome fedora tray icon. On **macOS** it's set as a *template* image,
    // so the system inverts it for light/dark menu bars automatically (the
    // silhouette is the alpha channel). On Windows/Linux it shows as-is. Falls
    // back to the bundled app icon if the embedded PNG can't be decoded —
    // never panic at startup over a cosmetic tray icon.
    match tauri::image::Image::from_bytes(include_bytes!("../assets/tray-hat.png")) {
        Ok(icon) => {
            tray_builder = tray_builder.icon(icon);
            #[cfg(target_os = "macos")]
            {
                tray_builder = tray_builder.icon_as_template(true);
            }
        }
        Err(e) => {
            tracing::warn!("tray hat icon failed to load ({e}); using the app icon");
            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
        }
    }
    let _tray = tray_builder.build(app)?;

    Ok(())
}
