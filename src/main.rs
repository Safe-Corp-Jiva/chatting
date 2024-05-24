use aws_sdk_dynamodb::Client;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};

mod agents;
mod copilot;
mod init;
mod messages;
mod state;

use crate::agents::AgentMessage;
use crate::copilot::CopilotMessage;
use crate::messages::Chat;
use crate::messages::MessageType;

const TIMEOUT_DURATION: std::time::Duration = std::time::Duration::from_secs(5);

struct ChatManager {
    chats: RwLock<HashMap<String, broadcast::Sender<WsMessage>>>,
}

impl ChatManager {
    pub fn new() -> Self {
        ChatManager {
            chats: RwLock::new(HashMap::new()),
        }
    }

    async fn get_or_create_chat(&self, chat_id: String) -> broadcast::Sender<WsMessage> {
        let mut chats = self.chats.write().await;
        if let Some(tx) = chats.get(&chat_id) {
            tx.clone()
        } else {
            let (tx, _) = broadcast::channel(100);
            chats.insert(chat_id, tx.clone());
            tx
        }
    }
}

#[tokio::main]
async fn main() {
    let region_provider =
        aws_config::meta::region::RegionProviderChain::default_provider().or_else("us-west-2");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);
    let chat_manager = Arc::new(ChatManager::new());

    let addr = "0.0.0.0:3030";
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");

    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        let client = client.clone();
        let chat_manager = chat_manager.clone();
        tokio::spawn(async move {
            handle_connection(stream, client, chat_manager).await;
        });
    }
}

async fn handle_connection(stream: TcpStream, client: Client, chat_manager: Arc<ChatManager>) {
    let (agent_id, secondary_id) = match init::generate_params_from_url(&stream).await {
        Ok((agent_id, secondary_id)) => (agent_id, secondary_id),
        Err(e) => {
            eprintln!("Error parsing URL: {:?}", e);
            ("Err".to_string(), e.to_string())
        }
    };

    let chat_id = format!("{}-{}", agent_id, secondary_id);
    let chat = Arc::new(Chat::new(agent_id.clone(), secondary_id));
    let client = Arc::new(client);

    init::send_chat_to_db(chat.clone(), client.clone())
        .await
        .expect("Failed to send chat to database");

    let ws_stream = accept_async(stream)
        .await
        .expect("Failed to accept WebSocket connection");

    let (write, read) = ws_stream.split();
    let write = Arc::new(Mutex::new(write));
    let read = Arc::new(Mutex::new(read));

    // Get or create the broadcast channel for the chat
    let tx = chat_manager.get_or_create_chat(chat_id.clone()).await;
    let mut rx = tx.subscribe();

    send_initial_messages(&client, &chat, write.clone()).await;

    let read_clone = read.clone();
    let chat_clone = chat.clone();
    let client_clone = client.clone();
    let chat_manager_clone = chat_manager.clone();

    let copilot_messages = Arc::new(Mutex::new(Vec::new()));

    tokio::spawn(async move {
        handle_messages(
            read_clone,
            chat_clone,
            client_clone,
            chat_manager_clone,
            copilot_messages,
            tx,
        )
        .await;
    });

    tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            let mut write = write.lock().await;
            if let Err(e) = write.send(message.clone()).await {
                eprintln!("Error sending message to client: {:?}", e);
            }
        }
    });
}

async fn send_initial_messages<S>(client: &Arc<Client>, chat: &Arc<Chat>, write: Arc<Mutex<S>>)
where
    S: futures_util::Sink<WsMessage> + Unpin,
    <S as futures_util::Sink<WsMessage>>::Error: Debug,
{
    let initial_messages = init::get_messages_from_db(client, chat.get_chat_id().to_string())
        .await
        .expect("Failed to get messages from db");

    for message in initial_messages {
        let message_json = serde_json::json!(message).to_string();
        let mut write = write.lock().await;
        write
            .send(WsMessage::Text(message_json))
            .await
            .expect("Failed to send message to client");
    }
}

async fn handle_messages<S>(
    read: Arc<
        Mutex<
            impl StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
        >,
    >,
    chat: Arc<Chat>,
    client: Arc<Client>,
    chat_manager: Arc<ChatManager>,
    copilot_messages: Arc<Mutex<Vec<CopilotMessage>>>,
    tx: broadcast::Sender<WsMessage>,
) where
    S: futures_util::Sink<WsMessage> + Unpin,
    <S as futures_util::Sink<WsMessage>>::Error: Debug,
{
    let mut read = read.lock().await;

    while let Some(Ok(message)) = read.next().await {
        if let WsMessage::Text(text) = message {
            match serde_json::from_str::<MessageType>(&text) {
                Ok(message_type) => match message_type {
                    MessageType::User(user_msg) => {
                        handle_user_message(&chat, &client, user_msg).await;
                        let message_json = serde_json::json!(message_type).to_string();
                        if let Err(e) = tx.send(WsMessage::Text(message_json)) {
                            eprintln!("Error broadcasting user message: {:?}", e);
                        }
                    }
                    MessageType::Copilot(copilot_msg) => {
                        handle_copilot_message(copilot_msg, &copilot_messages, &chat, &client)
                            .await;
                        let message_json = serde_json::json!(message_type).to_string();
                        if let Err(e) = tx.send(WsMessage::Text(message_json)) {
                            eprintln!("Error broadcasting copilot message: {:?}", e);
                        }
                    }
                },
                Err(e) => eprintln!("Error parsing message: {:?}", e),
            }
        }
    }
}

async fn handle_user_message(chat: &Arc<Chat>, client: &Arc<Client>, user_msg: AgentMessage) {
    chat.send_message_to_db(client, MessageType::User(user_msg))
        .await
        .expect("Failed to send user message to db");
}

async fn handle_copilot_message(
    copilot_msg: CopilotMessage,
    copilot_messages: &Arc<Mutex<Vec<CopilotMessage>>>,
    chat: &Arc<Chat>,
    client: &Arc<Client>,
) {
    // Accumulate copilot messages
    let mut messages = copilot_messages.lock().await;
    messages.push(copilot_msg);

    // Spawn a task to process copilot messages after a timeout
    let copilot_messages_clone = Arc::clone(&copilot_messages);
    let chat_clone = Arc::clone(&chat);
    let client_clone = Arc::clone(&client);

    tokio::spawn(async move {
        tokio::time::sleep(TIMEOUT_DURATION).await;
        process_copilot_messages(copilot_messages_clone, chat_clone, client_clone).await;
    });
}

async fn process_copilot_messages(
    copilot_messages: Arc<Mutex<Vec<CopilotMessage>>>,
    chat: Arc<Chat>,
    client: Arc<Client>,
) {
    let combined_message = {
        let mut messages = copilot_messages.lock().await;
        if messages.is_empty() {
            return;
        }

        // Sort messages by timestamp
        messages.sort_by_key(|msg| msg.timestamp);

        let combined_message = messages
            .drain(..)
            .map(|msg| msg.get_message().clone()) // Clone the string to resolve ownership issues
            .collect::<Vec<_>>()
            .join("");

        combined_message
    };

    let combined_copilot_message = CopilotMessage::new("Combined".to_string(), combined_message);

    chat.send_message_to_db(&client, MessageType::Copilot(combined_copilot_message))
        .await
        .expect("Failed to send copilot message to db");
}
