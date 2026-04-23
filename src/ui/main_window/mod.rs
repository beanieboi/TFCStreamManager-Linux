mod actions;
mod panels;
mod wiring;

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, Orientation, Separator};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

use super::DebugLog;
use crate::models::{Table, Tournament};
use crate::services::{
    KickertoolApiService, LogCallback, OverlayStateManager, ServiceDiscovery, Settings,
    SettingsService, TableMonitor, WebServer,
};

pub(super) struct HeaderControls {
    start_button: gtk4::Button,
    stop_button: gtk4::Button,
    server_url_label: gtk4::Label,
    settings_button: gtk4::Button,
    debug_button: gtk4::Button,
}

pub(super) struct ModeButtons {
    kickertool: gtk4::ToggleButton,
    remote: gtk4::ToggleButton,
    manual: gtk4::ToggleButton,
}

pub(super) struct KickertoolControls {
    tournament_combo: gtk4::ComboBoxText,
    table_combo: gtk4::ComboBoxText,
    refresh_button: gtk4::Button,
}

pub(super) struct ManualControls {
    entries: Vec<gtk4::Entry>,
    update_button: gtk4::Button,
}

pub(super) struct ServerContext {
    settings_service: Arc<SettingsService>,
    settings: Arc<RwLock<Settings>>,
    overlay_state: OverlayStateManager,
    web_server: Rc<RefCell<Option<WebServer>>>,
    service_discovery: Rc<RefCell<Option<Arc<ServiceDiscovery>>>>,
    log_callback: LogCallback,
    runtime: Arc<Runtime>,
}

pub(super) struct ManualValues {
    tournament: String,
    discipline: String,
    round: String,
    group: String,
    team_a: String,
    team_b: String,
    team_a_player: String,
    team_b_player: String,
    score_a: String,
    score_b: String,
    sets_a: String,
    sets_b: String,
}

pub struct MainWindow {
    window: ApplicationWindow,
}

impl MainWindow {
    pub fn new(app: &Application) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("TFC StreamManager")
            .default_width(600)
            .default_height(500)
            .build();

        Self { window }
    }

    pub fn setup(
        self,
        settings_service: Arc<SettingsService>,
        api_service: Arc<KickertoolApiService>,
        overlay_state: OverlayStateManager,
        settings: Arc<RwLock<Settings>>,
        runtime: Arc<Runtime>,
    ) {
        let window = self.window.clone();

        let debug_log = Rc::new(DebugLog::new());
        debug_log.install();
        let log_callback = debug_log.callback();

        let table_monitor = TableMonitor::new(
            Arc::clone(&api_service),
            overlay_state.clone(),
            Arc::clone(&settings),
            Arc::clone(&log_callback),
        );

        let tournaments: Rc<RefCell<Vec<Tournament>>> = Rc::new(RefCell::new(Vec::new()));
        let tournament_tables: Rc<RefCell<HashMap<String, Vec<Table>>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let selected_tournament: Rc<RefCell<Option<Tournament>>> = Rc::new(RefCell::new(None));
        let web_server: Rc<RefCell<Option<WebServer>>> = Rc::new(RefCell::new(None));
        let service_discovery: Rc<RefCell<Option<Arc<ServiceDiscovery>>>> =
            Rc::new(RefCell::new(None));

        let main_box = GtkBox::new(Orientation::Vertical, 8);
        main_box.set_margin_top(12);
        main_box.set_margin_bottom(12);
        main_box.set_margin_start(12);
        main_box.set_margin_end(12);

        let (header_box, header) = Self::build_header();
        let (mode_box, mode_buttons) = Self::build_mode_buttons();
        let (content_stack, kickertool, manual_controls) = Self::build_content_stack();

        main_box.append(&header_box);
        main_box.append(&Separator::new(Orientation::Horizontal));
        main_box.append(&mode_box);
        main_box.append(&content_stack);
        window.set_child(Some(&main_box));

        Self::connect_mode_buttons(
            &mode_buttons,
            &content_stack,
            &kickertool.refresh_button,
            overlay_state.clone(),
            Arc::clone(&runtime),
        );
        Self::connect_manual_update(
            &manual_controls,
            overlay_state.clone(),
            Arc::clone(&runtime),
        );
        Self::connect_settings_dialog(
            &header.settings_button,
            &window,
            Arc::clone(&settings_service),
            Arc::clone(&settings),
            Arc::clone(&api_service),
            Arc::clone(&log_callback),
            Arc::clone(&runtime),
        );
        Self::connect_debug_button(&header.debug_button, &window, Rc::clone(&debug_log));

        let server_context = ServerContext {
            settings_service: Arc::clone(&settings_service),
            settings: Arc::clone(&settings),
            overlay_state: overlay_state.clone(),
            web_server: Rc::clone(&web_server),
            service_discovery: Rc::clone(&service_discovery),
            log_callback: Arc::clone(&log_callback),
            runtime: Arc::clone(&runtime),
        };
        Self::connect_server_buttons(&header, server_context);
        Self::connect_refresh_tournaments(
            &kickertool,
            Arc::clone(&api_service),
            Rc::clone(&tournaments),
            Rc::clone(&tournament_tables),
            Arc::clone(&log_callback),
            Arc::clone(&runtime),
        );
        Self::connect_tournament_selection(
            &kickertool,
            Rc::clone(&tournaments),
            Rc::clone(&tournament_tables),
            Rc::clone(&selected_tournament),
        );
        Self::connect_table_selection(
            &kickertool,
            Rc::clone(&selected_tournament),
            table_monitor,
            Arc::clone(&runtime),
        );

        window.present();
    }
}
