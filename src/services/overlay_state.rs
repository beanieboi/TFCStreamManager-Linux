use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use super::Settings;
use crate::models::{Match, OverlayContent, Table, Tournament};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayMode {
    Empty,
    Kickertool,
    Remote,
    Manual,
}

impl std::fmt::Display for OverlayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverlayMode::Empty => write!(f, "Empty"),
            OverlayMode::Kickertool => write!(f, "Kickertool"),
            OverlayMode::Remote => write!(f, "Remote"),
            OverlayMode::Manual => write!(f, "Manual"),
        }
    }
}

pub struct OverlayStateManager {
    content: Arc<RwLock<OverlayContent>>,
    mode: Arc<RwLock<OverlayMode>>,
    content_changed_tx: broadcast::Sender<()>,
}

impl OverlayStateManager {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            content: Arc::new(RwLock::new(OverlayContent::empty())),
            mode: Arc::new(RwLock::new(OverlayMode::Empty)),
            content_changed_tx: tx,
        }
    }

    pub async fn get_content(&self) -> OverlayContent {
        self.content.read().await.clone()
    }

    pub async fn set_content(&self, content: OverlayContent) {
        let mut current = self.content.write().await;
        *current = content;
        let _ = self.content_changed_tx.send(());
    }

    pub async fn get_mode(&self) -> OverlayMode {
        self.mode.read().await.clone()
    }

    pub async fn set_mode(&self, mode: OverlayMode) {
        let mut current = self.mode.write().await;
        *current = mode;
    }

    pub async fn update_from_match(
        &self,
        match_data: &Match,
        table: &Table,
        tournament: &Tournament,
        settings: &Settings,
    ) {
        let content = OverlayContent::from_match(match_data, table, tournament, settings);
        self.set_content(content).await;
    }

    pub async fn reset(&self) {
        self.set_content(OverlayContent::empty()).await;
    }
}

impl Default for OverlayStateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for OverlayStateManager {
    fn clone(&self) -> Self {
        Self {
            content: Arc::clone(&self.content),
            mode: Arc::clone(&self.mode),
            content_changed_tx: self.content_changed_tx.clone(),
        }
    }
}
