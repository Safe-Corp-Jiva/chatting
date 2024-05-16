// src/main.rs
use crate::copilot::CopilotMessage;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::Client;
use futures_util::SinkExt;
use futures_util::StreamExt;
use messages::{Chat, MessageType};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{self, Duration};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

mod agents;
mod copilot;
mod init;
mod messages;

const BUFFER_SIZE: usize = 10;
const TIMEOUT_DURATION: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() {
    let region_provider = RegionProviderChain::default_provider().or_else("us-west-2");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);

    let addr = "127.0.0.1:3030";
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");

    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        let client = client.clone();
        tokio::spawn(async move {
            handle_connection(stream, client).await;
        });
    }
}

async fn handle_connection(stream: TcpStream, client: Client) -> tokio::task::JoinHandle<()> {
    let (agent_id, secondary_id) = match init::generate_params_from_url(&stream).await {
        Ok((agent_id, secondary_id)) => (agent_id, secondary_id),
        Err(e) => {
            eprintln!("Error parsing URL: {:?}", e);
            ("Err".to_string(), e.to_string())
        }
    };

    // Create call on db if it doesn't exist
    let mut chat = Arc::new(Chat::new(agent_id, secondary_id));
    let client = Arc::new(client);

    init::send_chat_to_db(chat.clone(), client.clone())
        .await
        .expect("Failed to send chat to database");

    let ws_stream = accept_async(stream)
        .await
        .expect("Failed to accept WebSocket connection");

    let (mut write, read) = ws_stream.split();
    let read = Arc::new(Mutex::new(read));
    let buffer = Arc::new(Mutex::new(VecDeque::new()));

    // Fetch initial messages and send to client
    let initial_messages = init::get_messages_from_db(&client, chat.get_chat_id().to_string())
        .await
        .expect("Failed to get messages from db");

    for message in initial_messages {
        write
            .send(Message::Text(message.to_string()))
            .await
            .expect("Failed to send message to client");
    }

    let read_clone = read.clone();
    let buffer_clone = buffer.clone();

    let chat_clone = chat.clone();
    let client_clone = client.clone();
    tokio::spawn(async move {
        handle_messages(read_clone, buffer_clone, chat_clone, client_clone).await;
    });

    let chat_clone = chat.clone();
    let client_clone = client.clone();
    tokio::spawn(async move {
        process_buffer(buffer, chat_clone, client_clone).await;
    })
}

async fn handle_messages(
    read: Arc<
        Mutex<
            impl StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
        >,
    >,
    buffer: Arc<Mutex<VecDeque<MessageType>>>,
    chat: Arc<Chat>,
    client: Arc<Client>,
) {
    let mut read = read.lock().await;

    while let Some(Ok(message)) = read.next().await {
        if let WsMessage::Text(text) = message {
            match serde_json::from_str::<MessageType>(&text) {
                Ok(message_type) => match message_type {
                    MessageType::User(user_msg) => {
                        chat.send_message_to_db(&client, MessageType::User(user_msg))
                            .await
                            .expect("Failed to send user message to db");
                    }
                    MessageType::Copilot(copilot_msg) => {
                        let mut buffer = buffer.lock().await;
                        buffer.push_back(MessageType::Copilot(copilot_msg));
                    }
                },
                Err(e) => eprintln!("Error parsing message: {:?}", e),
            }
        }
    }
}

async fn process_buffer(
    buffer: Arc<Mutex<VecDeque<MessageType>>>,
    chat: Arc<Chat>,
    client: Arc<Client>,
) {
    loop {
        let messages = {
            let mut buffer = buffer.lock().await;
            if !buffer.is_empty() {
                Some(buffer.drain(..).collect::<Vec<_>>())
            } else {
                None
            }
        };

        if let Some(messages) = messages {
            let combined_message = messages
                .iter()
                .map(|msg| msg.get_value())
                .collect::<Vec<_>>()
                .join("");

            let combined_copilot_message =
                CopilotMessage::new("Combined".to_string(), combined_message);

            handle_copilot_stream(chat.clone(), combined_copilot_message, &client).await;
        }
        time::sleep(TIMEOUT_DURATION).await;
    }
}
async fn handle_copilot_stream(chat: Arc<Chat>, copilot_msg: CopilotMessage, client: &Client) {
    // Assume the copilot message contains a JSON array of messages
    let messages: Vec<CopilotMessage> =
        serde_json::from_str(&copilot_msg.get_message()).unwrap_or_else(|_| vec![copilot_msg]);

    for message in messages {
        chat.send_message_to_db(client, MessageType::Copilot(message))
            .await
            .expect("Failed to send copilot message to db");
    }
}
