use axum::{
    extract::{ws::{Message, WebSocket}, WebSocketUpgrade},
    response::Response,
};
use tokio::sync::broadcast;
use tracing::info;

pub async fn websocket_handler(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    info!("WebSocket connection established");
    
    while let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            match msg {
                Message::Text(text) => {
                    info!("Received text: {}", text);
                    if let Err(e) = socket.send(Message::Text(format!("Echo: {}", text))).await {
                        info!("WebSocket send error: {}", e);
                        break;
                    }
                }
                Message::Close(_) => {
                    info!("WebSocket connection closed");
                    break;
                }
                _ => {}
            }
        } else {
            info!("WebSocket receive error");
            break;
        }
    }
}