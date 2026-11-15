use futures_channel::oneshot;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Button, ButtonsType, DialogFlags, MessageDialog, MessageType};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;

use super::{KickertoolControls, MainWindow, ServerContext};
use crate::competition::Competition;
use crate::models::{Table, Tournament};
use crate::services::{
    ApiError, KickertoolApiService, MatchStore, TableMonitor, is_kickertool_tournament,
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

    /// Saves the settings and restarts the web server when the port changed.
    pub(super) fn connect_settings_dialog(
        settings_button: &Button,
        parent: &ApplicationWindow,
        api_service: Arc<KickertoolApiService>,
        context: Rc<ServerContext>,
    ) {
        let parent = parent.clone();
        settings_button.connect_clicked(move |_| {
            let dialog = SettingsDialog::new(&parent, Arc::clone(&context.settings_service));
            let api = Arc::clone(&api_service);
            let context = Rc::clone(&context);

            dialog.run(move |result| {
                let Some((mut new_settings, api_key)) = result else {
                    return;
                };
                if let Err(e) = context.settings_service.save_api_key(&api_key) {
                    tracing::error!("Failed to save API key: {}", e);
                } else {
                    tracing::info!("API key saved successfully");
                }

                let (port_changed, settings) = context.runtime.block_on(async {
                    let mut settings = context.settings.write().await;
                    let changed = settings.port != new_settings.port;
                    // Chosen in the toolbar, not in the dialog.
                    new_settings.overlay_choices = settings.overlay_choices.clone();
                    *settings = new_settings;
                    api.update_api_key(api_key).await;
                    (changed, settings.clone())
                });
                Self::save_settings(&context);
                // Possibly another overlay folder; open pages reload with
                // its files.
                context.settings_service.use_overlay_dir(&settings);
                Self::show_overlays(&context);
                if port_changed {
                    Self::restart_server(&context);
                }
            });
        });
    }

    /// Opens the overlay of the running server in the default browser.
    pub(super) fn connect_overlay_button(
        overlay_button: &Button,
        parent: &ApplicationWindow,
        context: Rc<ServerContext>,
    ) {
        let parent = parent.clone();
        overlay_button.connect_clicked(move |_| {
            let port = context
                .runtime
                .block_on(async { context.settings.read().await.port });
            gtk4::show_uri(Some(&parent), &Self::overlay_url(port), 0);
        });
    }

    pub(super) fn connect_retry_button(retry_button: &Button, context: Rc<ServerContext>) {
        retry_button.connect_clicked(move |_| Self::restart_server(&context));
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

    fn show_kickertool_access_dialog(parent: &ApplicationWindow, message: &str) {
        let dialog = MessageDialog::new(
            Some(parent),
            DialogFlags::MODAL,
            MessageType::Error,
            ButtonsType::Ok,
            "Problem beim Zugriff auf die Kickertool-API",
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
        runtime: Arc<Runtime>,
    ) {
        let api_service_clone = Arc::clone(&api_service);
        let tournaments_clone = Rc::clone(&tournaments);
        let tournament_tables_clone = Rc::clone(&tournament_tables);
        let tournament_combo_clone = kickertool.tournament_combo.clone();
        let parent = parent.clone();
        let rt = Arc::clone(&runtime);
        kickertool.refresh_button.connect_clicked(move |_| {
            let api = Arc::clone(&api_service_clone);
            let tournaments = Rc::clone(&tournaments_clone);
            let tables = Rc::clone(&tournament_tables_clone);
            let combo = tournament_combo_clone.clone();
            let parent = parent.clone();

            combo.remove_all();
            tracing::debug!("Loading tournaments...");

            Self::run_async_into_ui(
                &rt,
                async move { api.load_tournaments_with_tables().await },
                move |result| match result {
                    Ok((all_tournaments, all_tables)) => {
                        tracing::info!("Loaded {} tournaments", all_tournaments.len());
                        if all_tournaments.is_empty() {
                            Self::show_kickertool_access_dialog(
                                &parent,
                                "Für diesen API-Schlüssel wurden keine Turniere gefunden. Prüfe, ob der Schlüssel gültig ist und Zugriff auf die Turniere hat.",
                            );
                        }
                        for tournament in &all_tournaments {
                            combo.append(Some(&tournament.id), &tournament.name);
                        }
                        *tournaments.borrow_mut() = all_tournaments;
                        *tables.borrow_mut() = all_tables;
                    }
                    Err(e) => {
                        tracing::error!("Failed to load tournaments: {}", e);
                        if let Some(api_error) = e.downcast_ref::<ApiError>() {
                            let message = match api_error {
                                ApiError::Unauthorized => {
                                    "Der Kickertool-API-Schlüssel ist ungültig oder abgelaufen. Erstelle in Kickertool einen neuen Schlüssel und speichere ihn in den Einstellungen."
                                }
                                ApiError::Forbidden => {
                                    "Der Kickertool-API-Schlüssel hat keinen Zugriff auf die Turnierdaten dieses Kontos."
                                }
                                ApiError::RequestFailed { .. } => {
                                    "Kickertool hat die API-Anfrage abgelehnt. HTTP-Status und Antwort stehen im Debug-Log."
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
        store: MatchStore,
        runtime: Arc<Runtime>,
    ) {
        let tournament_tables_clone = Rc::clone(&tournament_tables);
        let tournaments_clone = Rc::clone(&tournaments);
        let selected_tournament_clone = Rc::clone(&selected_tournament);
        let table_combo_clone = kickertool.table_combo.clone();
        kickertool.tournament_combo.connect_changed(move |combo| {
            table_combo_clone.remove_all();

            if let Some(id) = combo.active_id() {
                let id_str = id.to_string();

                // A Kickertool match from another tournament leaves the table.
                // Refreshing the list (no tournament chosen) keeps it.
                let foreign = runtime.block_on(store.current()).is_some_and(|m| {
                    matches!(m.competition, Competition::Kickertool { .. })
                        && !is_kickertool_tournament(&m.competition, &id_str)
                });
                if foreign {
                    runtime.block_on(store.replace(None));
                }

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
