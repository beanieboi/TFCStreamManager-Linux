use gtk4::ApplicationWindow;
use gtk4::prelude::*;
use std::rc::Rc;
use std::sync::Arc;

use super::{MainWindow, ServerContext};
use crate::services::{ServerState, ServiceDiscovery, WebServer};

impl MainWindow {
    /// (Re)starts the web server on the configured port. The server runs as
    /// long as the app is open; there is no stop button. mDNS announces
    /// `running` when the port is bound, `started` when it is not (ADR 0009).
    pub(super) fn restart_server(context: &ServerContext) {
        Self::stop_server(context);

        let port = context
            .runtime
            .block_on(async { context.settings.read().await.port });
        let mut server = WebServer::new(
            port,
            context.overlay.clone(),
            Arc::clone(&context.settings_service),
            Arc::clone(&context.settings),
            context.match_services.clone(),
            Arc::clone(&context.runtime),
        );

        match server.start() {
            Ok(()) => {
                *context.web_server.borrow_mut() = Some(server);
                context.status_label.set_text("");
                context.overlay_button.set_sensitive(true);
                context
                    .overlay_button
                    .set_tooltip_text(Some(&Self::overlay_url(port)));
                context.retry_button.set_visible(false);
                Self::advertise(context, port, ServerState::Running);
            }
            Err(e) => {
                tracing::error!("Failed to start server: {e:#}");
                context
                    .status_label
                    .set_text(&format!("Server läuft nicht: {e:#}"));
                context.overlay_button.set_sensitive(false);
                context.overlay_button.set_tooltip_text(None);
                context.retry_button.set_visible(true);
                Self::advertise(context, port, ServerState::Started);
            }
        }
    }

    pub(super) fn overlay_url(port: u16) -> String {
        format!("http://localhost:{port}")
    }

    pub(super) fn stop_server(context: &ServerContext) {
        if let Some(mut server) = context.web_server.borrow_mut().take() {
            server.stop();
        }
    }

    /// Announces synchronously, so consecutive restarts cannot reorder their
    /// announcements.
    fn advertise(context: &ServerContext, port: u16, state: ServerState) {
        let Some(discovery) = context.service_discovery.borrow().clone() else {
            return;
        };
        if let Err(e) = context.runtime.block_on(discovery.advertise(port, state)) {
            tracing::error!("Failed to advertise mDNS: {}", e);
        }
    }

    /// Creates the mDNS daemon. What it announces is decided by
    /// [`Self::restart_server`].
    pub(super) fn start_discovery() -> Option<Arc<ServiceDiscovery>> {
        match ServiceDiscovery::new() {
            Ok(discovery) => Some(Arc::new(discovery)),
            Err(e) => {
                tracing::error!("Failed to start mDNS: {}", e);
                None
            }
        }
    }

    /// Stops the server and sends an mDNS goodbye when the window closes, so
    /// the tablet turns red right away instead of after its timeout.
    pub(super) fn connect_shutdown_on_close(
        window: &ApplicationWindow,
        context: Rc<ServerContext>,
    ) {
        window.connect_close_request(move |window| {
            Self::stop_server(&context);
            if let Some(discovery) = context.service_discovery.borrow_mut().take() {
                context.runtime.block_on(async {
                    let _ = discovery.stop_advertising().await;
                });
            }
            // Closing the main window ends the app, also with the debug
            // window (an application window too) still around.
            if let Some(app) = window.application() {
                app.quit();
            }
            glib::Propagation::Proceed
        });
    }
}
