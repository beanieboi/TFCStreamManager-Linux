use gtk4::ApplicationWindow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::DebugWindow;
use crate::services::LogCallback;

pub struct DebugLog {
    window: Rc<RefCell<Option<DebugWindow>>>,
    history: Arc<Mutex<Vec<String>>>,
    pending: Arc<Mutex<VecDeque<String>>>,
    callback: LogCallback,
}

impl DebugLog {
    pub fn new() -> Self {
        let window = Rc::new(RefCell::new(None));
        let history = Arc::new(Mutex::new(Vec::new()));
        let pending = Arc::new(Mutex::new(VecDeque::new()));
        let callback = {
            let history = Arc::clone(&history);
            let pending = Arc::clone(&pending);
            Arc::new(move |sender: String, msg: String| {
                let line = format!("[{}] {}", sender, msg);
                if let Ok(mut history) = history.lock() {
                    history.push(line.clone());
                }
                if let Ok(mut pending) = pending.lock() {
                    pending.push_back(line);
                }
            })
        };

        Self {
            window,
            history,
            pending,
            callback,
        }
    }

    pub fn install(&self) {
        let window = Rc::clone(&self.window);
        let pending = Arc::clone(&self.pending);
        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            let mut drained = Vec::new();
            if let Ok(mut pending) = pending.lock() {
                while let Some(line) = pending.pop_front() {
                    drained.push(line);
                }
            }
            if !drained.is_empty()
                && let Some(ref debug) = *window.borrow()
            {
                for line in drained {
                    debug.append_text(&line);
                }
            }
            glib::ControlFlow::Continue
        });
    }

    pub fn callback(&self) -> LogCallback {
        Arc::clone(&self.callback)
    }

    pub fn show(&self, parent: &ApplicationWindow) {
        let mut dw = self.window.borrow_mut();
        if dw.is_none() {
            *dw = Some(DebugWindow::new(parent));
            if let Some(ref debug) = *dw
                && let Ok(history) = self.history.lock()
            {
                for line in history.iter() {
                    debug.append_text(line);
                }
            }
        }
        if let Some(ref debug) = *dw {
            debug.show();
        }
    }
}
