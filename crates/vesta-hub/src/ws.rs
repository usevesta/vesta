//! `GET /api/v1/ws`: one session per client.
//!
//! - On connect the client receives a `snapshot`, then `state`, `event`,
//!   `availability` and `dashboard_changed` updates as they happen.
//! - If the session falls behind the bus (`RecvError::Lagged`), it sends a
//!   fresh `snapshot` instead of the missed updates.
//! - Every `command` gets exactly one `command_result`. Commands run
//!   concurrently, so results may arrive in any order and either before or
//!   after the `state` they cause.
//! - Malformed messages without a usable `request_id` are dropped (logged at
//!   `debug`); the connection stays open.

use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::response::Response;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use tokio::task::JoinSet;
use tokio::time::{Instant, interval_at};
use vesta_core::{
    BusRecv, ClientMessage, CommandError, CommandRequest, CommandResult, ServerMessage,
};

use crate::api::AppState;
use crate::dashboard;

/// Commands are tiny; anything bigger is not a VESTA client.
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Per-connection commands in flight. Beyond this the session stops reading
/// until one finishes (backpressure, not an error).
const MAX_IN_FLIGHT: usize = 32;
/// `request_id` limit from the spec; longer ids are malformed.
const MAX_REQUEST_ID_CHARS: usize = 64;
/// Keeps NAT/Wi-Fi paths warm and detects dead peers.
const PING_EVERY: Duration = Duration::from_secs(25);

pub(crate) async fn upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_failed_upgrade(|error| tracing::debug!(%error, "websocket upgrade failed"))
        .on_upgrade(move |socket| async move {
            let mut commands = JoinSet::new();
            if let Err(error) = session(socket, state, &mut commands).await {
                tracing::debug!(%error, "websocket session ended with an error");
            }
            // Commands already sent run to completion even if the client
            // went away (e.g. a Wi-Fi handoff right after tapping a button).
            commands.detach_all();
        })
}

type Sender = SplitSink<WebSocket, Message>;

async fn send(tx: &mut Sender, message: &ServerMessage) -> anyhow::Result<()> {
    let json = serde_json::to_string(message)?;
    tx.send(Message::Text(json.into())).await?;
    Ok(())
}

async fn session(
    socket: WebSocket,
    state: AppState,
    commands: &mut JoinSet<CommandResult>,
) -> anyhow::Result<()> {
    let AppState {
        runtime,
        dashboards,
        mut shutdown,
        ..
    } = state;
    let (mut tx, mut rx) = socket.split();

    // Subscribe before the snapshot so no update falls in between.
    let mut updates = runtime.subscribe();
    let mut dashboard_changes = dashboards.subscribe();
    let mut dashboard_open = true;
    send(&mut tx, &ServerMessage::Snapshot(runtime.snapshot())).await?;

    let mut keepalive = interval_at(Instant::now() + PING_EVERY, PING_EVERY);
    loop {
        tokio::select! {
            incoming = rx.next(), if commands.len() < MAX_IN_FLIGHT => match incoming {
                Some(Ok(Message::Text(text))) => match parse(text.as_str()) {
                    Incoming::Command(request) => {
                        let runtime = runtime.clone();
                        commands.spawn(async move {
                            let result = runtime.execute(&request).await;
                            CommandResult::new(request.request_id, result)
                        });
                    }
                    Incoming::Invalid(request_id) => {
                        let result = CommandResult::failed(request_id, CommandError::Unsupported);
                        send(&mut tx, &ServerMessage::CommandResult(result)).await?;
                    }
                    Incoming::Malformed => tracing::debug!("dropped malformed client message"),
                },
                Some(Ok(Message::Binary(_))) => tracing::debug!("dropped binary client message"),
                // Pongs are answered by the WebSocket layer.
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Err(error)) => return Err(error.into()),
            },
            update = updates.recv() => match update {
                Some(BusRecv::Event(update)) => send(&mut tx, &ServerMessage::from(update)).await?,
                Some(BusRecv::Resync) => {
                    tracing::debug!("client fell behind; sending a fresh snapshot");
                    send(&mut tx, &ServerMessage::Snapshot(runtime.snapshot())).await?;
                }
                None => return Ok(()),
            },
            changed = dashboard_changes.changed(), if dashboard_open => match changed {
                Ok(()) => {
                    let message = dashboard::changed_message(&dashboard_changes.borrow_and_update());
                    send(&mut tx, &ServerMessage::from(message)).await?;
                }
                // The dashboard service is gone (shutting down).
                Err(_) => dashboard_open = false,
            },
            Some(done) = commands.join_next() => match done {
                Ok(result) => send(&mut tx, &ServerMessage::CommandResult(result)).await?,
                Err(error) => tracing::warn!(%error, "command task failed"),
            },
            _ = keepalive.tick() => tx.send(Message::Ping(Default::default())).await?,
            _ = shutdown.changed() => {
                let close = CloseFrame {
                    code: close_code::AWAY,
                    reason: "hub shutting down".into(),
                };
                // Best effort: the client reconnects either way.
                let _ = tx.send(Message::Close(Some(close))).await;
                return Ok(());
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum Incoming {
    Command(CommandRequest),
    /// A command the Hub cannot understand (unknown capability, value out
    /// of range, bad target) but can answer, so the client's pending state
    /// resolves now instead of timing out.
    Invalid(String),
    /// Nothing usable to answer to.
    Malformed,
}

fn usable_request_id(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= MAX_REQUEST_ID_CHARS
}

fn parse(text: &str) -> Incoming {
    match serde_json::from_str::<ClientMessage>(text) {
        Ok(ClientMessage::Command(request)) if usable_request_id(&request.request_id) => {
            Incoming::Command(request)
        }
        Ok(ClientMessage::Command(_)) => Incoming::Malformed,
        Err(_) => {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
                return Incoming::Malformed;
            };
            let is_command =
                value.get("type").and_then(serde_json::Value::as_str) == Some("command");
            match value.get("request_id").and_then(serde_json::Value::as_str) {
                Some(id) if is_command && usable_request_id(id) => Incoming::Invalid(id.to_owned()),
                _ => Incoming::Malformed,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse_json(value: serde_json::Value) -> Incoming {
        parse(&value.to_string())
    }

    #[test]
    fn parses_commands() {
        let incoming = parse_json(json!({
            "type": "command",
            "request_id": "c-1",
            "target": "living_room_fan/light",
            "command": { "capability": "level", "action": "set", "level": 50 }
        }));
        match incoming {
            Incoming::Command(request) => {
                assert_eq!(request.request_id, "c-1");
                assert!(!request.confirmed);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn answers_invalid_commands_that_have_a_request_id() {
        for bad in [
            json!({ "type": "command", "request_id": "c-2", "target": "living_room_fan/light",
                    "command": { "capability": "level", "action": "set", "level": 150 } }),
            json!({ "type": "command", "request_id": "c-2", "target": "Not A Target",
                    "command": { "capability": "on_off", "action": "on" } }),
            json!({ "type": "command", "request_id": "c-2", "target": "a/b",
                    "command": { "capability": "teleport", "action": "go" } }),
        ] {
            assert_eq!(parse_json(bad), Incoming::Invalid("c-2".to_owned()));
        }
    }

    #[test]
    fn drops_messages_without_a_usable_request_id() {
        let long_id = "x".repeat(MAX_REQUEST_ID_CHARS + 1);
        for bad in [
            "not json".to_owned(),
            json!({ "type": "hello" }).to_string(),
            json!({ "type": "command" }).to_string(),
            json!({ "type": "command", "request_id": 42 }).to_string(),
            json!({ "type": "subscribe", "request_id": "c-3" }).to_string(),
            json!({ "type": "command", "request_id": "", "target": "a/main",
                    "command": { "capability": "on_off", "action": "on" } })
            .to_string(),
            json!({ "type": "command", "request_id": long_id, "target": "a/main",
                    "command": { "capability": "on_off", "action": "on" } })
            .to_string(),
        ] {
            assert_eq!(parse(&bad), Incoming::Malformed, "{bad}");
        }
        let max_id = "é".repeat(MAX_REQUEST_ID_CHARS);
        assert!(matches!(
            parse_json(
                json!({ "type": "command", "request_id": max_id, "target": "a/main",
                               "command": { "capability": "on_off", "action": "on" } })
            ),
            Incoming::Command(_)
        ));
    }
}
