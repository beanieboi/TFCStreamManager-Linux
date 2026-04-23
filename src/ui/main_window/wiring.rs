use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Button, Stack};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

use super::{
    HeaderControls, KickertoolControls, MainWindow, ManualControls, ModeButtons, ServerContext,
};
use crate::models::{Table, Tournament};
use crate::services::{
    KickertoolApiService, LogCallback, OverlayMode, OverlayStateManager, Settings, SettingsService,
    TableMonitor, log,
};
use crate::ui::{DebugLog, SettingsDialog};

impl MainWindow {
    pub(super) fn connect_mode_buttons(
        mode_buttons: &ModeButtons,
        content_stack: &Stack,
        refresh_button: &Button,
        overlay_state: OverlayStateManager,
        runtime: Arc<Runtime>,
    ) {
        let stack_clone = content_stack.clone();
        let overlay_state_clone = overlay_state.clone();
        let refresh_btn = refresh_button.clone();
        let rt = Arc::clone(&runtime);
        mode_buttons.kickertool.connect_toggled(move |btn| {
            if btn.is_active() {
                stack_clone.set_visible_child_name("kickertool");
                Self::spawn_set_mode(&rt, overlay_state_clone.clone(), OverlayMode::Kickertool);
                refresh_btn.emit_clicked();
            }
        });

        let stack_clone = content_stack.clone();
        let overlay_state_clone = overlay_state.clone();
        let rt = Arc::clone(&runtime);
        mode_buttons.remote.connect_toggled(move |btn| {
            if btn.is_active() {
                stack_clone.set_visible_child_name("remote");
                let overlay = overlay_state_clone.clone();
                rt.spawn(async move {
                    overlay.set_mode(OverlayMode::Remote).await;
                    overlay.set_content(Self::remote_default_content()).await;
                });
            }
        });

        let stack_clone = content_stack.clone();
        let overlay_state_clone = overlay_state.clone();
        let rt = Arc::clone(&runtime);
        mode_buttons.manual.connect_toggled(move |btn| {
            if btn.is_active() {
                stack_clone.set_visible_child_name("manual");
                Self::spawn_set_mode(&rt, overlay_state_clone.clone(), OverlayMode::Manual);
            }
        });
    }

    pub(super) fn connect_manual_update(
        manual_controls: &ManualControls,
        overlay_state: OverlayStateManager,
        runtime: Arc<Runtime>,
    ) {
        let overlay_state_clone = overlay_state.clone();
        let entries_clone = manual_controls.entries.clone();
        let rt = Arc::clone(&runtime);
        manual_controls.update_button.connect_clicked(move |_| {
            let values = Self::read_manual_entries(&entries_clone);

            let overlay = overlay_state_clone.clone();
            rt.spawn(async move {
                overlay.set_content(Self::manual_content(values)).await;
            });
        });
    }

    pub(super) fn connect_settings_dialog(
        settings_button: &Button,
        parent: &ApplicationWindow,
        settings_service: Arc<SettingsService>,
        settings: Arc<RwLock<Settings>>,
        api_service: Arc<KickertoolApiService>,
        log_callback: LogCallback,
        runtime: Arc<Runtime>,
    ) {
        let parent = parent.clone();
        settings_button.connect_clicked(move |_| {
            let dialog = SettingsDialog::new(&parent, Arc::clone(&settings_service));
            let settings_service = Arc::clone(&settings_service);
            let settings_arc = Arc::clone(&settings);
            let api = Arc::clone(&api_service);
            let log_callback = Arc::clone(&log_callback);
            let rt = Arc::clone(&runtime);

            dialog.run(move |result| {
                if let Some((new_settings, api_key)) = result {
                    if let Err(e) = settings_service.save(&new_settings) {
                        log(
                            &log_callback,
                            "Settings",
                            format!("Failed to save settings: {}", e),
                        );
                    }
                    if let Err(e) = settings_service.save_api_key(&api_key) {
                        log(
                            &log_callback,
                            "Settings",
                            format!("Failed to save API key: {}", e),
                        );
                    } else {
                        log(&log_callback, "Settings", "API key saved successfully");
                    }

                    let settings_arc = Arc::clone(&settings_arc);
                    let api = Arc::clone(&api);
                    rt.spawn(async move {
                        let mut s = settings_arc.write().await;
                        *s = new_settings;
                        api.update_api_key(api_key).await;
                    });
                }
            });
        });
    }

    pub(super) fn connect_debug_button(
        debug_button: &Button,
        parent: &ApplicationWindow,
        debug_log: Rc<DebugLog>,
    ) {
        let parent = parent.clone();
        debug_button.connect_clicked(move |_| {
            debug_log.show(&parent);
        });
    }

    pub(super) fn connect_server_buttons(header: &HeaderControls, context: ServerContext) {
        let stop_btn_clone = header.stop_button.clone();
        let start_btn_clone2 = header.start_button.clone();
        let url_label_clone = header.server_url_label.clone();
        let context = Rc::new(context);
        let context_for_start = Rc::clone(&context);
        header.start_button.connect_clicked(move |_| {
            let start_btn = start_btn_clone2.clone();
            let stop_btn = stop_btn_clone.clone();
            let url_label = url_label_clone.clone();
            let context = Rc::clone(&context_for_start);
            Self::start_server(context, start_btn, stop_btn, url_label);
        });

        let start_btn_clone = header.start_button.clone();
        let url_label_clone = header.server_url_label.clone();
        let context_for_stop = Rc::clone(&context);
        header.stop_button.connect_clicked(move |btn| {
            let context = Rc::clone(&context_for_stop);
            Self::stop_server(context);

            start_btn_clone.set_sensitive(true);
            btn.set_sensitive(false);
            url_label_clone.set_text("Not running");
        });
    }

    pub(super) fn connect_refresh_tournaments(
        kickertool: &KickertoolControls,
        api_service: Arc<KickertoolApiService>,
        tournaments: Rc<RefCell<Vec<Tournament>>>,
        tournament_tables: Rc<RefCell<HashMap<String, Vec<Table>>>>,
        log_callback: LogCallback,
        runtime: Arc<Runtime>,
    ) {
        let api_service_clone = Arc::clone(&api_service);
        let tournaments_clone = Rc::clone(&tournaments);
        let tournament_tables_clone = Rc::clone(&tournament_tables);
        let tournament_combo_clone = kickertool.tournament_combo.clone();
        let log_callback_clone = Arc::clone(&log_callback);
        let rt = Arc::clone(&runtime);
        kickertool.refresh_button.connect_clicked(move |_| {
            let api = Arc::clone(&api_service_clone);
            let tournaments = Rc::clone(&tournaments_clone);
            let tables = Rc::clone(&tournament_tables_clone);
            let combo = tournament_combo_clone.clone();
            let log_callback = Arc::clone(&log_callback_clone);
            let rt = Arc::clone(&rt);

            combo.remove_all();

            let (tx, rx) = std::sync::mpsc::channel();

            rt.spawn(async move {
                let result = api.load_tournaments_with_tables().await;
                let _ = tx.send(result);
            });

            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                match rx.try_recv() {
                    Ok(Ok((all_tournaments, all_tables))) => {
                        for tournament in &all_tournaments {
                            combo.append(Some(&tournament.id), &tournament.name);
                        }
                        *tournaments.borrow_mut() = all_tournaments;
                        *tables.borrow_mut() = all_tables;
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        log(
                            &log_callback,
                            "Kickertool",
                            format!("Failed to load tournaments: {}", e),
                        );
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
                }
            });
        });
    }

    pub(super) fn connect_tournament_selection(
        kickertool: &KickertoolControls,
        tournaments: Rc<RefCell<Vec<Tournament>>>,
        tournament_tables: Rc<RefCell<HashMap<String, Vec<Table>>>>,
        selected_tournament: Rc<RefCell<Option<Tournament>>>,
    ) {
        let tournament_tables_clone = Rc::clone(&tournament_tables);
        let tournaments_clone = Rc::clone(&tournaments);
        let selected_tournament_clone = Rc::clone(&selected_tournament);
        let table_combo_clone = kickertool.table_combo.clone();
        kickertool.tournament_combo.connect_changed(move |combo| {
            table_combo_clone.remove_all();

            if let Some(id) = combo.active_id() {
                let id_str = id.to_string();

                let tournaments = tournaments_clone.borrow();
                if let Some(tournament) = tournaments.iter().find(|t| t.id == id_str) {
                    *selected_tournament_clone.borrow_mut() = Some(tournament.clone());

                    let tables = tournament_tables_clone.borrow();
                    if let Some(tournament_tables) = tables.get(&id_str) {
                        for table in tournament_tables {
                            table_combo_clone.append(Some(&table.id), &table.to_string());
                        }
                    }
                }
            } else {
                *selected_tournament_clone.borrow_mut() = None;
            }
        });
    }

    pub(super) fn connect_table_selection(
        kickertool: &KickertoolControls,
        selected_tournament: Rc<RefCell<Option<Tournament>>>,
        table_monitor: TableMonitor,
        runtime: Arc<Runtime>,
    ) {
        let selected_tournament_clone = Rc::clone(&selected_tournament);
        let table_monitor_clone = table_monitor.clone();
        let rt = Arc::clone(&runtime);
        kickertool.table_combo.connect_changed(move |combo| {
            if let Some(id) = combo.active_id() {
                let id_str = id.to_string();

                let tournament_opt = selected_tournament_clone.borrow().clone();
                if let Some(tournament) = tournament_opt {
                    let monitor = table_monitor_clone.clone();
                    rt.spawn(async move {
                        monitor.start_monitoring(tournament, id_str).await;
                    });
                }
            } else {
                let monitor = table_monitor_clone.clone();
                rt.spawn(async move {
                    monitor.stop_monitoring().await;
                });
            }
        });
    }
}
