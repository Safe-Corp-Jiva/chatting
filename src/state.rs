use futures_util::stream::SplitSink;
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};

type Tx = SplitSink<WebSocketStream<TcpStream>, Message>;

struct AppState {
    connections: RwLock<HashMap<String, Arc<Mutex<Tx>>>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            connections: RwLock::new(HashMap::new()),
        }
    }

    async fn broadcast(&self, msg: &Message) {
        let connections = self.connections.read().await;
        for tx in connections.values() {
            let mut tx = tx.lock().unwrap();
            let _ = tx.send(msg.clone()).await;
        }
    }

    async fn add_connection(&self, id: String, tx: Arc<Mutex<Tx>>) {
        let mut connections = self.connections.write().await;
        connections.insert(id, tx);
    }

    async fn remove_connection(&self, id: &str) {
        let mut connections = self.connections.write().await;
        connections.remove(id);
    }
}

type TxCM = broadcast::Sender<WsMessage>;
type Rx = broadcast::Receiver<WsMessage>;

struct ChatManager {
    chats: RwLock<HashMap<String, Tx>>,
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
