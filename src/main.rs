use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::{
    config::ProvideCredentials, types::AttributeValue, Client, Error as DynamoError,
};
use futures_util::{SinkExt, StreamExt};
use httparse::{Request, EMPTY_HEADER};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};
use url::Url;

mod init;
mod messages;

#[tokio::main]
async fn main() {
    let region_provider = RegionProviderChain::default_provider().or_else("us-west-2");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);

    println!(
        "Credentials loaded: {:?}",
        config
            .credentials_provider()
            .unwrap()
            .provide_credentials()
            .await
    );

    let addr = "127.0.0.1:3030";
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_connection(stream, client.clone()));
    }
}

async fn handle_connection(stream: TcpStream, client: Client) {
    let mut buf = [0u8; 2048];
    let mut headers = [EMPTY_HEADER; 30];
    let mut req = Request::new(&mut headers);

    let nbytes = stream
        .peek(&mut buf)
        .await
        .expect("Failed to read from stream");
    let _parsed = req
        .parse(&buf[..nbytes])
        .expect("Failed to parse HTTP request");
    let path = req.path.expect("Request path is required");

    let url = Url::parse(&format!("http://dummyhost{}", path)).expect("Failed to parse URL");
    let call_id = url
        .query_pairs()
        .find(|(k, _)| k == "call_id")
        .map(|(_, v)| v)
        .unwrap_or_default();

    let owner = url
        .query_pairs()
        .find(|(k, _)| k == "owner")
        .map(|(_, v)| v)
        .unwrap_or_default();

    let ws_stream = accept_async(stream)
        .await
        .expect("Failed to accept WebSocket connection");

    let (mut write, mut read) = ws_stream.split();

    let mut conversation = messages::CallChat::new(call_id.to_string(), owner.to_string());

    let initial_messages: Vec<messages::CallMessage> =
        get_messages_from_db(&client, call_id.to_string(), owner.to_string())
            .await
            .expect("Failed to get messages from db");

    for message in initial_messages {
        write
            .send(Message::Text(message.get_value().to_string()))
            .await
            .expect("Failed to send message");
        conversation.add_message(message);
    }

    while let Some(Ok(message)) = read.next().await {
        let message = message
            .to_text()
            .expect("Failed to convert message to text");
        send_message_to_db(&client, message.to_string())
            .await
            .expect("Failed to send message to db");
        if let Err(e) = write.send(message.into()).await {
            eprintln!("Error sending message: {:?}", e);
            break;
        }
    }
}

async fn get_messages_from_db(
    client: &Client,
    call_id: String,
    owner: String,
) -> Result<Vec<messages::CallMessage>, DynamoError> {
    let mut messages = Vec::new();

    let owner = AttributeValue::S(owner.to_string());
    let call_id = AttributeValue::S(call_id.to_string());

    let response = client
        .query()
        .table_name("Chats")
        .key_condition_expression("Chatter=:owner AND CallID=:call_id")
        .expression_attribute_values(":owner", owner)
        .expression_attribute_values(":call_id", call_id)
        .send()
        .await;

    if let Ok(result) = response {
        if let Some(items) = result.items {
            println!("Items: {:?}", items);
            for item in items {
                let message = messages::CallMessage::from_db_item(item);
                messages.push(message);
            }
        }
    }

    println!("Messages: {:?}", messages);

    Ok(messages)
}

async fn send_message_to_db(client: &Client, message: String) -> Result<(), DynamoError> {
    let attr_val = AttributeValue::S(message.clone());
    let response = client
        .put_item()
        .table_name("Messages")
        .item("Chatter", attr_val)
        .item("value", AttributeValue::S("Message content".to_string()))
        .send()
        .await;

    println!("Response: {:?}", response);

    Ok(())
}
