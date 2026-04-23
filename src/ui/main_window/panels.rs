use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, ComboBoxText, Entry, Label, Orientation, Stack, ToggleButton};

use super::{
    HeaderControls, KickertoolControls, MainWindow, ManualControls, ManualForm, ModeButtons,
};

const REMOTE_HELP_TEXT: &str = "Remote mode active.\n\nSend POST requests to /scores endpoint with JSON:\n\n{\n  \"teamAScore\": 0,\n  \"teamBScore\": 0,\n  \"teamAName\": \"Team A\",\n  \"teamBName\": \"Team B\",\n  \"eventName\": \"Tournament\"\n}";

impl MainWindow {
    pub(super) fn build_header() -> (GtkBox, HeaderControls) {
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

    pub(super) fn build_mode_buttons() -> (GtkBox, ModeButtons) {
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

    pub(super) fn build_content_stack() -> (Stack, KickertoolControls, ManualControls) {
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

        let form = ManualForm {
            tournament: Self::append_labeled_entry(&panel, "Tournament:"),
            discipline: Self::append_labeled_entry(&panel, "Discipline:"),
            round: Self::append_labeled_entry(&panel, "Round:"),
            group: Self::append_labeled_entry(&panel, "Group:"),
            team_a: Self::append_labeled_entry(&panel, "Team A:"),
            team_b: Self::append_labeled_entry(&panel, "Team B:"),
            team_a_player: Self::append_labeled_entry(&panel, "Player A:"),
            team_b_player: Self::append_labeled_entry(&panel, "Player B:"),
            score_a: Self::append_labeled_entry(&panel, "Score A:"),
            score_b: Self::append_labeled_entry(&panel, "Score B:"),
            sets_a: Self::append_labeled_entry(&panel, "Sets A:"),
            sets_b: Self::append_labeled_entry(&panel, "Sets B:"),
        };

        let update_button = Button::with_label("Update Overlay");
        panel.append(&update_button);

        (
            panel,
            ManualControls {
                form,
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

    fn append_labeled_entry(panel: &GtkBox, label_text: &str) -> Entry {
        let entry = Entry::new();
        let row = Self::build_labeled_entry(label_text, &entry);
        panel.append(&row);
        entry
    }
}
