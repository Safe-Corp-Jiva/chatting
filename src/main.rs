use anyhow::{anyhow, Context, Error, Result};
use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::Client;
use connections::ConnectionType;
use copilot::{CopilotChunk, CopilotSendMessage};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client as HttpClient;
use serde_json::{json, Value};
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

use crate::agents::AgentMessage;
use crate::copilot::CopilotMessage;
use crate::messages::Chat;
use crate::messages::MessageType;

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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = Client::new(&config);
    let chat_manager = Arc::new(ChatManager::new());

    let copilot_endpoint = env::var("COPILOT_ENDPOINT")?;

    println!("Copilot endpoint: {:?}", copilot_endpoint);

    let addr = "0.0.0.0:3030";
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind 🤞");

    println!("Listening on: {}", addr);
    while let Ok((stream, addr)) = listener.accept().await {
        let client = client.clone();
        let chat_manager = chat_manager.clone();
        let copilot_endpoint = copilot_endpoint.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, client, chat_manager, copilot_endpoint).await
            {
                eprintln!("Error handling connection: {:?} on {:?}", e, addr);
            }
        });
    }

    Ok(())
}

async fn handle_connection(
    stream: TcpStream,
    client: Client,
    chat_manager: Arc<ChatManager>,
    copilot_endpoint: String,
) -> Result<(), Box<Error>> {
    match ConnectionType::instantiate_connection_type(&stream).await {
        Ok(ConnectionType::Copilot(_, _)) => {
            handle_copilot_connection(stream, client, chat_manager, copilot_endpoint).await
        }
        Ok(ConnectionType::People(_, _)) => {
            handle_human_connection(stream, client, chat_manager).await
        }
        Err(e) => Err(Box::new(anyhow!(e.to_string()))),
    }
}

async fn handle_human_connection(
    stream: TcpStream,
    client: Client,
    chat_manager: Arc<ChatManager>,
) -> Result<(), Box<Error>> {
    println!("Handling human connection");
    let (primary_id, secondary_id) = init::generate_params_from_url(&stream)
        .await
        .expect("Failed to generate params from URL");
    let chat_id = format!("{}-{}", primary_id, secondary_id);
    let mut chat = Chat::new(primary_id.clone(), secondary_id.clone());
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
            &mut chat,
            client.clone(),
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
    let mut chat = Chat::new(agent_id, secondary_id);
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
    let client_clone = client.clone();
    let chat_manager_clone = chat_manager.clone();

    tokio::spawn(async move {
        if let Err(e) = handle_copilot_messages::<SplitSink<WebSocketStream<TcpStream>, WsMessage>>(
            read_clone,
            &mut chat,
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

async fn send_initial_messages<S>(client: &Arc<Client>, chat: &Chat, write: Arc<Mutex<S>>)
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
    chat: &mut Chat,
    client: Arc<Client>,
    tx: broadcast::Sender<WsMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut read = read.lock().await;
    while let Some(Ok(message)) = read.next().await {
        match serde_json::from_str::<AgentMessage>(&message.to_string()) {
            Ok(message) => {
                let client = client.clone();
                handle_user_message(chat, &client, message.clone()).await?;
                let message_json = serde_json::json!(message).to_string();
                if let Err(e) = tx.send(WsMessage::Text(message_json)) {
                    eprintln!("Error broadcasting user message: {:?}", e);
                } else {
                    // Send notification that message was received
                    let primary_id = chat.get_agent_id().to_string();
                    let secondary_id = chat.get_secondary_id().to_string();
                    let notification_type = "HUMAN";
                    match send_notification(primary_id, secondary_id, notification_type.to_string())
                        .await
                    {
                        Ok(_) => println!("Notification sent"),
                        Err(e) => eprintln!("Error sending notification: {:?}", e),
                    }
                }
            }
            Err(e) => {
                let error_json = serde_json::json!({
                    "error": "Failed to parse message"
                });
                let _ = tx.send(WsMessage::Text(error_json.to_string()));
            }
        }
    }
    Ok(())
}

async fn handle_copilot_messages<W>(
    read: Arc<
        Mutex<impl Stream<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin>,
    >,
    chat: &mut Chat,
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
                    let client = client.clone();
                    handle_user_message(chat, &client, user_msg.clone()).await?;
                    let copilot_send_message =
                        CopilotSendMessage::from_agent_message(user_msg.clone(), chat);
                    println!("Sending message to copilot: {:?}", copilot_send_message);
                    send_post_request_and_stream_response(
                        chat,
                        copilot_send_message,
                        chat_manager.clone(),
                        copilot_endpoint.clone(),
                        client,
                    )
                    .await?;
                    let message_json = serde_json::json!(message_type).to_string();
                    if let Err(e) = tx.send(WsMessage::Text(message_json)) {
                        eprintln!("Error broadcasting user message: {:?}", e);
                    }
                } else {
                    let message_json = serde_json::json!(message_type).to_string();
                    if let Err(e) = tx.send(WsMessage::Text(message_json)) {
                        let error_json = serde_json::json!({
                            "error": "Failed to parse message"
                        });
                        let _ = tx.send(WsMessage::Text(error_json.to_string()));
                    }
                }
            }
            Err(e) => eprintln!("Error parsing message: {:?}", e),
        }
    }
    Ok(())
}
async fn handle_user_message(
    chat: &mut Chat,
    client: &Arc<Client>,
    user_msg: AgentMessage,
) -> Result<(), Box<dyn std::error::Error>> {
    // Send message to database
    if let Err(e) = chat
        .send_message_to_db(client, MessageType::User(user_msg.clone()))
        .await
    {
        eprintln!("Failed to send user message to db: {:?}", e);
    } else {
        chat.add_message(MessageType::User(user_msg));
    }

    Ok(())
}

async fn send_notification(
    primary_id: String,
    secondary_id: String,
    notification_type: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = env::var("GRAPHQL_API_URL")?;
    let api_key = env::var("GRAPHQL_API_KEY")?;
    let http_client = HttpClient::new();
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    headers.insert("x-api-key", HeaderValue::from_str(&api_key)?);

    let query = json!({
        "query": "
            mutation sendNotification($primaryID: String!, $secondaryID: String!, $notification_type: NotificationType!) {
                createNotification(
                    input: {
                        primaryID: $primaryID, 
                        secondaryID: $secondaryID, 
                        notification_type: $notification_type
                    }
                ) {
                    id
                    primaryID
                }
            }
    ",
        "variables": {
            "primaryID": primary_id,
            "secondaryID": secondary_id,
            "notification_type": notification_type
        }
    });

    let response = http_client
        .post(url)
        .headers(headers)
        .json(&query)
        .send()
        .await?;

    let response_body: Value = response.json().await?;

    println!("GraphQL Notification Reponse: {:#?}", response_body);
    Ok(())
}

async fn send_post_request_and_stream_response(
    chat: &mut Chat,
    user_msg: CopilotSendMessage,
    chat_manager: Arc<ChatManager>,
    copilot_endpoint: String,
    client: Arc<Client>,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = copilot_endpoint;
    let http_client = HttpClient::new();
    let primary_id = chat.get_agent_id().to_string();
    let secondary_id = chat.get_secondary_id().to_string();
    match http_client.post(url).json(&user_msg).send().await {
        Ok(response) => {
            let mut stream = response.bytes_stream();
            let mut buffer = Vec::new();
            let tx = chat_manager
                .get_or_create_chat(chat.get_chat_id().to_string())
                .await;

            while let Some(item) = stream.next().await {
                match item {
                    Ok(chunk) => {
                        let chunks = std::str::from_utf8(&chunk)
                            .unwrap()
                            .split_inclusive('}')
                            .collect::<Vec<_>>();

                        for chunk in chunks {
                            let message_text = chunk;

                            if let Err(e) = tx.send(WsMessage::Text(message_text.to_string())) {
                                eprintln!("Error broadcasting raw copilot message: {:?}", e);
                            }
                            let copilot_chunk = serde_json::from_str::<CopilotChunk>(chunk)
                                .expect("Failed to parse copilot chunk");
                            buffer.push(copilot_chunk);
                        }
                    }
                    Err(e) => {
                        let error_json = serde_json::json!({
                            "error": format!("Failed to get chunk: {:?}", e)
                        });
                        if let Err(e) = tx.send(WsMessage::Text(error_json.to_string())) {
                            eprintln!("Error broadcasting error message: {:?}", e);
                        }
                    }
                }
            }

            let copilot_message = CopilotMessage::new(buffer);
            chat.add_message(MessageType::Copilot(copilot_message.clone()));
            if let Err(e) = chat
                .send_message_to_db(&client, MessageType::Copilot(copilot_message.clone()))
                .await
            {
                eprintln!("Failed to send copilot message to db: {:?}", e);
            }
            let message_text = serde_json::json!(copilot_message).to_string();
            if let Err(e) = tx.send(WsMessage::Text(message_text)) {
                eprintln!("Error broadcasting copilot message: {:?}", e);
            } else {
                let notification_type = "COPILOT";
                match send_notification(primary_id, secondary_id, notification_type.to_string())
                    .await
                {
                    Ok(_) => println!("Notification sent"),
                    Err(e) => eprintln!("Error sending notification: {:?}", e),
                }
            }

            let final_json = serde_json::json!({  "action": "end",  "output": ""});

            tx.send(WsMessage::Text(final_json.to_string()))?;

            Ok(())
        }
        Err(e) => {
            let error_json = serde_json::json!({
            "error": format!("Failed to send post request: {:?}", e)
            });
            if let Err(e) = chat_manager
                .get_or_create_chat(chat.get_chat_id().to_string())
                .await
                .send(WsMessage::Text(error_json.to_string()))
            {
                eprintln!("Error broadcasting error message: {:?}", e);
                Err(Box::new(e))
            } else {
                Ok(())
            }
        }
    }
}
