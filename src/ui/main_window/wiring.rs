use futures_channel::oneshot;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Button, ButtonsType, DialogFlags, MessageDialog, MessageType, Stack,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

use super::{
    HeaderControls, KickertoolControls, MainWindow, ManualControls, ModeButtons, ServerContext,
};
use crate::models::{Table, Tournament};
use crate::services::{
    ApiError, KickertoolApiService, LogCallback, ObsConnectionState, ObsService, OverlayMode,
    OverlayStateManager, Settings, SettingsService, TableMonitor, log,
};
use crate::ui::{DebugLog, SettingsDialog};

impl MainWindow {
    pub(super) fn run_async_into_ui<T, Fut, OnResult>(
        runtime: &Arc<Runtime>,
        future: Fut,
        on_result: OnResult,
    ) where
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        OnResult: FnOnce(T) + 'static,
    {
        let (tx, rx) = oneshot::channel();

        runtime.spawn(async move {
            let result = future.await;
            let _ = tx.send(result);
        });

        glib::spawn_future_local(async move {
            if let Ok(result) = rx.await {
                on_result(result);
            }
        });
    }

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
        let form = manual_controls.form.clone();
        let rt = Arc::clone(&runtime);
        manual_controls.update_button.connect_clicked(move |_| {
            let values = Self::read_manual_entries(&form);

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
                if let Some((new_settings, api_key, obs_password)) = result {
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
                    if let Err(e) = settings_service.save_obs_password(&obs_password) {
                        log(
                            &log_callback,
                            "Settings",
                            format!("Failed to save OBS password: {}", e),
                        );
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

    pub(super) fn connect_obs_buttons(
        header: &HeaderControls,
        obs_service: Arc<ObsService>,
        settings_service: Arc<SettingsService>,
        settings: Arc<RwLock<Settings>>,
        runtime: Arc<Runtime>,
    ) {
        let obs = Arc::clone(&obs_service);
        let ss = Arc::clone(&settings_service);
        let s = Arc::clone(&settings);
        let rt = Arc::clone(&runtime);
        let connect_btn = header.obs_connect_button.clone();
        let pause_btn = header.obs_pause_button.clone();
        let status_label = header.obs_status_label.clone();
        header.obs_connect_button.connect_clicked(move |_| {
            let obs = Arc::clone(&obs);
            let ss = Arc::clone(&ss);
            let s = Arc::clone(&s);
            let connect_btn = connect_btn.clone();
            let pause_btn = pause_btn.clone();
            let status_label = status_label.clone();

            let is_connected = rt.block_on({
                let obs = Arc::clone(&obs);
                async move { matches!(obs.get_state().await, ObsConnectionState::Connected) }
            });

            if is_connected {
                rt.spawn(async move {
                    obs.disconnect().await;
                });
                connect_btn.set_label("Connect OBS");
                pause_btn.set_sensitive(false);
                status_label.set_text("OBS: Not connected");
                return;
            }

            let password = ss.load_obs_password().unwrap_or_default();
            connect_btn.set_sensitive(false);
            status_label.set_text("OBS: Connecting...");

            Self::run_async_into_ui(
                &rt,
                async move {
                    let port = s.read().await.obs_port;
                    obs.connect(port, password).await;
                    obs.get_state().await
                },
                move |state| {
                    connect_btn.set_sensitive(true);
                    match state {
                        ObsConnectionState::Connected => {
                            status_label.set_text("OBS: Connected");
                            connect_btn.set_label("Disconnect OBS");
                            pause_btn.set_sensitive(true);
                        }
                        ObsConnectionState::Error(msg) => {
                            status_label.set_text(&format!("OBS: {msg}"));
                            connect_btn.set_label("Connect OBS");
                            pause_btn.set_sensitive(false);
                        }
                        _ => {
                            status_label.set_text("OBS: Not connected");
                            connect_btn.set_label("Connect OBS");
                            pause_btn.set_sensitive(false);
                        }
                    }
                },
            );
        });

        let obs = Arc::clone(&obs_service);
        let rt = Arc::clone(&runtime);
        header.obs_pause_button.connect_clicked(move |_| {
            let obs = Arc::clone(&obs);
            rt.spawn(async move {
                obs.switch_scene("Pause".to_string()).await;
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

    fn show_kickertool_access_dialog(parent: &ApplicationWindow, message: &str) {
        let dialog = MessageDialog::new(
            Some(parent),
            DialogFlags::MODAL,
            MessageType::Error,
            ButtonsType::Ok,
            "Kickertool API access problem",
        );
        dialog.format_secondary_text(Some(message));
        dialog.connect_response(|dialog, _| {
            dialog.close();
        });
        dialog.present();
    }

    pub(super) fn connect_refresh_tournaments(
        kickertool: &KickertoolControls,
        parent: &ApplicationWindow,
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
        let parent = parent.clone();
        let rt = Arc::clone(&runtime);
        kickertool.refresh_button.connect_clicked(move |_| {
            let api = Arc::clone(&api_service_clone);
            let tournaments = Rc::clone(&tournaments_clone);
            let tables = Rc::clone(&tournament_tables_clone);
            let combo = tournament_combo_clone.clone();
            let log_callback = Arc::clone(&log_callback_clone);
            let parent = parent.clone();

            combo.remove_all();
            log(&log_callback, "Kickertool", "Loading tournaments...");

            Self::run_async_into_ui(
                &rt,
                async move { api.load_tournaments_with_tables().await },
                move |result| match result {
                    Ok((all_tournaments, all_tables)) => {
                        log(
                            &log_callback,
                            "Kickertool",
                            format!("Loaded {} tournaments", all_tournaments.len()),
                        );
                        if all_tournaments.is_empty() {
                            Self::show_kickertool_access_dialog(
                                &parent,
                                "No tournaments were returned for this API key. Check that the key is valid and has access to the expected tournaments.",
                            );
                        }
                        for tournament in &all_tournaments {
                            combo.append(Some(&tournament.id), &tournament.name);
                        }
                        *tournaments.borrow_mut() = all_tournaments;
                        *tables.borrow_mut() = all_tables;
                    }
                    Err(e) => {
                        log(
                            &log_callback,
                            "Kickertool",
                            format!("Failed to load tournaments: {}", e),
                        );
                        if let Some(api_error) = e.downcast_ref::<ApiError>() {
                            let message = match api_error {
                                ApiError::Unauthorized => {
                                    "The Kickertool API key is invalid or expired. Create a new key in Kickertool and save it in Settings."
                                }
                                ApiError::Forbidden => {
                                    "The Kickertool API key does not have access to tournament data for this account."
                                }
                                ApiError::RequestFailed { .. } => {
                                    "Kickertool rejected the API request. Check the Debug log for the HTTP status and response."
                                }
                            };
                            Self::show_kickertool_access_dialog(&parent, message);
                        }
                    }
                },
            );
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
