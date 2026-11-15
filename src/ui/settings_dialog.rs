use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, Dialog, Entry, FileChooserAction, FileChooserDialog,
    InputPurpose, Label, Orientation, ResponseType, SpinButton,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use crate::services::{Settings, SettingsService};

pub struct SettingsDialog {
    dialog: Dialog,
    api_key_entry: Entry,
    port_entry: Entry,
    refresh_spin: SpinButton,
    /// The overlay folder as chosen in the dialog.
    overlay_dir: Rc<RefCell<PathBuf>>,
    default_overlay_dir: PathBuf,
    /// As loaded; fields the dialog does not show are kept.
    settings: Settings,
}

impl SettingsDialog {
    pub fn new(parent: &ApplicationWindow, settings_service: Arc<SettingsService>) -> Self {
        let dialog = Dialog::builder()
            .title("Einstellungen")
            .transient_for(parent)
            .modal(true)
            .default_width(500)
            .build();

        // The buttons are part of the content (see the end), so they keep
        // the same margin to the window border as everything else.
        let content = dialog.content_area();
        content.set_spacing(12);
        content.set_margin_top(20);
        content.set_margin_bottom(20);
        content.set_margin_start(20);
        content.set_margin_end(20);

        // Load current settings
        let settings = settings_service.load();
        let api_key = settings_service.load_api_key().unwrap_or_default();

        // API Key
        let api_key_box = GtkBox::new(Orientation::Horizontal, 8);
        let api_key_label = Label::new(Some("API-Schlüssel:"));
        api_key_label.set_width_chars(15);
        api_key_label.set_xalign(0.0);
        let api_key_entry = Entry::builder()
            .text(&api_key)
            .visibility(false)
            .hexpand(true)
            .placeholder_text("tournament.io-API-Schlüssel")
            .build();
        api_key_box.append(&api_key_label);
        api_key_box.append(&api_key_entry);
        content.append(&api_key_box);

        // Port
        let port_box = GtkBox::new(Orientation::Horizontal, 8);
        let port_label = Label::new(Some("Webserver-Port:"));
        port_label.set_width_chars(15);
        port_label.set_xalign(0.0);
        let port_entry = Entry::builder()
            .text(settings.port.to_string())
            .input_purpose(InputPurpose::Digits)
            .max_length(5)
            .width_chars(6)
            .tooltip_text("1024–65535")
            .build();
        port_box.append(&port_label);
        port_box.append(&port_entry);
        content.append(&port_box);

        // Refresh Interval
        let refresh_box = GtkBox::new(Orientation::Horizontal, 8);
        let refresh_label = Label::new(Some("Kickertool-Abfrage:"));
        refresh_label.set_width_chars(15);
        refresh_label.set_xalign(0.0);
        let refresh_spin = SpinButton::with_range(1.0, 300.0, 1.0);
        refresh_spin.set_value(settings.refresh_interval as f64);
        let refresh_suffix = Label::new(Some("Sekunden"));
        refresh_box.append(&refresh_label);
        refresh_box.append(&refresh_spin);
        refresh_box.append(&refresh_suffix);
        content.append(&refresh_box);

        // All overlay templates come from this folder; the one shown on air
        // is chosen in the toolbar.
        // The full path on its own line, the buttons below it.
        let overlay_box = GtkBox::new(Orientation::Horizontal, 8);
        let overlay_label = Label::new(Some("Overlay-Ordner:"));
        overlay_label.set_width_chars(15);
        overlay_label.set_xalign(0.0);
        overlay_label.set_valign(gtk4::Align::Start);
        let overlay_dir = Rc::new(RefCell::new(settings_service.overlay_dir(&settings)));
        let overlay_dir_label = Label::builder()
            .label(overlay_dir.borrow().to_string_lossy().as_ref())
            .hexpand(true)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::WordChar)
            .selectable(true)
            .build();
        overlay_box.append(&overlay_label);
        overlay_box.append(&overlay_dir_label);
        content.append(&overlay_box);

        let overlay_buttons = GtkBox::new(Orientation::Horizontal, 8);
        let indent = Label::new(None);
        indent.set_width_chars(15);
        let choose_button = Button::with_label("Wählen…");
        let open_overlays_button = Button::with_label("Ordner öffnen");
        overlay_buttons.append(&indent);
        overlay_buttons.append(&choose_button);
        overlay_buttons.append(&open_overlays_button);
        content.append(&overlay_buttons);

        let dialog_clone = dialog.clone();
        let chosen = Rc::clone(&overlay_dir);
        choose_button.connect_clicked(move |_| {
            let chooser = FileChooserDialog::new(
                Some("Overlay-Ordner wählen"),
                Some(&dialog_clone),
                FileChooserAction::SelectFolder,
                &[
                    ("Abbrechen", ResponseType::Cancel),
                    ("Wählen", ResponseType::Accept),
                ],
            );
            let _ = chooser.set_current_folder(Some(&gtk4::gio::File::for_path(&*chosen.borrow())));
            let chosen = Rc::clone(&chosen);
            let label = overlay_dir_label.clone();
            chooser.connect_response(move |chooser, response| {
                if response == ResponseType::Accept
                    && let Some(path) = chooser.file().and_then(|f| f.path())
                {
                    label.set_label(path.to_string_lossy().as_ref());
                    *chosen.borrow_mut() = path;
                }
                chooser.close();
            });
            chooser.present();
        });

        let dialog_clone = dialog.clone();
        let shown = Rc::clone(&overlay_dir);
        open_overlays_button.connect_clicked(move |_| {
            let dir = shown.borrow().clone();
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!("Cannot create {}: {e}", dir.display());
                return;
            }
            let uri = gtk4::gio::File::for_path(&dir).uri();
            gtk4::show_uri(Some(&dialog_clone), &uri, 0);
        });

        let buttons = GtkBox::new(Orientation::Horizontal, 8);
        buttons.set_halign(gtk4::Align::End);
        buttons.set_margin_top(12);
        for (label, response) in [
            ("Abbrechen", ResponseType::Cancel),
            ("Speichern", ResponseType::Accept),
        ] {
            let button = Button::with_label(label);
            if response == ResponseType::Accept {
                button.add_css_class("suggested-action");
            }
            let dialog = dialog.clone();
            button.connect_clicked(move |_| dialog.response(response));
            buttons.append(&button);
        }
        content.append(&buttons);

        Self {
            dialog,
            api_key_entry,
            port_entry,
            refresh_spin,
            overlay_dir,
            default_overlay_dir: settings_service.overlay_dir(&Settings::default()),
            settings,
        }
    }

    pub fn run<F>(&self, callback: F)
    where
        F: Fn(Option<(Settings, String)>) + 'static,
    {
        let api_key_entry = self.api_key_entry.clone();
        let port_entry = self.port_entry.clone();
        let refresh_spin = self.refresh_spin.clone();
        let overlay_dir = Rc::clone(&self.overlay_dir);
        let default_overlay_dir = self.default_overlay_dir.clone();
        let loaded = self.settings.clone();

        self.dialog.connect_response(move |dialog, response| {
            let result = if response == ResponseType::Accept {
                // An invalid port keeps the current one.
                let port = port_entry
                    .text()
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|p| *p >= 1024)
                    .unwrap_or(loaded.port);
                let dir = overlay_dir.borrow().clone();
                let settings = Settings {
                    port,
                    refresh_interval: refresh_spin.value() as u32,
                    overlay_dir: (dir != default_overlay_dir).then_some(dir),
                    ..loaded.clone()
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
