use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::Client;
use futures_util::{SinkExt, StreamExt};
use messages::CallChat;
use messages::{send_message_to_db, MessageType};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::{Error, Message as WsMessage};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

mod agents;
mod copilot;
mod init;
mod messages;

#[tokio::main]
async fn main() {
    let region_provider = RegionProviderChain::default_provider().or_else("us-west-2");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);

    let addr = "127.0.0.1:3030";
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_connection(stream, client.clone()));
    }
}

async fn handle_connection(stream: TcpStream, client: Client) {
    let (call_id, owner) = match init::generate_params_from_url(&stream).await {
        Ok((call_id, owner)) => (call_id, owner),
        Err(e) => {
            eprintln!("Error parsing URL: {:?}", e);
            return;
        }
    };

    let ws_stream = accept_async(stream)
        .await
        .expect("Failed to accept WebSocket connection");

    let (mut write, mut read) = ws_stream.split();

    let mut conversation = CallChat::new(call_id.to_string());

    let initial_messages: Vec<MessageType> =
        init::get_messages_from_db(&client, call_id.to_string(), owner.to_string())
            .await
            .expect("Failed to get messages from db");

    for message in initial_messages {
        write
            .send(Message::Text(message.get_value().to_string()))
            .await
            .expect("Failed to send message");
        conversation.add_message(message);
    }

    handle_messages(&mut read, &client, &call_id).await;
}

async fn handle_messages(
    read: &mut (impl StreamExt<Item = Result<WsMessage, Error>> + Unpin),
    client: &Client,
    call_id: &str,
) {
    while let Some(Ok(message)) = read.next().await {
        if let WsMessage::Text(text) = message {
            match serde_json::from_str::<MessageType>(&text) {
                Ok(message) => {
                    send_message_to_db(&client, message, &call_id)
                        .await
                        .expect("Failed to send message to db");
                }
                Err(e) => eprintln!("Error parsing message: {:?}", e),
            }
        }
    }
}
