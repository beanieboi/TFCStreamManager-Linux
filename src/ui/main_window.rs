use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, ComboBoxText, Entry, Label, Orientation,
    Separator, Stack, ToggleButton,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

use super::{DebugLog, SettingsDialog};
use crate::models::{DEFAULT_SCORE_NAME, DEFAULT_SETS_NAME, OverlayContent, Table, Tournament};
use crate::services::{
    KickertoolApiService, LogCallback, OverlayMode, OverlayStateManager, ServiceDiscovery,
    Settings, SettingsService, TableMonitor, WebServer, log,
};

const REMOTE_HELP_TEXT: &str = "Remote mode active.\n\nSend POST requests to /scores endpoint with JSON:\n\n{\n  \"teamAScore\": 0,\n  \"teamBScore\": 0,\n  \"teamAName\": \"Team A\",\n  \"teamBName\": \"Team B\",\n  \"eventName\": \"Tournament\"\n}";

struct HeaderControls {
    start_button: Button,
    stop_button: Button,
    server_url_label: Label,
    settings_button: Button,
    debug_button: Button,
}

struct ModeButtons {
    kickertool: ToggleButton,
    remote: ToggleButton,
    manual: ToggleButton,
}

struct KickertoolControls {
    tournament_combo: ComboBoxText,
    table_combo: ComboBoxText,
    refresh_button: Button,
}

struct ManualControls {
    entries: Vec<Entry>,
    update_button: Button,
}

struct ServerContext {
    settings_service: Arc<SettingsService>,
    settings: Arc<RwLock<Settings>>,
    overlay_state: OverlayStateManager,
    web_server: Rc<RefCell<Option<WebServer>>>,
    service_discovery: Rc<RefCell<Option<Arc<ServiceDiscovery>>>>,
    log_callback: LogCallback,
    runtime: Arc<Runtime>,
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
        let tournament_tables: Rc<RefCell<std::collections::HashMap<String, Vec<Table>>>> =
            Rc::new(RefCell::new(std::collections::HashMap::new()));
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

    fn build_header() -> (GtkBox, HeaderControls) {
        let header_box = GtkBox::new(Orientation::Horizontal, 8);

        let start_button = Button::with_label("Start Server");
        let stop_button = Button::with_label("Stop Server");
        stop_button.set_sensitive(false);

        let server_url_label = Label::new(Some("Not running"));
        server_url_label.set_hexpand(true);
        server_url_label.set_xalign(0.0);

        let settings_button = Button::with_label("Settings");
        let debug_button = Button::with_label("Debug");

        header_box.append(&start_button);
        header_box.append(&stop_button);
        header_box.append(&server_url_label);
        header_box.append(&settings_button);
        header_box.append(&debug_button);

        (
            header_box,
            HeaderControls {
                start_button,
                stop_button,
                server_url_label,
                settings_button,
                debug_button,
            },
        )
    }

    fn build_mode_buttons() -> (GtkBox, ModeButtons) {
        let mode_box = GtkBox::new(Orientation::Horizontal, 8);
        mode_box.set_halign(gtk4::Align::Center);
        mode_box.set_margin_top(8);
        mode_box.set_margin_bottom(8);

        let kickertool = ToggleButton::with_label("Kickertool");
        let remote = ToggleButton::with_label("Remote");
        let manual = ToggleButton::with_label("Manual");

        kickertool.set_group(Some(&remote));
        manual.set_group(Some(&remote));

        mode_box.append(&kickertool);
        mode_box.append(&remote);
        mode_box.append(&manual);

        (
            mode_box,
            ModeButtons {
                kickertool,
                remote,
                manual,
            },
        )
    }

    fn build_content_stack() -> (Stack, KickertoolControls, ManualControls) {
        let content_stack = Stack::new();

        let empty_panel = GtkBox::new(Orientation::Vertical, 8);
        let empty_label = Label::new(Some("Select a mode to get started"));
        empty_label.set_vexpand(true);
        empty_label.set_valign(gtk4::Align::Center);
        empty_panel.append(&empty_label);
        content_stack.add_named(&empty_panel, Some("empty"));

        let (kickertool_panel, kickertool_controls) = Self::build_kickertool_panel();
        content_stack.add_named(&kickertool_panel, Some("kickertool"));

        let remote_panel = Self::build_remote_panel();
        content_stack.add_named(&remote_panel, Some("remote"));

        let (manual_panel, manual_controls) = Self::build_manual_panel();
        content_stack.add_named(&manual_panel, Some("manual"));

        content_stack.set_visible_child_name("empty");

        (content_stack, kickertool_controls, manual_controls)
    }

    fn build_kickertool_panel() -> (GtkBox, KickertoolControls) {
        let panel = GtkBox::new(Orientation::Vertical, 8);
        panel.set_margin_top(8);

        let (tournament_box, tournament_combo) = Self::build_labeled_combo("Tournament:");
        panel.append(&tournament_box);

        let (table_box, table_combo) = Self::build_labeled_combo("Table:");
        panel.append(&table_box);

        let refresh_button = Button::with_label("Refresh Tournaments");
        panel.append(&refresh_button);

        (
            panel,
            KickertoolControls {
                tournament_combo,
                table_combo,
                refresh_button,
            },
        )
    }

    fn build_remote_panel() -> GtkBox {
        let panel = GtkBox::new(Orientation::Vertical, 8);
        panel.set_margin_top(8);

        let remote_label = Label::new(Some(REMOTE_HELP_TEXT));
        remote_label.set_xalign(0.0);
        panel.append(&remote_label);

        panel
    }

    fn build_manual_panel() -> (GtkBox, ManualControls) {
        let panel = GtkBox::new(Orientation::Vertical, 8);
        panel.set_margin_top(8);

        let labels = [
            "Tournament:",
            "Discipline:",
            "Round:",
            "Group:",
            "Team A:",
            "Team B:",
            "Score A:",
            "Score B:",
            "Sets A:",
            "Sets B:",
        ];

        let mut entries = Vec::new();
        for label_text in labels {
            let entry = Entry::new();
            let row = Self::build_labeled_entry(label_text, &entry);
            panel.append(&row);
            entries.push(entry);
        }

        let update_button = Button::with_label("Update Overlay");
        panel.append(&update_button);

        (
            panel,
            ManualControls {
                entries,
                update_button,
            },
        )
    }

    fn build_labeled_combo(label_text: &str) -> (GtkBox, ComboBoxText) {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let label = Label::new(Some(label_text));
        label.set_width_chars(12);
        label.set_xalign(0.0);
        let combo = ComboBoxText::new();
        combo.set_hexpand(true);
        row.append(&label);
        row.append(&combo);

        (row, combo)
    }

    fn build_labeled_entry(label_text: &str, entry: &Entry) -> GtkBox {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let label = Label::new(Some(label_text));
        label.set_width_chars(12);
        label.set_xalign(0.0);
        entry.set_hexpand(true);
        row.append(&label);
        row.append(entry);

        row
    }

    fn connect_mode_buttons(
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
                // Automatically load tournaments when switching to Kickertool mode
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

    fn connect_manual_update(
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

    fn connect_settings_dialog(
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

    fn connect_debug_button(
        debug_button: &Button,
        parent: &ApplicationWindow,
        debug_log: Rc<DebugLog>,
    ) {
        let parent = parent.clone();
        debug_button.connect_clicked(move |_| {
            debug_log.show(&parent);
        });
    }

    fn connect_server_buttons(header: &HeaderControls, context: ServerContext) {
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

    fn connect_refresh_tournaments(
        kickertool: &KickertoolControls,
        api_service: Arc<KickertoolApiService>,
        tournaments: Rc<RefCell<Vec<Tournament>>>,
        tournament_tables: Rc<RefCell<std::collections::HashMap<String, Vec<Table>>>>,
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

    fn connect_tournament_selection(
        kickertool: &KickertoolControls,
        tournaments: Rc<RefCell<Vec<Tournament>>>,
        tournament_tables: Rc<RefCell<std::collections::HashMap<String, Vec<Table>>>>,
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

    fn connect_table_selection(
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

    fn read_manual_entries(entries: &[Entry]) -> ManualValues {
        ManualValues {
            tournament: entries[0].text().to_string(),
            discipline: entries[1].text().to_string(),
            round: entries[2].text().to_string(),
            group: entries[3].text().to_string(),
            team_a: entries[4].text().to_string(),
            team_b: entries[5].text().to_string(),
            score_a: entries[6].text().to_string(),
            score_b: entries[7].text().to_string(),
            sets_a: entries[8].text().to_string(),
            sets_b: entries[9].text().to_string(),
        }
    }

    fn spawn_set_mode(
        runtime: &Arc<Runtime>,
        overlay_state: OverlayStateManager,
        mode: OverlayMode,
    ) {
        let rt = Arc::clone(runtime);
        rt.spawn(async move {
            overlay_state.set_mode(mode).await;
        });
    }

    fn remote_default_content() -> OverlayContent {
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

    fn manual_content(values: ManualValues) -> OverlayContent {
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

    fn start_server(
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

    fn stop_server(context: Rc<ServerContext>) {
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

struct ManualValues {
    tournament: String,
    discipline: String,
    round: String,
    group: String,
    team_a: String,
    team_b: String,
    score_a: String,
    score_b: String,
    sets_a: String,
    sets_b: String,
}
