mod api;
mod overlay_state;
mod service_discovery;
mod settings;
mod table_monitor;
mod web_server;

use std::sync::Arc;

pub type LogCallback = Arc<dyn Fn(String, String) + Send + Sync>;

pub fn log(log: &LogCallback, sender: &str, msg: impl Into<String>) {
    log(sender.to_string(), msg.into());
}

pub use api::KickertoolApiService;
pub use overlay_state::{OverlayMode, OverlayStateManager};
pub use service_discovery::ServiceDiscovery;
pub use settings::{Settings, SettingsService};
pub use table_monitor::TableMonitor;
pub use web_server::WebServer;
