use anyhow::{anyhow, Context, Error, Result};
use aws_sdk_dynamodb::Client;
use connections::ConnectionType;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client as HttpClient;
use state::ConnectionMetadata;
use std::collections::HashMap;
use std::env;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};

mod agents;
mod connections;
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
    dotenv::dotenv().ok();
    let region_provider =
        aws_config::meta::region::RegionProviderChain::default_provider().or_else("us-west-2");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);
    let chat_manager = Arc::new(ChatManager::new());

    let copilot_endpoint =
        env::var("COPILOT_ENDPOINT").expect("Could not load Copilot endpoint 🤖");

    let addr = "0.0.0.0:3030";
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind 🤞");

    let app_state = Arc::new(state::AppState::new());

    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        let client = client.clone();
        let chat_manager = chat_manager.clone();
        let copilot_endpoint_clone = copilot_endpoint.clone();
        let app_state = app_state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(
                app_state,
                stream,
                client,
                chat_manager,
                copilot_endpoint_clone,
            )
            .await
            {
                eprintln!("Error handling connection: {:?}", e);
            }
        });
    }
}

async fn handle_connection(
    app_state: Arc<state::AppState>,
    stream: TcpStream,
    client: Client,
    chat_manager: Arc<ChatManager>,
    copilot_endpoint: String,
) -> Result<(), Box<Error>> {
    let (agent_id, secondary_id) = match init::generate_params_from_url(&stream).await {
        Ok((agent_id, secondary_id)) => (agent_id, secondary_id),
        Err(e) => {
            eprintln!("Error parsing URL: {:?}", e);
            return Err(Box::new(anyhow!(e.to_string())));
        }
    };

    let chat_id = format!("{}-{}", agent_id, secondary_id);
    let chat = Arc::new(Chat::new(agent_id.clone(), secondary_id.clone()));
    let client = Arc::new(client);

    init::send_chat_to_db(chat.clone(), client.clone())
        .await
        .context("Failed to send chat to db")?;

    let ws_stream = accept_async(stream)
        .await
        .context("Failed to accept async stream")?;
    let (write, read) = ws_stream.split();
    let write = Arc::new(Mutex::new(write));
    let read = Arc::new(Mutex::new(read));

    let tx = chat_manager.get_or_create_chat(chat_id.clone()).await;
    let mut rx = tx.subscribe();

    app_state
        .add_connection(
            agent_id.clone(),
            ConnectionMetadata::new(&agent_id.clone(), &secondary_id.clone(), write.clone()),
        )
        .await;

    send_initial_messages(&client, &chat, write.clone()).await;

    let read_clone = read.clone();
    let chat_clone = chat.clone();
    let client_clone = client.clone();
    let chat_manager_clone = chat_manager.clone();

    let copilot_messages = Arc::new(Mutex::new(Vec::new()));

    tokio::spawn(async move {
        if let Err(e) = handle_messages::<_, SplitSink<WebSocketStream<TcpStream>, WsMessage>>(
            read_clone,
            chat_clone,
            client_clone,
            chat_manager_clone,
            copilot_messages,
            tx,
            copilot_endpoint,
        )
        .await
        {
            eprintln!("Error handling messages: {:?}", e);
        }
    });

    tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            let mut write = write.lock().await;
            if let Err(e) = write.send(message.clone()).await {
                eprintln!("Error sending message to client: {:?}", e);
            }
        }
    });

    Ok(())
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
        if let Err(e) = write.send(WsMessage::Text(message_json)).await {
            eprintln!("Error sending initial message: {:?}", e);
        }
    }
}

async fn handle_messages<R, W>(
    read: Arc<Mutex<R>>,
    chat: Arc<Chat>,
    client: Arc<Client>,
    chat_manager: Arc<ChatManager>,
    copilot_messages: Arc<Mutex<Vec<CopilotMessage>>>,
    tx: broadcast::Sender<WsMessage>,
    copilot_endpoint: String,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: futures_util::Stream<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
    W: futures_util::Sink<WsMessage> + Unpin + Send + 'static,
    <W as futures_util::Sink<WsMessage>>::Error: Debug,
{
    let mut read = read.lock().await;

    while let Some(Ok(message)) = read.next().await {
        if let WsMessage::Text(text) = message {
            match serde_json::from_str::<MessageType>(&text) {
                Ok(message_type) => match message_type {
                    MessageType::User(ref user_msg) => {
                        handle_user_message(&chat, &client, user_msg.clone()).await;
                        let message_json = serde_json::json!(message_type).to_string();
                        if let Err(e) = tx.send(WsMessage::Text(message_json)) {
                            eprintln!("Error broadcasting user message: {:?}", e);
                        }
                        send_post_request_and_stream_response(
                            chat.clone(),
                            user_msg.clone(),
                            chat_manager.clone(),
                        )
                        .await;
                    }
                    MessageType::Copilot(ref copilot_msg) => {
                        handle_copilot_message(
                            copilot_msg.clone(),
                            &copilot_messages,
                            &chat,
                            &client,
                        )
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

    Ok(())
}

async fn handle_user_message(chat: &Arc<Chat>, client: &Arc<Client>, user_msg: AgentMessage) {
    if let Err(e) = chat
        .send_message_to_db(client, MessageType::User(user_msg))
        .await
    {
        eprintln!("Failed to send user message to db: {:?}", e);
    }
}

async fn handle_copilot_message(
    copilot_msg: CopilotMessage,
    copilot_messages: &Arc<Mutex<Vec<CopilotMessage>>>,
    chat: &Arc<Chat>,
    client: &Arc<Client>,
) {
    let mut messages = copilot_messages.lock().await;
    messages.push(copilot_msg);

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

        messages.sort_by_key(|msg| msg.get_timestamp());

        let combined_message = messages
            .drain(..)
            .map(|msg| msg.get_message().to_string())
            .collect::<Vec<_>>()
            .join("");

        combined_message
    };

    let combined_copilot_message = CopilotMessage::new("Combined".to_string(), combined_message);

    if let Err(e) = chat
        .send_message_to_db(&client, MessageType::Copilot(combined_copilot_message))
        .await
    {
        eprintln!("Failed to send copilot message to db: {:?}", e);
    }
}

async fn send_post_request_and_stream_response(
    chat: Arc<Chat>,
    user_msg: AgentMessage,
    chat_manager: Arc<ChatManager>,
) {
    // Define the URL and create an HTTP client
    let url = "http://your-copilot-service/endpoint";
    let client = HttpClient::new();

    // Send POST request
    let response = client
        .post(url)
        .json(&user_msg)
        .send()
        .await
        .expect("Failed to send POST request");

    // Handle the streaming response
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => {
                let copilot_message: CopilotMessage =
                    serde_json::from_slice(&chunk).expect("Failed to parse CopilotMessage");

                // Here you would broadcast the message to WebSocket clients
                // For example, using a broadcast channel in the ChatManager

                let tx = chat_manager
                    .get_or_create_chat(chat.get_chat_id().to_string())
                    .await;
                let message_json =
                    serde_json::json!(MessageType::Copilot(copilot_message)).to_string();
                if let Err(e) = tx.send(WsMessage::Text(message_json)) {
                    eprintln!(
                        "Error broadcasting copilot message from POST response: {:?}",
                        e
                    );
                }
            }
            Err(e) => eprintln!("Error reading stream response: {:?}", e),
        }
    }
}
