use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Grid, Label, LinkButton, Orientation};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use super::MainWindow;
use crate::services::trackers::{Battery, Reading};
use crate::services::{TrackerStatus, Trackers};

/// The tracker phones, one row each. Self-contained: it only reads the
/// reports and never changes anything.
pub(super) struct TrackerPanel {
    grid: Grid,
    empty: Label,
    /// The phone id per shown row, in order.
    rows: RefCell<Vec<(String, TrackerRow)>>,
}

struct TrackerRow {
    name: Label,
    place: Label,
    battery: Label,
    reading: Label,
    seen: Label,
    link: LinkButton,
}

impl MainWindow {
    pub(super) fn build_tracker_panel() -> (GtkBox, Rc<TrackerPanel>) {
        let panel = GtkBox::new(Orientation::Vertical, 8);

        let title = Label::new(Some("Tracker"));
        title.set_xalign(0.0);
        title.add_css_class("heading");
        panel.append(&title);

        let empty = Label::new(Some("Noch keine Tracker gemeldet"));
        empty.set_xalign(0.0);
        empty.add_css_class("dim-label");
        panel.append(&empty);

        let grid = Grid::builder().row_spacing(4).column_spacing(16).build();
        panel.append(&grid);

        (
            panel,
            Rc::new(TrackerPanel {
                grid,
                empty,
                rows: Default::default(),
            }),
        )
    }

    /// Every second, not on each report: "last seen" counts up between
    /// reports, and a phone going offline is only noticed by time passing.
    /// Runs as long as the app does.
    pub(super) fn follow_trackers(panel: Rc<TrackerPanel>, trackers: Trackers) {
        panel.show(&trackers.snapshot());
        glib::timeout_add_local(Duration::from_secs(1), move || {
            panel.show(&trackers.snapshot());
            glib::ControlFlow::Continue
        });
    }
}

impl TrackerPanel {
    fn show(&self, phones: &[TrackerStatus]) {
        self.empty.set_visible(phones.is_empty());
        let mut rows = self.rows.borrow_mut();
        let same_phones = rows
            .iter()
            .map(|(id, _)| id)
            .eq(phones.iter().map(|p| &p.report.id));
        // Rebuilt only when phones come, go or reorder, so the link is not
        // replaced under the pointer every second.
        if !same_phones {
            while let Some(child) = self.grid.first_child() {
                self.grid.remove(&child);
            }
            *rows = phones
                .iter()
                .zip(0..)
                .map(|(phone, row)| (phone.report.id.clone(), TrackerRow::attach(&self.grid, row)))
                .collect();
        }
        for ((_, row), phone) in rows.iter().zip(phones) {
            row.show(phone);
        }
    }
}

impl TrackerRow {
    fn attach(grid: &Grid, row: i32) -> Self {
        let label = |column: i32| {
            let label = Label::new(None);
            label.set_xalign(0.0);
            grid.attach(&label, column, row, 1, 1);
            label
        };
        let name = label(0);
        name.set_max_width_chars(24);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let place = label(1);
        place.add_css_class("dim-label");
        let battery = label(2);
        let reading = label(3);
        reading.set_hexpand(true);
        let seen = label(4);
        seen.set_xalign(1.0);
        let link = LinkButton::new("");
        link.set_tooltip_text(Some("Debug-Seite des Telefons öffnen"));
        grid.attach(&link, 5, row, 1, 1);
        Self {
            name,
            place,
            battery,
            reading,
            seen,
            link,
        }
    }

    fn show(&self, phone: &TrackerStatus) {
        let report = &phone.report;
        self.name.set_text(report.label());
        self.name.set_tooltip_text(Some(&details_text(phone)));
        self.place.set_text(&place_text(
            report.table_end.as_deref(),
            report.side.as_deref(),
        ));

        self.battery
            .set_text(&battery_text(report.battery.as_ref()));
        if battery_low(report.battery.as_ref()) {
            self.battery.add_css_class("warning");
        } else {
            self.battery.remove_css_class("warning");
        }

        self.reading
            .set_text(&reading_text(report.reading.as_ref()));
        self.seen.set_text(&seen_text(phone.last_seen));

        self.link.set_label(report.ip.as_deref().unwrap_or("–"));
        self.link.set_uri(report.debug_url.as_deref().unwrap_or(""));
        self.link.set_sensitive(report.debug_url.is_some());

        let opacity = if phone.online() { 1.0 } else { 0.5 };
        for widget in [
            self.name.upcast_ref::<gtk4::Widget>(),
            self.place.upcast_ref(),
            self.battery.upcast_ref(),
            self.reading.upcast_ref(),
            self.seen.upcast_ref(),
            self.link.upcast_ref(),
        ] {
            widget.set_opacity(opacity);
        }
    }
}

/// "Ende A · Heim": where the phone is and whose goals it counts.
fn place_text(table_end: Option<&str>, side: Option<&str>) -> String {
    let end = table_end.map(|end| format!("Ende {end}"));
    let side = side.map(|side| match side {
        "home" => "Heim".to_owned(),
        "away" => "Gast".to_owned(),
        other => other.to_owned(),
    });
    [end, side]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

fn battery_text(battery: Option<&Battery>) -> String {
    let Some(battery) = battery else {
        return "–".into();
    };
    match (battery.level, battery.charging) {
        (Some(level), Some(true)) => format!("{level} %, lädt"),
        (Some(level), _) => format!("{level} %"),
        (None, Some(true)) => "lädt".into(),
        (None, _) => "–".into(),
    }
}

/// A phone that is about to switch off; one on the charger is fine.
fn battery_low(battery: Option<&Battery>) -> bool {
    battery.is_some_and(|b| b.level.is_some_and(|l| l < 20) && b.charging != Some(true))
}

fn reading_text(reading: Option<&Reading>) -> String {
    let Some(reading) = reading else {
        return "–".into();
    };
    let count = |v: Option<u32>| v.map_or("–".into(), |v| v.to_string());
    format!(
        "Tore {} · Sätze {} · Auszeiten {}",
        count(reading.goals),
        count(reading.sets),
        count(reading.timeouts)
    )
}

/// Reports come every 5 s, so "jetzt" only right after one.
fn seen_text(last_seen: Duration) -> String {
    match last_seen.as_secs() {
        0..2 => "jetzt".into(),
        secs @ 2..120 => format!("vor {secs} s"),
        secs => format!("vor {} min", secs / 60),
    }
}

/// For the tooltip: what helps to tell why a phone misreads.
fn details_text(phone: &TrackerStatus) -> String {
    let report = &phone.report;
    let mut lines = vec![format!("ID {}", report.id)];
    if let Some(version) = &report.app_version {
        lines.push(format!("App {version}"));
    }
    if let Some(rssi) = report.wifi_rssi {
        lines.push(format!("WLAN {rssi} dBm"));
    }
    if let Some(temperature) = report.battery.as_ref().and_then(|b| b.temperature_c) {
        lines.push(format!("Akku {temperature:.1} °C"));
    }
    if let Some(fps) = report.camera.as_ref().and_then(|c| c.fps) {
        lines.push(format!("Kamera {fps:.1} fps"));
    }
    let problems = report.reading.as_ref().map_or(&[][..], |r| &r.problems);
    if !problems.is_empty() {
        lines.push(format!("Probleme: {}", problems.join(", ")));
    }
    if let Some(calibration) = &report.calibration {
        lines.push(format!("Kalibrierung: {calibration}"));
    }
    if let Some(sync) = &report.sync {
        lines.push(format!("Sync: {sync}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_warns_when_low_and_not_charging() {
        let battery = |level, charging| Battery {
            level,
            charging,
            temperature_c: None,
        };
        assert_eq!(
            battery_text(Some(&battery(Some(87), Some(true)))),
            "87 %, lädt"
        );
        assert_eq!(battery_text(Some(&battery(Some(19), None))), "19 %");
        assert_eq!(battery_text(None), "–");
        assert!(battery_low(Some(&battery(Some(19), Some(false)))));
        assert!(battery_low(Some(&battery(Some(19), None))));
        assert!(!battery_low(Some(&battery(Some(19), Some(true)))));
        assert!(!battery_low(Some(&battery(Some(20), None))));
        assert!(!battery_low(Some(&battery(None, None))));
    }

    #[test]
    fn texts() {
        assert_eq!(place_text(Some("A"), Some("home")), "Ende A · Heim");
        assert_eq!(place_text(None, Some("away")), "Gast");
        assert_eq!(place_text(None, None), "");
        let reading = Reading {
            goals: Some(2),
            sets: Some(0),
            timeouts: None,
            ..Default::default()
        };
        assert_eq!(
            reading_text(Some(&reading)),
            "Tore 2 · Sätze 0 · Auszeiten –"
        );
        assert_eq!(seen_text(Duration::from_millis(1500)), "jetzt");
        assert_eq!(seen_text(Duration::from_secs(12)), "vor 12 s");
        assert_eq!(seen_text(Duration::from_secs(300)), "vor 5 min");
    }
}
