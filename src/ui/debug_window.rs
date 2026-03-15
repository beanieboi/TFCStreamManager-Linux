use gtk4::prelude::*;
use gtk4::{ApplicationWindow, ScrolledWindow, TextBuffer, TextView, Window};

pub struct DebugWindow {
    window: Window,
    text_buffer: TextBuffer,
}

impl DebugWindow {
    pub fn new(parent: &ApplicationWindow) -> Self {
        let window = Window::builder()
            .title("Debug Log")
            .default_width(600)
            .default_height(400)
            .destroy_with_parent(true)
            .build();

        window.set_display(&WidgetExt::display(parent));

        let text_view = TextView::builder().editable(false).monospace(true).build();

        let text_buffer = text_view.buffer();

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .child(&text_view)
            .build();

        window.set_child(Some(&scrolled));

        window.connect_close_request(|win| {
            win.set_visible(false);
            glib::Propagation::Stop
        });

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
