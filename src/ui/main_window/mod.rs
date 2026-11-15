mod actions;
mod competition;
mod overlay_picker;
mod panels;
mod score_panel;
mod wiring;

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, Orientation, Separator};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

use super::DebugLog;
use crate::models::{Table, Tournament};
use crate::services::{
    KickertoolApiService, MatchServices, OverlayFeed, OverlayLibrary, ServiceDiscovery, Settings,
    SettingsService, TableMonitor, WebServer,
};

pub(super) struct HeaderControls {
    status_label: gtk4::Label,
    overlay_combo: gtk4::ComboBoxText,
    overlay_button: gtk4::Button,
    retry_button: gtk4::Button,
    settings_button: gtk4::Button,
    debug_button: gtk4::Button,
}

pub(super) struct CompetitionButtons {
    kickertool: gtk4::ToggleButton,
    mtfv: gtk4::ToggleButton,
    manual: gtk4::ToggleButton,
}

pub(super) struct KickertoolControls {
    tournament_combo: gtk4::ComboBoxText,
    table_combo: gtk4::ComboBoxText,
    refresh_button: gtk4::Button,
}

pub(super) struct MtfvControls {
    league_combo: gtk4::ComboBoxText,
    reload_button: gtk4::Button,
    games_label: gtk4::Label,
    /// Race leagues: the race settings, and the league's games (read only).
    race: RaceControls,
    fixture_combo: gtk4::ComboBoxText,
    load_button: gtk4::Button,
}

/// Race settings for the day, shown in the MTFV and the Frei panel: target,
/// goals per segment, and one button per segment for its game (D or E).
#[derive(Clone)]
pub(super) struct RaceControls {
    settings_row: gtk4::Box,
    target: gtk4::SpinButton,
    segment: gtk4::SpinButton,
    segments_label: gtk4::Label,
    games_label: gtk4::Label,
    games: gtk4::Box,
}

#[derive(Clone)]
pub(super) struct FreiControls {
    singles: gtk4::ToggleButton,
    doubles: gtk4::ToggleButton,
    race: gtk4::ToggleButton,
    tournament: gtk4::Entry,
    discipline: gtk4::Entry,
    round: gtk4::Entry,
    group: gtk4::Entry,
    /// Race only: the teams, and the race settings with the games.
    team_label: gtk4::Label,
    team_a: gtk4::Entry,
    team_b: gtk4::Entry,
    race_settings: RaceControls,
    apply_button: gtk4::Button,
    new_button: gtk4::Button,
}

/// A value per side with +1 and -1: goals, or sets.
pub(super) struct CounterRows {
    /// Everything in these rows, to enable or disable them together.
    widgets: Vec<gtk4::Widget>,
    home_value: gtk4::Label,
    away_value: gtk4::Label,
    home_add: gtk4::Button,
    away_add: gtk4::Button,
    home_sub: gtk4::Button,
    away_sub: gtk4::Button,
}

impl CounterRows {
    fn add_button(&self, side: crate::competition::Side) -> &gtk4::Button {
        match side {
            crate::competition::Side::Home => &self.home_add,
            crate::competition::Side::Away => &self.away_add,
        }
    }

    fn sub_button(&self, side: crate::competition::Side) -> &gtk4::Button {
        match side {
            crate::competition::Side::Home => &self.home_sub,
            crate::competition::Side::Away => &self.away_sub,
        }
    }
}

/// The live score of the match on the table, for every competition.
pub(super) struct ScorePanel {
    title: gtk4::Label,
    home_team: gtk4::Label,
    away_team: gtk4::Label,
    home_players: gtk4::Entry,
    away_players: gtk4::Entry,
    players_button: gtk4::Button,
    /// MTFV set leagues: on to the next game once the current one is over.
    next_game_button: gtk4::Button,
    goals: CounterRows,
    /// Entered by hand; disabled in a race and where sets are derived (MTFV).
    sets: CounterRows,
    error: gtk4::Label,
    /// The game last shown, to tell a new game from an update.
    shown_game: RefCell<String>,
    /// The players last filled in, to tell typed names from shown ones.
    shown_players: RefCell<[String; 2]>,
}

/// The overlay template dropdown (ADR 0010). It shows the selection for the
/// competition of the match on the table; choosing by hand remembers the
/// template for that competition.
pub(super) struct OverlayPicker {
    combo: gtk4::ComboBoxText,
    library: OverlayLibrary,
    /// Set while the list is filled, so that is not taken as a choice.
    updating: Cell<bool>,
    /// The format of the selected template, which a Kickertool or Frei match
    /// follows.
    format: Cell<Option<crate::services::TemplateFormat>>,
}

pub(super) struct ServerContext {
    settings_service: Arc<SettingsService>,
    settings: Arc<RwLock<Settings>>,
    overlay: OverlayFeed,
    match_services: MatchServices,
    web_server: Rc<RefCell<Option<WebServer>>>,
    service_discovery: Rc<RefCell<Option<Arc<ServiceDiscovery>>>>,
    runtime: Arc<Runtime>,
    status_label: gtk4::Label,
    overlay_button: gtk4::Button,
    retry_button: gtk4::Button,
    overlay_picker: OverlayPicker,
    /// Its fields follow the template selected for Frei.
    frei: FreiControls,
    /// The MTFV panel's race settings, kept in step with Frei's.
    mtfv_race: RaceControls,
    score: ScorePanel,
    table_monitor: TableMonitor,
}

pub struct MainWindow {
    window: ApplicationWindow,
}

impl MainWindow {
    pub fn new(app: &Application) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("TFC StreamManager")
            .default_width(820)
            .default_height(640)
            .build();

        Self { window }
    }

    pub fn setup(
        self,
        settings_service: Arc<SettingsService>,
        api_service: Arc<KickertoolApiService>,
        overlay: OverlayFeed,
        match_services: MatchServices,
        settings: Arc<RwLock<Settings>>,
        runtime: Arc<Runtime>,
    ) {
        let window = self.window.clone();

        let debug_log = Rc::new(DebugLog::new(crate::services::logging::log_path()));

        let table_monitor = TableMonitor::new(
            Arc::clone(&api_service),
            match_services.store.clone(),
            Arc::clone(&settings),
            settings_service.translations_path(),
        );

        let tournaments: Rc<RefCell<Vec<Tournament>>> = Rc::new(RefCell::new(Vec::new()));
        let tournament_tables: Rc<RefCell<HashMap<String, Vec<Table>>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let selected_tournament: Rc<RefCell<Option<Tournament>>> = Rc::new(RefCell::new(None));

        let main_box = GtkBox::new(Orientation::Vertical, 8);
        main_box.set_margin_top(12);
        main_box.set_margin_bottom(12);
        main_box.set_margin_start(12);
        main_box.set_margin_end(12);

        let (competition_box, competition_buttons) = Self::build_competition_buttons();
        let (header_box, header) = Self::build_header(&competition_box);
        let (content_stack, kickertool, mtfv, frei) = Self::build_content_stack();
        let (score_box, score) = Self::build_score_panel();

        main_box.append(&header_box);
        main_box.append(&Separator::new(Orientation::Horizontal));
        main_box.append(&content_stack);
        main_box.append(&Separator::new(Orientation::Horizontal));
        main_box.append(&score_box);
        window.set_child(Some(&main_box));

        let context = Rc::new(ServerContext {
            settings_service: Arc::clone(&settings_service),
            settings: Arc::clone(&settings),
            overlay,
            match_services,
            web_server: Rc::new(RefCell::new(None)),
            service_discovery: Rc::new(RefCell::new(Self::start_discovery())),
            runtime: Arc::clone(&runtime),
            status_label: header.status_label.clone(),
            overlay_button: header.overlay_button.clone(),
            retry_button: header.retry_button.clone(),
            overlay_picker: OverlayPicker {
                combo: header.overlay_combo.clone(),
                library: settings_service.overlays(),
                updating: Cell::new(false),
                format: Cell::new(None),
            },
            frei,
            mtfv_race: mtfv.race.clone(),
            score,
            table_monitor: table_monitor.clone(),
        });

        Self::restart_server(&context);
        Self::connect_retry_button(&header.retry_button, Rc::clone(&context));
        Self::connect_overlay_button(&header.overlay_button, &window, Rc::clone(&context));
        Self::connect_shutdown_on_close(&window, Rc::clone(&context));
        Self::connect_settings_dialog(
            &header.settings_button,
            &window,
            Arc::clone(&api_service),
            Rc::clone(&context),
        );
        Self::connect_debug_button(&header.debug_button, &window, Rc::clone(&debug_log));
        Self::connect_overlay_picker(Rc::clone(&context));
        Self::connect_score_panel(Rc::clone(&context));
        Self::connect_frei_panel(Rc::clone(&context));
        Self::connect_mtfv_panel(&mtfv, Rc::clone(&context));
        Self::connect_refresh_tournaments(
            &kickertool,
            &window,
            Arc::clone(&api_service),
            Rc::clone(&tournaments),
            Rc::clone(&tournament_tables),
            Arc::clone(&runtime),
        );
        Self::connect_tournament_selection(
            &kickertool,
            Rc::clone(&tournaments),
            Rc::clone(&tournament_tables),
            Rc::clone(&selected_tournament),
            context.match_services.store.clone(),
            Arc::clone(&runtime),
        );
        Self::connect_table_selection(
            &kickertool,
            Rc::clone(&selected_tournament),
            table_monitor,
            Arc::clone(&runtime),
        );
        Self::connect_competition_buttons(
            &competition_buttons,
            &content_stack,
            &kickertool,
            &mtfv,
            Rc::clone(&context),
        );
        Self::follow_match_on_the_table(Rc::clone(&context));
        Self::show_overlays(&context);
        Self::restore_competition(&competition_buttons, &context);

        window.present();
    }
}
