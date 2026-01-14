use gtk4::prelude::*;
use gtk4::{ApplicationWindow, ScrolledWindow, TextBuffer, TextView};

pub struct DebugWindow {
    window: ApplicationWindow,
    text_buffer: TextBuffer,
}

impl DebugWindow {
    pub fn new(parent: &ApplicationWindow) -> Self {
        let window = ApplicationWindow::builder()
            .title("Debug Log")
            .default_width(600)
            .default_height(400)
            .transient_for(parent)
            .build();

        let text_view = TextView::builder().editable(false).monospace(true).build();

        let text_buffer = text_view.buffer();

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .child(&text_view)
            .build();

        window.set_child(Some(&scrolled));

        Self {
            window,
            text_buffer,
        }
    }

    pub fn append_text(&self, text: &str) {
        let mut end_iter = self.text_buffer.end_iter();
        self.text_buffer
            .insert(&mut end_iter, &format!("{}\n", text));
    }

    pub fn show(&self) {
        self.window.present();
    }
}
