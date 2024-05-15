use httparse::{Request, EMPTY_HEADER};
use messages::MessageType;

use aws_sdk_dynamodb::{types::AttributeValue, Client, Error as DynamoError};
use tokio::net::TcpStream;
use url::Url;

use crate::messages;

pub async fn get_messages_from_db(
    client: &Client,
    call_id: String,
    owner: String,
) -> Result<Vec<MessageType>, DynamoError> {
    let mut messages = Vec::new();

    let owner = AttributeValue::S(owner.to_string());
    let call_id = AttributeValue::S(call_id.to_string());

    let response = client
        .query()
        .table_name("Messages")
        .key_condition_expression("Sender=:value AND CallID=:call_id")
        .expression_attribute_values(":value", owner)
        .expression_attribute_values(":call_id", call_id)
        .send()
        .await;

    if let Ok(result) = response {
        if let Some(items) = result.items {
            for item in items {
                let message = messages::MessageType::from_db_item(item);
                match message {
                    Ok(message) => messages.push(message),
                    Err(e) => eprintln!("Error parsing message: {:?}", e),
                }
            }
        }
        for message in &messages {
            println!("{}", message);
        }
    }

    Ok(messages)
}

pub async fn generate_params_from_url(stream: &TcpStream) -> Result<(String, String), String> {
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
    Ok((call_id.to_string(), owner.to_string()))
}
