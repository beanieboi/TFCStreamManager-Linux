use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Button, Label};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

use super::{MainWindow, ManualForm, ManualValues, ServerContext};
use crate::models::{DEFAULT_SCORE_NAME, DEFAULT_SETS_NAME, OverlayContent, Table};
use crate::services::{
    LogCallback, OverlayMode, OverlayStateManager, ServerState, ServiceDiscovery, Settings,
    WebServer, log,
};

impl MainWindow {
    pub(super) fn read_manual_entries(form: &ManualForm) -> ManualValues {
        ManualValues {
            tournament: form.tournament.text().to_string(),
            discipline: form.discipline.text().to_string(),
            round: form.round.text().to_string(),
            group: form.group.text().to_string(),
            team_a: form.team_a.text().to_string(),
            team_b: form.team_b.text().to_string(),
            team_a_player: form.team_a_player.text().to_string(),
            team_b_player: form.team_b_player.text().to_string(),
            score_a: form.score_a.text().to_string(),
            score_b: form.score_b.text().to_string(),
            sets_a: form.sets_a.text().to_string(),
            sets_b: form.sets_b.text().to_string(),
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
        Self::spawn_advertise(&context, port, ServerState::Running);

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

        let port = context
            .runtime
            .block_on(async { context.settings.read().await.port });
        Self::spawn_advertise(&context, port, ServerState::Started);
    }

    fn spawn_advertise(context: &ServerContext, port: u16, state: ServerState) {
        let Some(discovery) = context.service_discovery.borrow().clone() else {
            return;
        };
        let log_callback = Arc::clone(&context.log_callback);
        context.runtime.spawn(async move {
            if let Err(e) = discovery.advertise(port, state).await {
                log(
                    &log_callback,
                    "ServiceDiscovery",
                    format!("Failed to advertise mDNS: {}", e),
                );
            }
        });
    }

    /// Registers mDNS as `started` as soon as the app is open, so the tablet
    /// can tell "app open, server stopped" from "app not running".
    pub(super) fn start_discovery(
        settings: &Arc<RwLock<Settings>>,
        log_callback: &LogCallback,
        runtime: &Arc<Runtime>,
    ) -> Option<Arc<ServiceDiscovery>> {
        let discovery = match ServiceDiscovery::new(Arc::clone(log_callback)) {
            Ok(discovery) => Arc::new(discovery),
            Err(e) => {
                log(
                    log_callback,
                    "ServiceDiscovery",
                    format!("Failed to start mDNS: {}", e),
                );
                return None;
            }
        };
        let port = runtime.block_on(async { settings.read().await.port });
        let for_task = Arc::clone(&discovery);
        let log_callback = Arc::clone(log_callback);
        runtime.spawn(async move {
            if let Err(e) = for_task.advertise(port, ServerState::Started).await {
                log(
                    &log_callback,
                    "ServiceDiscovery",
                    format!("Failed to advertise mDNS: {}", e),
                );
            }
        });
        Some(discovery)
    }

    /// Sends an mDNS goodbye when the window closes, so the tablet turns red
    /// right away instead of after its timeout.
    pub(super) fn connect_unregister_on_close(
        window: &ApplicationWindow,
        discovery: &Rc<RefCell<Option<Arc<ServiceDiscovery>>>>,
        runtime: &Arc<Runtime>,
    ) {
        let discovery = Rc::clone(discovery);
        let runtime = Arc::clone(runtime);
        window.connect_close_request(move |_| {
            if let Some(discovery) = discovery.borrow_mut().take() {
                runtime.block_on(async {
                    let _ = discovery.stop_advertising().await;
                });
            }
            glib::Propagation::Proceed
        });
    }
}
