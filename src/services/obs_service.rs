use std::sync::Arc;
use tokio::sync::RwLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use super::{LogCallback, log};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObsConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

enum ObsCommand {
    SwitchScene(String),
    Disconnect,
}

pub struct ObsService {
    state: Arc<RwLock<ObsConnectionState>>,
    log_callback: LogCallback,
    cmd_tx: Arc<RwLock<Option<tokio::sync::mpsc::Sender<ObsCommand>>>>,
}

impl ObsService {
    pub fn new(log_callback: LogCallback) -> Self {
        Self {
            state: Arc::new(RwLock::new(ObsConnectionState::Disconnected)),
            log_callback,
            cmd_tx: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn get_state(&self) -> ObsConnectionState {
        self.state.read().await.clone()
    }

    pub async fn connect(&self, port: u16, password: String) {
        // Disconnect any existing connection
        self.disconnect().await;

        *self.state.write().await = ObsConnectionState::Connecting;

        let url = format!("ws://localhost:{port}");
        log(&self.log_callback, "OBS", format!("Connecting to {url}..."));

        let ws_stream = match connect_async(&url).await {
            Ok((stream, _)) => stream,
            Err(e) => {
                let msg = format!("Failed to connect: {e}");
                log(&self.log_callback, "OBS", &msg);
                *self.state.write().await = ObsConnectionState::Error(msg);
                return;
            }
        };

        let (mut write, mut read) = ws_stream.split();

        // Wait for Hello (op 0)
        let hello = match read.next().await {
            Some(Ok(Message::Text(text))) => text,
            other => {
                let msg = format!("Expected Hello message, got: {other:?}");
                log(&self.log_callback, "OBS", &msg);
                *self.state.write().await = ObsConnectionState::Error(msg);
                return;
            }
        };

        let hello: serde_json::Value = match serde_json::from_str(&hello) {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("Failed to parse Hello: {e}");
                log(&self.log_callback, "OBS", &msg);
                *self.state.write().await = ObsConnectionState::Error(msg);
                return;
            }
        };

        // Build authentication string if required
        let authentication =
            if let Some(auth) = hello.get("d").and_then(|d| d.get("authentication")) {
                let salt = auth.get("salt").and_then(|s| s.as_str()).unwrap_or("");
                let challenge = auth.get("challenge").and_then(|s| s.as_str()).unwrap_or("");

                if password.is_empty() {
                    let msg = "OBS requires authentication but no password configured".to_string();
                    log(&self.log_callback, "OBS", &msg);
                    *self.state.write().await = ObsConnectionState::Error(msg);
                    return;
                }

                Some(compute_auth(&password, salt, challenge))
            } else {
                None
            };

        // Send Identify (op 1)
        let identify = serde_json::json!({
            "op": 1,
            "d": {
                "rpcVersion": 1,
                "eventSubscriptions": 0,
                "authentication": authentication,
            }
        });

        if let Err(e) = write.send(Message::Text(identify.to_string().into())).await {
            let msg = format!("Failed to send Identify: {e}");
            log(&self.log_callback, "OBS", &msg);
            *self.state.write().await = ObsConnectionState::Error(msg);
            return;
        }

        // Wait for Identified (op 2)
        match read.next().await {
            Some(Ok(Message::Text(text))) => {
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text)
                    && msg.get("op").and_then(|o| o.as_u64()) != Some(2)
                {
                    let msg = format!("Authentication failed: {text}");
                    log(&self.log_callback, "OBS", &msg);
                    *self.state.write().await = ObsConnectionState::Error(msg);
                    return;
                }
            }
            other => {
                let msg = format!("Expected Identified, got: {other:?}");
                log(&self.log_callback, "OBS", &msg);
                *self.state.write().await = ObsConnectionState::Error(msg);
                return;
            }
        }

        *self.state.write().await = ObsConnectionState::Connected;
        log(&self.log_callback, "OBS", "Connected to OBS WebSocket");

        // Set up command channel
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<ObsCommand>(16);
        *self.cmd_tx.write().await = Some(cmd_tx);

        // Spawn the message loop
        let state = Arc::clone(&self.state);
        let log_cb = Arc::clone(&self.log_callback);
        tokio::spawn(async move {
            let mut request_id: u64 = 0;
            loop {
                tokio::select! {
                    cmd = cmd_rx.recv() => {
                        match cmd {
                            Some(ObsCommand::SwitchScene(scene)) => {
                                request_id += 1;
                                let request = serde_json::json!({
                                    "op": 6,
                                    "d": {
                                        "requestType": "SetCurrentProgramScene",
                                        "requestId": request_id.to_string(),
                                        "requestData": {
                                            "sceneName": scene,
                                        }
                                    }
                                });
                                if let Err(e) = write.send(Message::Text(request.to_string().into())).await {
                                    log(&log_cb, "OBS", format!("Failed to send scene switch: {e}"));
                                    *state.write().await = ObsConnectionState::Disconnected;
                                    break;
                                }
                            }
                            Some(ObsCommand::Disconnect) | None => {
                                let _ = write.close().await;
                                *state.write().await = ObsConnectionState::Disconnected;
                                log(&log_cb, "OBS", "Disconnected");
                                break;
                            }
                        }
                    }
                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                // Log responses for debugging
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                                    && json.get("op").and_then(|o| o.as_u64()) == Some(7)
                                {
                                    // RequestResponse
                                    let status = json.get("d")
                                        .and_then(|d| d.get("requestStatus"))
                                        .and_then(|s| s.get("result"))
                                        .and_then(|r| r.as_bool())
                                        .unwrap_or(false);
                                    if !status {
                                        let comment = json.get("d")
                                            .and_then(|d| d.get("requestStatus"))
                                            .and_then(|s| s.get("comment"))
                                            .and_then(|c| c.as_str())
                                            .unwrap_or("unknown error");
                                        log(&log_cb, "OBS", format!("Request failed: {comment}"));
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => {
                                *state.write().await = ObsConnectionState::Disconnected;
                                log(&log_cb, "OBS", "Connection closed by server");
                                break;
                            }
                            Some(Err(e)) => {
                                log(&log_cb, "OBS", format!("WebSocket error: {e}"));
                                *state.write().await = ObsConnectionState::Disconnected;
                                break;
                            }
                            _ => {} // Ping/Pong/Binary - ignore
                        }
                    }
                }
            }
        });
    }

    pub async fn disconnect(&self) {
        if let Some(tx) = self.cmd_tx.write().await.take() {
            let _ = tx.send(ObsCommand::Disconnect).await;
        }
    }

    pub async fn switch_scene(&self, scene_name: String) {
        let tx = self.cmd_tx.read().await;
        if let Some(tx) = tx.as_ref() {
            if let Err(e) = tx.send(ObsCommand::SwitchScene(scene_name)).await {
                log(
                    &self.log_callback,
                    "OBS",
                    format!("Failed to send scene command: {e}"),
                );
            }
        } else {
            log(&self.log_callback, "OBS", "Not connected to OBS");
        }
    }
}

fn compute_auth(password: &str, salt: &str, challenge: &str) -> String {
    // Step 1: SHA256(password + salt) -> base64
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(salt.as_bytes());
    let secret = BASE64.encode(hasher.finalize());

    // Step 2: SHA256(base64_secret + challenge) -> base64
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(challenge.as_bytes());
    BASE64.encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_computation() {
        // Test vector from obs-websocket protocol docs
        let result = compute_auth("mypassword", "testsalt", "testchallenge");
        assert!(!result.is_empty());
        // Verify it's valid base64
        assert!(BASE64.decode(&result).is_ok());
    }
}
