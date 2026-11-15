//! Opens the debug window on demand: a new one each time, or the one already
//! open. It tails the app log file, which `services::logging` writes whether
//! a window is open or not.

use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Window};
use std::cell::RefCell;
use std::path::PathBuf;

use super::DebugWindow;

pub struct DebugLog {
    path: PathBuf,
    open: RefCell<glib::WeakRef<Window>>,
}

impl DebugLog {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            open: RefCell::new(glib::WeakRef::new()),
        }
    }

    pub fn show(&self, parent: &ApplicationWindow) {
        if let Some(window) = self.open.borrow().upgrade() {
            window.present();
            return;
        }
        let window = DebugWindow::open(parent, self.path.clone());
        self.open.borrow_mut().set(Some(&window));
    }
}
