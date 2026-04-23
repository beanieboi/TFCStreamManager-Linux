use gtk4::prelude::*;
use gtk4::{Button, Entry, Label};
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;

use super::{MainWindow, ManualValues, ServerContext};
use crate::models::{DEFAULT_SCORE_NAME, DEFAULT_SETS_NAME, OverlayContent, Table};
use crate::services::{OverlayMode, OverlayStateManager, ServiceDiscovery, WebServer, log};

impl MainWindow {
    pub(super) fn read_manual_entries(entries: &[Entry]) -> ManualValues {
        ManualValues {
            tournament: entries[0].text().to_string(),
            discipline: entries[1].text().to_string(),
            round: entries[2].text().to_string(),
            group: entries[3].text().to_string(),
            team_a: entries[4].text().to_string(),
            team_b: entries[5].text().to_string(),
            team_a_player: entries[6].text().to_string(),
            team_b_player: entries[7].text().to_string(),
            score_a: entries[8].text().to_string(),
            score_b: entries[9].text().to_string(),
            sets_a: entries[10].text().to_string(),
            sets_b: entries[11].text().to_string(),
        }
    }

    pub(super) fn spawn_set_mode(
        runtime: &Arc<Runtime>,
        overlay_state: OverlayStateManager,
        mode: OverlayMode,
    ) {
        let rt = Arc::clone(runtime);
        rt.spawn(async move {
            overlay_state.set_mode(mode).await;
        });
    }

    pub(super) fn remote_default_content() -> OverlayContent {
        OverlayContent {
            table: Table {
                name: "Table 1".to_owned(),
                ..Default::default()
            },
            team_a: "-".to_owned(),
            team_b: "-".to_owned(),
            score_a: "0".to_owned(),
            score_b: "0".to_owned(),
            score_name: DEFAULT_SCORE_NAME.to_owned(),
            sets_name: DEFAULT_SETS_NAME.to_owned(),
            sets_a: "0".to_owned(),
            sets_b: "0".to_owned(),
            state: "running".to_owned(),
            tournament_name: "Remote Mode".to_owned(),
            ..Default::default()
        }
    }

    pub(super) fn manual_content(values: ManualValues) -> OverlayContent {
        OverlayContent {
            table: Table {
                name: "Manual".to_owned(),
                ..Default::default()
            },
            tournament_name: values.tournament,
            discipline_name: values.discipline,
            round_name: values.round,
            group_name: values.group,
            team_a: values.team_a,
            team_b: values.team_b,
            team_a_player: values.team_a_player,
            team_b_player: values.team_b_player,
            score_a: values.score_a,
            score_b: values.score_b,
            score_name: DEFAULT_SCORE_NAME.to_owned(),
            sets_name: DEFAULT_SETS_NAME.to_owned(),
            sets_a: values.sets_a,
            sets_b: values.sets_b,
            state: "running".to_owned(),
            ..Default::default()
        }
    }

    pub(super) fn start_server(
        context: Rc<ServerContext>,
        start_button: Button,
        stop_button: Button,
        url_label: Label,
    ) {
        let current_settings = context
            .runtime
            .block_on(async { context.settings.read().await.clone() });
        let port = current_settings.port;

        let overlay_path = context.settings_service.get_overlay_path(&current_settings);
        if !overlay_path.exists() {
            log(
                &context.log_callback,
                "WebServer",
                format!("Overlay template not found: {:?}", overlay_path),
            );
            return;
        }

        let mut server = WebServer::new(
            port,
            context.overlay_state.clone(),
            Arc::clone(&context.settings_service),
            Arc::clone(&context.settings),
            Arc::clone(&context.log_callback),
            Arc::clone(&context.runtime),
        );

        if let Err(e) = server.start() {
            log(
                &context.log_callback,
                "WebServer",
                format!("Failed to start server: {}", e),
            );
            return;
        }

        *context.web_server.borrow_mut() = Some(server);
        let discovery = match ServiceDiscovery::new(Arc::clone(&context.log_callback)) {
            Ok(discovery) => Arc::new(discovery),
            Err(e) => {
                log(
                    &context.log_callback,
                    "ServiceDiscovery",
                    format!("Failed to start mDNS: {}", e),
                );
                return;
            }
        };
        *context.service_discovery.borrow_mut() = Some(Arc::clone(&discovery));

        let log_callback = Arc::clone(&context.log_callback);
        context.runtime.spawn(async move {
            if let Err(e) = discovery.start_advertising(port).await {
                log(
                    &log_callback,
                    "ServiceDiscovery",
                    format!("Failed to start mDNS: {}", e),
                );
            }
        });

        start_button.set_sensitive(false);
        stop_button.set_sensitive(true);
        url_label.set_markup(&format!(
            "<a href=\"http://localhost:{}\">http://localhost:{}</a>",
            port, port
        ));
    }

    pub(super) fn stop_server(context: Rc<ServerContext>) {
        if let Some(ref mut server) = *context.web_server.borrow_mut() {
            server.stop();
        }
        *context.web_server.borrow_mut() = None;

        if let Some(discovery) = context.service_discovery.borrow_mut().take() {
            context.runtime.spawn(async move {
                let _ = discovery.stop_advertising().await;
            });
        }
    }
}
