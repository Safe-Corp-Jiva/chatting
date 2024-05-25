use futures_util::stream::SplitSink;
use futures_util::SinkExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use crate::connections::ConnectionType;

type Tx = SplitSink<WebSocketStream<TcpStream>, WsMessage>;

pub struct ConnectionMetadata {
    connection_type: ConnectionType,
    write: Arc<Mutex<Tx>>,
}

impl ConnectionMetadata {
    pub fn new(primary_id: &str, secondary_id: &str, write: Arc<Mutex<Tx>>) -> Self {
        ConnectionMetadata {
            connection_type: ConnectionType::instantiate_connection_type(primary_id, secondary_id),
            write,
        }
    }
}

pub struct AppState {
    connections: RwLock<HashMap<String, ConnectionMetadata>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            connections: RwLock::new(HashMap::new()),
        }
    }

    pub async fn broadcast(&self, msg: &WsMessage) {
        let connections = self.connections.read().await;
        for tx in connections.values() {
            let mut tx_guard = tx.write.lock().await;
            if let Err(e) = tx_guard.send(msg.clone()).await {
                eprintln!("Error broadcasting message: {:?}", e);
            }
        }
    }

    pub async fn add_connection(&self, id: String, connection_metadata: ConnectionMetadata) {
        println!("New connection: {:?}", id);
        let mut connections = self.connections.write().await;
        connections.insert(id, connection_metadata);
    }

    async fn remove_connection(&self, id: &str) {
        let mut connections = self.connections.write().await;
        connections.remove(id);
    }
}

type TxCM = broadcast::Sender<WsMessage>;
type Rx = broadcast::Receiver<WsMessage>;

struct ChatManager {
    chats: RwLock<HashMap<String, TxCM>>,
}

impl ChatManager {
    pub fn new() -> Self {
        ChatManager {
            chats: RwLock::new(HashMap::new()),
        }
    }

    async fn get_or_create_chat(&self, chat_id: String) -> TxCM {
        let mut chats = self.chats.write().await;
        if let Some(tx) = chats.get(&chat_id) {
            tx.clone()
        } else {
            let (tx, _) = broadcast::channel(100);
            chats.insert(chat_id.clone(), tx.clone());
            tx
        }
    }
}
