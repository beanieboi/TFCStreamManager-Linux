mod competition;
mod models;
mod mtfv;
mod services;
mod ui;

use gtk4::Application;
use gtk4::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;

use mtfv::MtfvClient;
use services::{KickertoolApiService, MatchServices, MatchStore, OverlayFeed, SettingsService};
use ui::MainWindow;

const APP_ID: &str = "com.benfritsch.TFCStreamManager";

fn main() -> glib::ExitCode {
    // Initialize logging
    // Logs go to a file the debug window tails, and to the terminal.
    if let Err(e) = services::logging::init() {
        eprintln!("Logging to a file is not possible: {e:#}");
    }

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

        let mut loaded = settings_service.load();
        // Templates only come from the overlay folder (ADR 0010).
        settings_service.use_overlay_dir(&loaded);
        if let Some(message) = settings_service.migrate_overlay_path(&mut loaded) {
            tracing::info!("{message}");
        }
        let settings = Arc::new(RwLock::new(loaded));

        let api_service = Arc::new(
            KickertoolApiService::new(Arc::clone(&settings_service))
                .expect("Failed to initialize API service"),
        );

        let store = MatchStore::open(settings_service.table_match_path());
        let overlay = OverlayFeed::new(store.clone(), settings_service.translations_path());
        let match_services = MatchServices {
            settings: Arc::clone(&settings),
            store,
            mtfv: Arc::new(MtfvClient::new().expect("Failed to initialize mtfv.de client")),
        };

        // Create and setup main window
        let main_window = MainWindow::new(app);
        main_window.setup(
            settings_service,
            api_service,
            overlay,
            match_services,
            settings,
            Arc::clone(&rt_clone),
        );
    });

    app.run()
}
