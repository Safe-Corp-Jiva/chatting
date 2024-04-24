use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::config::ProvideCredentials;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{accept_async, WebSocketStream};

#[tokio::main]
async fn main() {
    // Setup of the AWS Client
    let region_provider = RegionProviderChain::default_provider().or_else("us-west-2");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);

    let addr = "127.0.0.1:3030";
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_connection(stream, client.clone()));
    }
}

async fn handle_connection(stream: TcpStream, client: Client) {
    let ws_stream = accept_async(stream).await.expect("Failed to accept");

    let (mut write, mut read) = ws_stream.split();

    // Fetch initial messages and send to the client
    let initial_messages = get_messages_from_db(client)
        .await
        .expect("Failed to get messages from db");

    for message in initial_messages {
        write
            .send(Message::Text(message))
            .await
            .expect("Failed to send message");
    }

    // Echo incoming messages back to the client
    while let Some(Ok(message)) = read.next().await {
        println!("Received a message: {}", message);
        if let Err(e) = write.send(message).await {
            eprintln!("Error sending message: {:?}", e);
            break;
        }
    }
}

async fn get_messages_from_db(client: Client) -> Result<Vec<String>, aws_sdk_dynamodb::Error> {
    let mut messages = Vec::new();

    let attr_val = AttributeValue::S("test2".to_string());
    let response = client
        .query()
        .table_name("Messages")
        .key_condition_expression("Chatter=:value")
        .expression_attribute_values(":value", attr_val)
        .send()
        .await;

    for item in response.unwrap().items.unwrap() {
        let message = item.get("value").unwrap().as_s().unwrap().to_string();
        messages.push(message);
    }

    println!("Messages: {:?}", messages);

    Ok(messages)
}
