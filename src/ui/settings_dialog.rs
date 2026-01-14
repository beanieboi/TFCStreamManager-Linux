use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, CheckButton, Dialog, Entry, FileChooserAction,
    FileChooserDialog, Label, Orientation, ResponseType, SpinButton,
};
use std::sync::Arc;

use crate::services::{Settings, SettingsService};

pub struct SettingsDialog {
    dialog: Dialog,
    api_key_entry: Entry,
    port_spin: SpinButton,
    refresh_spin: SpinButton,
    overlay_path_entry: Entry,
    show_sets_check: CheckButton,
    show_score_check: CheckButton,
}

impl SettingsDialog {
    pub fn new(parent: &ApplicationWindow, settings_service: Arc<SettingsService>) -> Self {
        let dialog = Dialog::builder()
            .title("Settings")
            .transient_for(parent)
            .modal(true)
            .default_width(500)
            .build();

        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Save", ResponseType::Accept);

        let content = dialog.content_area();
        content.set_spacing(12);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);

        // Load current settings
        let settings = settings_service.load();
        let api_key = settings_service.load_api_key().unwrap_or_default();

        // API Key
        let api_key_box = GtkBox::new(Orientation::Horizontal, 8);
        let api_key_label = Label::new(Some("API Key:"));
        api_key_label.set_width_chars(15);
        api_key_label.set_xalign(0.0);
        let api_key_entry = Entry::builder()
            .text(&api_key)
            .visibility(false)
            .hexpand(true)
            .placeholder_text("Enter your tournament.io API key")
            .build();
        api_key_box.append(&api_key_label);
        api_key_box.append(&api_key_entry);
        content.append(&api_key_box);

        // Port
        let port_box = GtkBox::new(Orientation::Horizontal, 8);
        let port_label = Label::new(Some("Web Server Port:"));
        port_label.set_width_chars(15);
        port_label.set_xalign(0.0);
        let port_spin = SpinButton::with_range(1024.0, 65535.0, 1.0);
        port_spin.set_value(settings.port as f64);
        port_box.append(&port_label);
        port_box.append(&port_spin);
        content.append(&port_box);

        // Refresh Interval
        let refresh_box = GtkBox::new(Orientation::Horizontal, 8);
        let refresh_label = Label::new(Some("Refresh Interval:"));
        refresh_label.set_width_chars(15);
        refresh_label.set_xalign(0.0);
        let refresh_spin = SpinButton::with_range(1.0, 300.0, 1.0);
        refresh_spin.set_value(settings.refresh_interval as f64);
        let refresh_suffix = Label::new(Some("seconds"));
        refresh_box.append(&refresh_label);
        refresh_box.append(&refresh_spin);
        refresh_box.append(&refresh_suffix);
        content.append(&refresh_box);

        // Overlay Path
        let overlay_box = GtkBox::new(Orientation::Horizontal, 8);
        let overlay_label = Label::new(Some("Overlay Template:"));
        overlay_label.set_width_chars(15);
        overlay_label.set_xalign(0.0);
        let overlay_path = settings_service.get_overlay_path(&settings);
        let overlay_path_entry = Entry::builder()
            .text(overlay_path.to_string_lossy().as_ref())
            .hexpand(true)
            .build();
        let browse_button = Button::with_label("Browse...");
        overlay_box.append(&overlay_label);
        overlay_box.append(&overlay_path_entry);
        overlay_box.append(&browse_button);
        content.append(&overlay_box);

        // Display options
        let display_label = Label::builder()
            .label("<b>Display Options</b>")
            .use_markup(true)
            .xalign(0.0)
            .margin_top(12)
            .build();
        content.append(&display_label);

        let show_sets_check = CheckButton::with_label("Show Sets");
        show_sets_check.set_active(settings.show_sets);
        content.append(&show_sets_check);

        let show_score_check = CheckButton::with_label("Show Score");
        show_score_check.set_active(settings.show_score);
        content.append(&show_score_check);

        // Browse button handler
        let overlay_entry_clone = overlay_path_entry.clone();
        let dialog_clone = dialog.clone();
        browse_button.connect_clicked(move |_| {
            let file_chooser = FileChooserDialog::new(
                Some("Select Overlay Template"),
                Some(&dialog_clone),
                FileChooserAction::Open,
                &[
                    ("Cancel", ResponseType::Cancel),
                    ("Open", ResponseType::Accept),
                ],
            );

            let entry = overlay_entry_clone.clone();
            file_chooser.connect_response(move |chooser, response| {
                if response == ResponseType::Accept
                    && let Some(file) = chooser.file()
                    && let Some(path) = file.path()
                {
                    entry.set_text(path.to_string_lossy().as_ref());
                }
                chooser.close();
            });

            file_chooser.present();
        });

        Self {
            dialog,
            api_key_entry,
            port_spin,
            refresh_spin,
            overlay_path_entry,
            show_sets_check,
            show_score_check,
        }
    }

    pub fn run<F>(&self, callback: F)
    where
        F: Fn(Option<(Settings, String)>) + 'static,
    {
        let api_key_entry = self.api_key_entry.clone();
        let port_spin = self.port_spin.clone();
        let refresh_spin = self.refresh_spin.clone();
        let overlay_path_entry = self.overlay_path_entry.clone();
        let show_sets_check = self.show_sets_check.clone();
        let show_score_check = self.show_score_check.clone();

        self.dialog.connect_response(move |dialog, response| {
            let result = if response == ResponseType::Accept {
                let settings = Settings {
                    port: port_spin.value() as u16,
                    refresh_interval: refresh_spin.value() as u32,
                    overlay_path: Some(overlay_path_entry.text().to_string()),
                    show_sets: show_sets_check.is_active(),
                    show_score: show_score_check.is_active(),
                };
                let api_key = api_key_entry.text().to_string();
                Some((settings, api_key))
            } else {
                None
            };

            dialog.close();
            callback(result);
        });

        self.dialog.present();
    }
}
