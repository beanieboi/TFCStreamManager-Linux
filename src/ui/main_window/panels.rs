use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, ComboBoxText, Entry, Grid, Label, Orientation, SpinButton, Stack,
    ToggleButton,
};

use super::{
    CompetitionButtons, CounterRows, FreiControls, HeaderControls, KickertoolControls, MainWindow,
    MtfvControls, RaceControls, ScorePanel,
};
use crate::competition::Side;

impl MainWindow {
    /// The only toolbar: competition and overlay template on the left,
    /// server and app actions on the right. The status label in between
    /// only shows server errors.
    pub(super) fn build_header(competition_box: &GtkBox) -> (GtkBox, HeaderControls) {
        let header_box = GtkBox::new(Orientation::Horizontal, 8);

        let overlay_combo = ComboBoxText::new();
        overlay_combo.set_tooltip_text(Some("Overlay-Vorlage"));

        let status_label = Label::new(None);
        status_label.set_hexpand(true);
        status_label.set_xalign(1.0);
        status_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        let overlay_button = Button::with_label("Overlay anzeigen");
        overlay_button.set_sensitive(false);

        let retry_button = Button::with_label("Erneut versuchen");
        retry_button.set_visible(false);

        let settings_button = Button::with_label("Einstellungen");
        let debug_button = Button::with_label("Debug");

        header_box.append(competition_box);
        header_box.append(&overlay_combo);
        header_box.append(&status_label);
        header_box.append(&retry_button);
        header_box.append(&overlay_button);
        header_box.append(&settings_button);
        header_box.append(&debug_button);

        (
            header_box,
            HeaderControls {
                status_label,
                overlay_combo,
                overlay_button,
                retry_button,
                settings_button,
                debug_button,
            },
        )
    }

    /// What is played (ADR 0012).
    pub(super) fn build_competition_buttons() -> (GtkBox, CompetitionButtons) {
        let competition_box = GtkBox::new(Orientation::Horizontal, 0);
        competition_box.add_css_class("linked");

        let kickertool = ToggleButton::with_label("Kickertool");
        let mtfv = ToggleButton::with_label("MTFV");
        let manual = ToggleButton::with_label("Frei");
        mtfv.set_group(Some(&kickertool));
        manual.set_group(Some(&kickertool));

        competition_box.append(&kickertool);
        competition_box.append(&mtfv);
        competition_box.append(&manual);

        (
            competition_box,
            CompetitionButtons {
                kickertool,
                mtfv,
                manual,
            },
        )
    }

    pub(super) fn build_content_stack() -> (Stack, KickertoolControls, MtfvControls, FreiControls) {
        let content_stack = Stack::new();

        let empty_label = Label::new(Some("Wettbewerb wählen, um loszulegen"));
        empty_label.set_valign(gtk4::Align::Center);
        content_stack.add_named(&empty_label, Some("empty"));

        let (kickertool_panel, kickertool) = Self::build_kickertool_panel();
        content_stack.add_named(&kickertool_panel, Some("kickertool"));

        let (mtfv_panel, mtfv) = Self::build_mtfv_panel();
        content_stack.add_named(&mtfv_panel, Some("mtfv"));

        let (frei_panel, frei) = Self::build_frei_panel();
        content_stack.add_named(&frei_panel, Some("manual"));

        content_stack.set_visible_child_name("empty");
        (content_stack, kickertool, mtfv, frei)
    }

    fn build_kickertool_panel() -> (GtkBox, KickertoolControls) {
        let panel = GtkBox::new(Orientation::Vertical, 8);
        panel.set_margin_top(8);

        let (tournament_box, tournament_combo) = Self::build_labeled_combo("Turnier:");
        panel.append(&tournament_box);

        let (table_box, table_combo) = Self::build_labeled_combo("Tisch:");
        panel.append(&table_box);

        let refresh_button = Button::with_label("Turniere aktualisieren");
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

    /// Begegnungen of a day from the mtfv.de season overview.
    /// A grid, so labels, dropdowns and buttons line up: the buttons share
    /// a column (same width), the format info sits under the dropdowns.
    fn build_mtfv_panel() -> (Grid, MtfvControls) {
        let grid = Grid::builder()
            .row_spacing(8)
            .column_spacing(8)
            .margin_top(8)
            .build();
        let row_label = |text: &str, row: i32| {
            let label = Label::new(Some(text));
            label.set_width_chars(12);
            label.set_xalign(0.0);
            grid.attach(&label, 0, row, 1, 1);
        };

        row_label("Liga:", 0);
        let league_combo = ComboBoxText::new();
        league_combo.set_hexpand(true);
        grid.attach(&league_combo, 1, 0, 1, 1);
        let reload_button = Button::with_label("Aktualisieren");
        reload_button.set_tooltip_text(Some("Saisonübersicht von mtfv.de neu laden"));
        grid.attach(&reload_button, 2, 0, 1, 1);

        // Under the league: its games (set league), or the race settings
        // for the day (race league). One of the two is shown.
        let games_label = Label::new(None);
        games_label.set_xalign(0.0);
        games_label.add_css_class("dim-label");
        grid.attach(&games_label, 1, 1, 1, 1);

        let race = race_controls();
        grid.attach(&race.settings_row, 1, 1, 1, 1);
        grid.attach(&race.games_label, 0, 2, 1, 1);
        grid.attach(&race.games, 1, 2, 1, 1);

        row_label("Begegnung:", 3);
        let fixture_combo = ComboBoxText::new();
        fixture_combo.set_hexpand(true);
        grid.attach(&fixture_combo, 1, 3, 1, 1);
        let load_button = Button::with_label("Auf den Tisch");
        load_button.set_sensitive(false);
        grid.attach(&load_button, 2, 3, 1, 1);

        (
            grid,
            MtfvControls {
                league_combo,
                reload_button,
                games_label,
                race,
                fixture_combo,
                load_button,
            },
        )
    }

    /// Everything entered (ADR 0012). The kind at the top decides the
    /// fields: team names for a race; the players of Einzel and Doppel are
    /// entered below, with the score.
    fn build_frei_panel() -> (Grid, FreiControls) {
        let grid = Grid::builder()
            .row_spacing(8)
            .column_spacing(8)
            .margin_top(8)
            .build();
        let row_label = |text: &str, row: i32| {
            let label = Label::new(Some(text));
            label.set_width_chars(12);
            label.set_xalign(0.0);
            grid.attach(&label, 0, row, 1, 1);
            label
        };

        row_label("Art:", 0);
        let kinds = GtkBox::new(Orientation::Horizontal, 0);
        kinds.add_css_class("linked");
        let singles = ToggleButton::with_label("Einzel");
        let doubles = ToggleButton::with_label("Doppel");
        let race = ToggleButton::with_label("Race");
        doubles.set_group(Some(&singles));
        race.set_group(Some(&singles));
        kinds.append(&singles);
        kinds.append(&doubles);
        kinds.append(&race);
        grid.attach(&kinds, 1, 0, 2, 1);

        let wide_entry = |text: &str, row: i32| {
            row_label(text, row);
            let entry = Entry::new();
            entry.set_hexpand(true);
            grid.attach(&entry, 1, row, 2, 1);
            entry
        };
        let tournament = wide_entry("Turnier:", 1);
        let discipline = wide_entry("Disziplin:", 2);
        let round = wide_entry("Runde:", 3);
        let group = wide_entry("Gruppe:", 4);

        // Race only: the teams; their names head the score below, as in
        // MTFV. Players of Einzel and Doppel are entered below, with the
        // score.
        let team_label = row_label("Teams:", 5);
        let [team_a, team_b] = [Entry::new(), Entry::new()];
        for (column, entry, placeholder) in [(1, &team_a, "Team Heim"), (2, &team_b, "Team Gast")] {
            entry.set_hexpand(true);
            entry.set_placeholder_text(Some(placeholder));
            grid.attach(entry, column, 5, 1, 1);
        }

        let race_settings = race_controls();
        grid.attach(&race_settings.settings_row, 1, 6, 2, 1);
        grid.attach(&race_settings.games_label, 0, 7, 1, 1);
        grid.attach(&race_settings.games, 1, 7, 2, 1);

        let buttons = GtkBox::new(Orientation::Horizontal, 8);
        buttons.set_halign(gtk4::Align::End);
        buttons.set_margin_top(4);
        let apply_button = Button::with_label("Auf den Tisch");
        apply_button.set_tooltip_text(Some(
            "Diese Angaben auf den Tisch. Läuft dort ein Frei-Spiel derselben Art, \
             bleibt der Stand; sonst beginnt ein neues Spiel.",
        ));
        let new_button = Button::with_label("Neustart");
        new_button.set_tooltip_text(Some("Ein neues Spiel auf den Tisch, Stand 0:0"));
        buttons.append(&apply_button);
        buttons.append(&new_button);
        grid.attach(&buttons, 1, 8, 2, 1);

        (
            grid,
            FreiControls {
                singles,
                doubles,
                race,
                tournament,
                discipline,
                round,
                group,
                team_label,
                team_a,
                team_b,
                race_settings,
                apply_button,
                new_button,
            },
        )
    }

    /// The live score of the match on the table, for every competition.
    pub(super) fn build_score_panel() -> (GtkBox, ScorePanel) {
        let panel = GtkBox::new(Orientation::Vertical, 8);

        let title = Label::new(None);
        title.set_xalign(0.0);
        title.set_wrap(true);
        panel.append(&title);

        let grid = Grid::builder().row_spacing(8).column_spacing(8).build();
        let row_label = |text: &str, row: i32| {
            let label = Label::new(Some(text));
            label.set_width_chars(12);
            label.set_xalign(0.0);
            grid.attach(&label, 0, row, 1, 1);
        };

        let home_team = Label::new(None);
        let away_team = Label::new(None);
        for (column, label) in [(1, &home_team), (2, &away_team)] {
            label.set_hexpand(true);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            grid.attach(label, column, 0, 1, 1);
        }

        row_label("Spieler:", 1);
        let home_players = Entry::builder().placeholder_text("Name / Name").build();
        let away_players = Entry::builder().placeholder_text("Name / Name").build();
        grid.attach(&home_players, 1, 1, 1, 1);
        grid.attach(&away_players, 2, 1, 1, 1);
        let players_button = Button::with_label("Auf den Tisch");
        grid.attach(&players_button, 3, 1, 1, 1);

        let goals = counter_rows(&grid, 2, "Tore:");
        let sets = counter_rows(&grid, 3, "Sätze:");
        sets.add_button(Side::Home)
            .set_tooltip_text(Some("Satz für Heim, der nächste beginnt bei 0:0"));
        sets.add_button(Side::Away)
            .set_tooltip_text(Some("Satz für Gast, der nächste beginnt bei 0:0"));
        // MTFV set leagues: after the game's sets, the operator moves on.
        let next_game_button = Button::with_label("Nächstes Spiel");
        next_game_button.set_visible(false);
        grid.attach(&next_game_button, 3, 3, 1, 1);
        panel.append(&grid);

        let error = Label::new(None);
        error.set_xalign(0.0);
        error.add_css_class("error");
        panel.append(&error);

        (
            panel,
            ScorePanel {
                title,
                home_team,
                away_team,
                home_players,
                away_players,
                players_button,
                next_game_button,
                goals,
                sets,
                error,
                shown_game: Default::default(),
                shown_players: Default::default(),
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
}

/// One grid row: the label, then per team `[−1] value [  +1  ]`, so label,
/// value and buttons line up with the rows above. Used for goals and sets.
fn counter_rows(grid: &Grid, row: i32, title: &str) -> CounterRows {
    let label = Label::new(Some(title));
    label.set_width_chars(12);
    label.set_xalign(0.0);
    grid.attach(&label, 0, row, 1, 1);

    let mut widgets: Vec<gtk4::Widget> = vec![label.upcast()];
    let mut side = |column| {
        let cell = GtkBox::new(Orientation::Horizontal, 8);
        let sub = Button::with_label("−1");
        sub.set_valign(gtk4::Align::Center);
        sub.set_tooltip_text(Some("Einen zurücknehmen"));
        // Room for three digits; "+1" takes the rest of the width.
        let value = Label::new(Some("–"));
        value.add_css_class("title-2");
        value.set_width_chars(3);
        let add = Button::with_label("+1");
        add.set_valign(gtk4::Align::Center);
        add.set_hexpand(true);
        cell.append(&sub);
        cell.append(&value);
        cell.append(&add);
        grid.attach(&cell, column, row, 1, 1);
        widgets.push(cell.upcast());
        (value, add, sub)
    };
    let (home_value, home_add, home_sub) = side(1);
    let (away_value, away_add, away_sub) = side(2);

    CounterRows {
        widgets,
        home_value,
        away_value,
        home_add,
        away_add,
        home_sub,
        away_sub,
    }
}

/// Race settings for the day: target, goals per segment and the resulting
/// number of segments, and a row for one button per segment's game.
fn race_controls() -> RaceControls {
    let settings_row = GtkBox::new(Orientation::Horizontal, 8);
    let target = SpinButton::with_range(1.0, 999.0, 1.0);
    target.set_tooltip_text(Some("Ein Unentschieden ist ein Tor davor."));
    let segment = SpinButton::with_range(0.0, 99.0, 1.0);
    segment.set_tooltip_text(Some("0: keine Segmente"));
    let segments_label = Label::new(None);
    segments_label.add_css_class("dim-label");
    settings_row.append(&Label::new(Some("Race bis")));
    settings_row.append(&target);
    settings_row.append(&Label::new(Some("Tore je Segment")));
    settings_row.append(&segment);
    settings_row.append(&segments_label);

    let games_label = Label::new(Some("Spiele:"));
    games_label.set_width_chars(12);
    games_label.set_xalign(0.0);
    let games = GtkBox::new(Orientation::Horizontal, 0);
    games.add_css_class("linked");
    games.set_halign(gtk4::Align::Start);
    RaceControls {
        settings_row,
        target,
        segment,
        segments_label,
        games_label,
        games,
    }
}
