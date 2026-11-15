//! The debug window: tails the app log file and shows it as rows (time,
//! level, source, message), filtered by level and coloured by it; long
//! messages wrap. It jumps
//! to the latest entry on opening and follows new ones while at the bottom.

use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, CssProvider, CustomFilter, DropDown, FilterChange,
    FilterListModel, Label, ListItem, ListView, NoSelection, Orientation, ScrolledWindow,
    SignalListItemFactory, Window, gio, glib::BoxedAnyObject,
};
use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use crate::services::logging::{Entry, Level, parse_line};

/// How much of the log is read on opening.
const INITIAL_BYTES: u64 = 512 * 1024;
/// The oldest entries go beyond this many.
const MAX_ENTRIES: u32 = 20_000;
const POLL: Duration = Duration::from_millis(500);
/// The levels offered, lowest first; DEBUG (the default) shows everything
/// the app writes.
const LEVELS: [Level; 4] = [Level::Debug, Level::Info, Level::Warn, Level::Error];

const CSS: &str = "
.log-window, .log-window listview, .log-window listview > row {
    background-color: #0f1e29;
    color: #d7e3ea;
}
.log-window .toolbar {
    padding: 8px 12px; background-color: #0f1e29; border-bottom: 1px solid #22313b;
}
.log-window .toolbar label { color: #9fb7c6; }
.log-window .toolbar dropdown > button {
    background-color: #22313b; background-image: none; color: #d7e3ea;
    border: 1px solid #3a5568; border-radius: 6px; padding: 2px 10px;
    box-shadow: none;
}
.log-window .toolbar dropdown > button:hover { background-color: #2c4050; }
.log-window .toolbar dropdown > button label,
.log-window .toolbar dropdown > button arrow { color: #d7e3ea; -gtk-icon-style: symbolic; }
.log-window .log-header {
    padding: 4px 12px; color: #9fb7c6; border-bottom: 1px solid #22313b;
}
.log-window .log-header label { color: #9fb7c6; font-weight: bold; }
.log-window .log-row { padding: 1px 12px; }
.log-window listview > row:hover { background-color: #17303f; }
.log-window .mono { font-family: monospace; }
.log-window .dim { color: #7d8f9b; }
.log-window .badge { border-radius: 4px; padding: 0 6px; font-weight: bold; font-family: monospace; }
.log-window .badge.error { background-color: #5c1f24; color: #ffb4ad; }
.log-window .badge.warn  { background-color: #5a4614; color: #f2cc60; }
.log-window .badge.info  { background-color: #163f5c; color: #8fd0f5; }
.log-window .badge.debug { background-color: #22313b; color: #8a9aa6; }
.log-window .badge.trace { background-color: #1a252d; color: #6b7a85; }
.log-window .message.error { color: #ff8f86; }
.log-window .message.warn  { color: #f2cc60; }
.log-window .message.info  { color: #d7e3ea; }
.log-window .message.debug { color: #9aabb6; }
.log-window .message.trace { color: #6b7a85; }
.log-window .log-row.launch {
    border-top: 1px solid #3a5568; margin-top: 14px; padding-top: 6px; padding-bottom: 4px;
}
.log-window .log-row.launch label { color: #63b1dc; font-weight: bold; }
";

/// A row of the table: a log entry, or the start of an app run.
enum Row {
    Entry(Entry),
    Launch { time: String, launch: String },
}

/// Built on every open and gone on close; the log file keeps everything,
/// so nothing has to survive in between.
pub struct DebugWindow;

impl DebugWindow {
    /// Opens a new window that shows the end of the log file and follows it.
    pub fn open(parent: &ApplicationWindow, path: PathBuf) -> Window {
        // A window of the application, like the main window: GTK then knows
        // both when focus moves, and does not hand it back to the main
        // window (seen on macOS while resizing an unregistered window).
        let window = Window::builder()
            .title("Debug-Log")
            .default_width(1000)
            .default_height(520)
            .build();
        if let Some(app) = parent.application() {
            window.set_application(Some(&app));
        }
        window.set_display(&WidgetExt::display(parent));
        window.add_css_class("log-window");
        let css = CssProvider::new();
        css.load_from_data(CSS);
        gtk4::style_context_add_provider_for_display(
            &WidgetExt::display(parent),
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // Entries, filtered by the chosen level.
        let store = gio::ListStore::new::<BoxedAnyObject>();
        let min_level = Rc::new(Cell::new(LEVELS[0]));
        let filter = {
            let min_level = Rc::clone(&min_level);
            CustomFilter::new(move |item| {
                item.downcast_ref::<BoxedAnyObject>().is_some_and(|obj| {
                    match &*obj.borrow::<Row>() {
                        Row::Entry(entry) => entry.level >= min_level.get(),
                        // Starts stay visible, whatever the level.
                        Row::Launch { .. } => true,
                    }
                })
            })
        };
        let filtered = FilterListModel::new(Some(store.clone()), Some(filter.clone()));
        // A ListView of row widgets, not a ColumnView: only a ListView gives
        // a row the height its wrapped message needs. Fixed widths for time,
        // level and source keep the columns aligned with the header.
        let factory = SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            if let Some(item) = item.downcast_ref::<ListItem>() {
                item.set_child(Some(&row_widget()));
            }
        });
        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<ListItem>() else {
                return;
            };
            let (Some(obj), Some(row)) = (
                item.item().and_downcast::<BoxedAnyObject>(),
                item.child().and_downcast::<GtkBox>(),
            ) else {
                return;
            };
            bind_row(&row, &obj.borrow::<Row>());
        });
        let view = ListView::new(
            Some(NoSelection::new(Some(filtered.clone()))),
            Some(factory),
        );

        let header = row_widget();
        header.add_css_class("log-header");
        for (label, title) in row_labels(&header)
            .iter()
            .zip(["Zeit", "Level", "Quelle", "Meldung"])
        {
            label.set_text(title);
            label.set_css_classes(&[]);
        }

        // No sideways scrolling: long messages wrap.
        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .vexpand(true)
            .child(&view)
            .build();

        // Toolbar: level filter and the number of entries shown.
        let toolbar = GtkBox::new(Orientation::Horizontal, 8);
        toolbar.add_css_class("toolbar");
        let level_names: Vec<&str> = LEVELS.iter().map(|l| l.as_str()).collect();
        let level_dropdown = DropDown::from_strings(&level_names);
        level_dropdown.set_tooltip_text(Some("Dieses Level und alles darüber zeigen"));
        let count = Label::new(None);
        count.set_hexpand(true);
        count.set_xalign(1.0);
        toolbar.append(&Label::new(Some("Level:")));
        toolbar.append(&level_dropdown);
        toolbar.append(&count);

        let content = GtkBox::new(Orientation::Vertical, 0);
        content.append(&toolbar);
        content.append(&header);
        content.append(&scrolled);
        window.set_child(Some(&content));

        let scroll_to_end = {
            let scrolled = scrolled.clone();
            move || {
                // After GTK has laid out the new rows.
                let adj = scrolled.vadjustment();
                glib::idle_add_local_once(move || adj.set_value(adj.upper() - adj.page_size()));
            }
        };
        let show_count = {
            let (count, filtered, store) = (count.clone(), filtered.clone(), store.clone());
            move || {
                count.set_text(&format!(
                    "{} von {} Einträgen",
                    filtered.n_items(),
                    store.n_items()
                ));
            }
        };

        {
            let (filter, min_level) = (filter.clone(), Rc::clone(&min_level));
            let (scroll_to_end, show_count) = (scroll_to_end.clone(), show_count.clone());
            level_dropdown.connect_selected_notify(move |dropdown| {
                let level = LEVELS
                    .get(dropdown.selected() as usize)
                    .copied()
                    .unwrap_or(LEVELS[0]);
                min_level.set(level);
                filter.changed(FilterChange::Different);
                show_count();
                scroll_to_end();
            });
        }

        // Tail the file: everything new is parsed into entries.
        let tail = Tail::new(path);
        let first = Cell::new(true);
        let tick = move || {
            let adj = scrolled.vadjustment();
            let at_end = adj.value() + adj.page_size() >= adj.upper() - 1.0;
            if !tail.read_into(&store) {
                return;
            }
            show_count();
            // On opening, and while the reader stays at the bottom.
            if first.replace(false) || at_end {
                scroll_to_end();
            }
        };
        tick();
        let poll = glib::timeout_add_local(POLL, move || {
            tick();
            glib::ControlFlow::Continue
        });
        // Closing ends the following too.
        let poll = RefCell::new(Some(poll));
        window.connect_destroy(move |_| {
            if let Some(poll) = poll.take() {
                poll.remove();
            }
        });

        window.present();
        window
    }
}

fn css_class(level: Level) -> &'static str {
    match level {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
    }
}

/// Time · level · source · message. Fixed widths for the first three, so
/// rows and header line up; the message takes the rest and wraps.
fn row_widget() -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("log-row");
    let label = |width: i32| {
        let label = Label::builder().xalign(0.0).yalign(0.0).build();
        label.set_valign(gtk4::Align::Start);
        if width > 0 {
            label.set_width_chars(width);
            label.set_max_width_chars(width);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        }
        label
    };
    let time = label(8);
    let level = label(5);
    level.set_xalign(0.5);
    let source = label(18);
    let message = label(0);
    message.set_hexpand(true);
    message.set_wrap(true);
    message.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    for widget in [&time, &level, &source, &message] {
        row.append(widget);
    }
    row
}

fn row_labels(row: &GtkBox) -> Vec<Label> {
    let mut labels = Vec::new();
    let mut child = row.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Ok(label) = widget.downcast::<Label>() {
            labels.push(label);
        }
    }
    labels
}

fn bind_row(row: &GtkBox, content: &Row) {
    let Ok([time, level, source, message]) = <[Label; 4]>::try_from(row_labels(row)) else {
        return;
    };
    match content {
        Row::Entry(entry) => {
            row.remove_css_class("launch");
            time.set_text(entry.time());
            time.set_css_classes(&["mono", "dim"]);
            level.set_text(entry.level.as_str());
            level.set_css_classes(&["badge", css_class(entry.level)]);
            source.set_text(entry.short_source());
            source.set_tooltip_text(Some(&entry.source));
            source.set_css_classes(&["mono", "dim"]);
            message.set_text(&entry.message);
            message.set_css_classes(&["mono", "message", css_class(entry.level)]);
        }
        Row::Launch { time: at, launch } => {
            row.add_css_class("launch");
            let launch = if launch.is_empty() { "–" } else { launch };
            time.set_text(at);
            time.set_css_classes(&["mono"]);
            level.set_text("START");
            level.set_css_classes(&["mono"]);
            source.set_text("");
            source.set_tooltip_text(None);
            source.set_css_classes(&[]);
            message.set_text(&format!("App-Start · Launch {launch}"));
            message.set_css_classes(&[]);
        }
    }
}

/// Reads what was added to the log file since the last call.
struct Tail {
    path: PathBuf,
    position: Cell<Option<u64>>,
    /// The end of the last read when it was not a full line yet.
    partial: RefCell<String>,
    /// The launch of the last entry; a new one starts with a separator.
    launch: RefCell<Option<String>>,
}

impl Tail {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            position: Cell::new(None),
            partial: RefCell::new(String::new()),
            launch: RefCell::new(None),
        }
    }

    /// Appends new entries; a line that is no entry of its own (e.g. part
    /// of a longer message) is added to the one before. A rotated file is
    /// read again from the start. Returns whether anything changed.
    fn read_into(&self, store: &gio::ListStore) -> bool {
        let Some((text, restarted)) = self.read_new(&self.path) else {
            return false;
        };
        if restarted {
            store.remove_all();
            self.launch.replace(None);
        }
        let mut partial = self.partial.borrow_mut();
        partial.push_str(&text);
        let Some(complete) = partial.rfind('\n') else {
            return restarted;
        };
        let lines: String = partial.drain(..=complete).collect();
        let mut added = Vec::new();
        let mut current_launch = self.launch.borrow_mut();
        for line in lines.lines() {
            match parse_line(line) {
                Some(entry) => {
                    if current_launch.as_deref() != Some(entry.launch.as_str()) {
                        *current_launch = Some(entry.launch.clone());
                        added.push(BoxedAnyObject::new(Row::Launch {
                            time: entry.time().to_owned(),
                            launch: entry.launch.clone(),
                        }));
                    }
                    added.push(BoxedAnyObject::new(Row::Entry(entry)));
                }
                None if !line.trim().is_empty() => {
                    let last = added.last().cloned().or_else(|| {
                        store
                            .item(store.n_items().saturating_sub(1))
                            .and_downcast::<BoxedAnyObject>()
                    });
                    if let Some(last) = last
                        && let Row::Entry(entry) = &mut *last.borrow_mut::<Row>()
                    {
                        entry.message.push('\n');
                        entry.message.push_str(line);
                    }
                }
                None => {}
            }
        }
        store.extend_from_slice(&added);
        let excess = store.n_items().saturating_sub(MAX_ENTRIES);
        if excess > 0 {
            store.splice(0, excess, &[] as &[BoxedAnyObject]);
        }
        true
    }

    /// The text added since the last read, and whether the file was read
    /// again from the start. On the first read only the last part, from a
    /// line start.
    fn read_new(&self, path: &Path) -> Option<(String, bool)> {
        let mut file = File::open(path).ok()?;
        let len = file.metadata().ok()?.len();
        let first_read = self.position.get().is_none();
        let (start, restarted) = match self.position.get() {
            Some(pos) if pos <= len => (pos, false),
            Some(_) => {
                self.partial.borrow_mut().clear();
                (0, true)
            }
            None => (len.saturating_sub(INITIAL_BYTES), false),
        };
        if start == len {
            self.position.set(Some(len));
            return restarted.then(|| (String::new(), true));
        }
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        file.take(len - start).read_to_end(&mut bytes).ok()?;
        self.position.set(Some(len));
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if first_read && start > 0 {
            // Started mid-file: from the next full line on.
            let rest = text.split_once('\n').map(|(_, rest)| rest.to_owned())?;
            return Some((rest, restarted));
        }
        Some((text, restarted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn entries(store: &gio::ListStore) -> Vec<String> {
        (0..store.n_items())
            .filter_map(|i| store.item(i).and_downcast::<BoxedAnyObject>())
            .map(|obj| match &*obj.borrow::<Row>() {
                Row::Entry(e) => format!("{} {}", e.level.as_str(), e.message),
                Row::Launch { launch, .. } => format!("START {launch}"),
            })
            .collect()
    }

    #[test]
    fn tail_reads_every_entry_written_in_pieces() {
        let path = std::env::temp_dir().join(format!("tfc-tail-{}.log", std::process::id()));
        let mut file = File::create(&path).unwrap();
        let store = gio::ListStore::new::<BoxedAnyObject>();
        let tail = Tail::new(path.clone());

        writeln!(file, "2026-11-15 09:44:30 c10f52  INFO a::b: App started").unwrap();
        tail.read_into(&store);
        // A line half written when the window reads.
        write!(
            file,
            "2026-11-15 09:44:31 c10f52 DEBUG a::sd: mDNS announce: x.local.:en0"
        )
        .unwrap();
        tail.read_into(&store);
        write!(
            file,
            " (svc)\n2026-11-15 09:44:31 c10f52 DEBUG a::sd: mDNS announce: x.local.:lo0 (svc)\n"
        )
        .unwrap();
        tail.read_into(&store);
        assert_eq!(
            entries(&store),
            [
                "START c10f52",
                "INFO App started",
                "DEBUG mDNS announce: x.local.:en0 (svc)",
                "DEBUG mDNS announce: x.local.:lo0 (svc)"
            ]
        );
        std::fs::remove_file(path).unwrap();
    }
}
