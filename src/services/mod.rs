mod api;
pub mod logging;
mod match_api;
pub(crate) mod match_store;
mod overlay_feed;
pub(crate) mod overlays;
mod service_discovery;
mod settings;
mod table_monitor;
pub(crate) mod trackers;
mod web_server;

pub use api::{ApiError, KickertoolApiService};
pub use match_api::MatchServices;
pub use match_store::MatchStore;
pub use overlay_feed::{OverlayFeed, overlay_key};
pub use overlays::{
    OverlayLibrary, Template as OverlayTemplate, TemplateFormat, select as select_overlay,
};
pub use service_discovery::{ServerState, ServiceDiscovery};
pub use settings::{CompetitionKind, FreiKind, Settings, SettingsService};
pub use table_monitor::{TableMonitor, is_kickertool_tournament};
pub use trackers::{TrackerStatus, Trackers};
pub use web_server::WebServer;
