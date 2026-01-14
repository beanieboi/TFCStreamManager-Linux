mod models;
mod services;
mod ui;

use gtk4::Application;
use gtk4::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;

use services::{KickertoolApiService, OverlayStateManager, SettingsService};
use ui::MainWindow;

const APP_ID: &str = "com.benfritsch.TFCStreamManager";

fn main() -> glib::ExitCode {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Create tokio runtime BEFORE GTK app - it needs to live for the entire program
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    // Wrap runtime in Arc so it can be shared
    let rt = Arc::new(rt);

    // Create the GTK application
    let app = Application::builder().application_id(APP_ID).build();

    let rt_clone = Arc::clone(&rt);
    app.connect_activate(move |app| {
        // Enter the runtime context
        let _guard = rt_clone.enter();

        // Initialize services
        let settings_service =
            Arc::new(SettingsService::new().expect("Failed to initialize settings service"));

        let settings = Arc::new(RwLock::new(settings_service.load()));

        let api_service = Arc::new(
            KickertoolApiService::new(Arc::clone(&settings_service))
                .expect("Failed to initialize API service"),
        );

        let overlay_state = OverlayStateManager::new();

        // Create and setup main window
        let main_window = MainWindow::new(app);
        main_window.setup(
            settings_service,
            api_service,
            overlay_state,
            settings,
            Arc::clone(&rt_clone),
        );
    });

    app.run()
}
