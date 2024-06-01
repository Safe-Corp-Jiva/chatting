use anyhow::{anyhow, Context, Error, Result};
use aws_sdk_dynamodb::Client;
use connections::ConnectionType;
use copilot::CopilotSendMessage;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, Stream, StreamExt};
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
        if let connection_type = ConnectionType::instantiate_connection_type(&stream).await {
            match connection_type {
                ConnectionType::Copilot(primary_id, secondary_id) => {
                    app_state
                        .add_connection(
                            format!("{}-{}", primary_id, secondary_id),
                            ConnectionMetadata::new(connection_type, write).await,
                        )
                        .await;
                    let client = client.clone();
                    let chat_manager = Arc::clone(&chat_manager);
                    let copilot_endpoint = copilot_endpoint.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_copilot_connection(
                            stream,
                            client,
                            chat_manager,
                            copilot_endpoint,
                        )
                        .await
                        {
                            eprintln!("Error handling connection: {:?}", e);
                        }
                    });
                }
                ConnectionType::People(primary_id, secondary_id) => {
                    app_state
                        .add_connection(
                            format!("{}-{}", primary_id, secondary_id),
                            ConnectionMetadata::new(connection_type, stream).await,
                        )
                        .await;
                    println!("Human connection");
                    let client = client.clone();
                    let chat_manager = Arc::clone(&chat_manager);
                    tokio::spawn(async move {
                        if let Err(e) = handle_human_connection(stream, client, chat_manager).await
                        {
                            eprintln!("Error handling connection: {:?}", e);
                        }
                    });
                }
            }
        }
    }
}

async fn handle_human_connection(
    stream: TcpStream,
    client: Client,
    chat_manager: Arc<ChatManager>,
) -> Result<(), Box<Error>> {
    let (primary_id, secondary_id) = init::generate_params_from_url(&stream)
        .await
        .expect("Failed to generate params from URL");
    let chat_id = format!("{}-{}", primary_id, secondary_id);
    let chat = Arc::new(Chat::new(primary_id.clone(), secondary_id.clone()));
    let client = Arc::new(client);

    init::send_chat_to_db(chat.clone(), client.clone())
        .await
        .expect("Failed to send chat to db");

    let ws_stream = accept_async(stream)
        .await
        .context("Failed to accept async stream")?;

    let (write, read) = ws_stream.split();

    let write = Arc::new(Mutex::new(write));
    let read = Arc::new(Mutex::new(read));

    let tx = chat_manager.get_or_create_chat(chat_id.clone()).await;
    let mut rx = tx.subscribe();

    send_initial_messages(&client, &chat, write.clone()).await;

    tokio::spawn(async move {
        if let Err(e) = handle_human_messages::<SplitSink<WebSocketStream<TcpStream>, WsMessage>>(
            read.clone(),
            chat.clone(),
            client.clone(),
            tx,
        )
        .await
        {
            eprintln!("Error handling messages: {:?}", e);
        }
    });

    Ok(())
}

async fn handle_copilot_connection(
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

    send_initial_messages(&client, &chat, write.clone()).await;

    let read_clone = read.clone();
    let chat_clone = chat.clone();
    let client_clone = client.clone();
    let chat_manager_clone = chat_manager.clone();

    tokio::spawn(async move {
        if let Err(e) = handle_copilot_messages::<SplitSink<WebSocketStream<TcpStream>, WsMessage>>(
            read_clone,
            chat_clone,
            client_clone,
            chat_manager_clone,
            copilot_endpoint,
            tx,
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

async fn handle_human_messages<W>(
    read: Arc<
        Mutex<impl Stream<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin>,
    >,
    chat: Arc<Chat>,
    client: Arc<Client>,
    tx: broadcast::Sender<WsMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut read = read.lock().await;
    while let Some(Ok(message)) = read.next().await {
        let message_to_send = serde_json::from_str::<AgentMessage>(&message.to_string())
            .expect("Failed to parse message");
        handle_user_message(&chat, &client, message_to_send.clone()).await;
        let message_json = serde_json::json!(message_to_send).to_string();
        if let Err(e) = tx.send(WsMessage::Text(message_json)) {
            eprintln!("Error broadcasting user message: {:?}", e);
        }
    }
    Ok(())
}

async fn handle_copilot_messages<W>(
    read: Arc<
        Mutex<impl Stream<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin>,
    >,
    chat: Arc<Chat>,
    client: Arc<Client>,
    chat_manager: Arc<ChatManager>,
    copilot_endpoint: String,
    tx: broadcast::Sender<WsMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut read = read.lock().await;
    while let Some(Ok(message)) = read.next().await {
        match serde_json::from_str::<MessageType>(&message.to_string()) {
            Ok(message_type) => {
                if let MessageType::User(ref user_msg) = message_type {
                    handle_user_message(&chat, &client, user_msg.clone()).await;
                    let copilot_send_message =
                        CopilotSendMessage::from_agent_message(user_msg.clone(), chat.clone());
                    send_post_request_and_stream_response(
                        chat.clone(),
                        copilot_send_message,
                        chat_manager.clone(),
                        copilot_endpoint.clone(),
                    );
                    let message_json = serde_json::json!(message_type).to_string();
                    if let Err(e) = tx.send(WsMessage::Text(message_json)) {
                        eprintln!("Error broadcasting user message: {:?}", e);
                    }
                } else {
                    let message_json = serde_json::json!(message_type).to_string();
                    if let Err(e) = tx.send(WsMessage::Text(message_json)) {
                        eprintln!("Error broadcasting copilot message: {:?}", e);
                    }
                }
            }
            Err(e) => eprintln!("Error parsing message: {:?}", e),
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

async fn send_post_request_and_stream_response(
    chat: Arc<Chat>,
    user_msg: CopilotSendMessage,
    chat_manager: Arc<ChatManager>,
    copilot_endpoint: String,
) {
    let url = copilot_endpoint;
    let client = HttpClient::new();
    let response = client
        .post(url)
        .json(&user_msg)
        .send()
        .await
        .expect("Failed to send POST request");
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => {
                let tx = chat_manager
                    .get_or_create_chat(chat.get_chat_id().to_string())
                    .await;
                let message_text =
                    String::from_utf8(chunk.to_vec()).unwrap_or_else(|_| "Binary data".to_string());
                if let Err(e) = tx.send(WsMessage::Text(message_text.clone())) {
                    eprintln!("Error broadcasting raw copilot message: {:?}", e);
                }

                buffer.extend(chunk);

                if buffer.len() > 1024 {
                    match serde_json::from_slice::<CopilotMessage>(&buffer) {
                        Ok(parsed_message) => {
                            println!("Parsed Message: {:?}", parsed_message);
                            buffer.clear();
                        }
                        Err(_) => {}
                    }
                }
            }
            Err(e) => eprintln!("Error reading stream response: {:?}", e),
        }
    }
}
