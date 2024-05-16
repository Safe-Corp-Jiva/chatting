use crate::messages::MessageError;
use crate::messages::MessageType;
use crate::CallChat;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use httparse::Request;
use httparse::EMPTY_HEADER;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;

use crate::messages;
use url::Url;

pub async fn get_messages_from_db(
    client: &Client,
    call_id: String,
    owner: String,
) -> Result<Vec<MessageType>, aws_sdk_dynamodb::Error> {
    let mut messages = Vec::new();

    let owner = AttributeValue::S(owner.to_string());
    let call_id = AttributeValue::S(call_id.to_string());

    let response = client
        .scan()
        .table_name("Messages")
        .filter_expression("CallID = :call_id")
        .expression_attribute_values(":call_id", call_id.clone())
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
        } else {
            println!("No messages found for call_id: {:?}", call_id);
        }
        for message in &messages {
            println!("Message: {}", message);
        }
    } else {
        eprintln!("Error querying database: {:?}", response.err().unwrap());
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
pub async fn send_chat_to_db(chat: Arc<CallChat>, client: Arc<Client>) -> Result<(), MessageError> {
    let table_name = "Chats";
    let call_id = chat.get_call_id().to_string();
    let call_id_attr = AttributeValue::S(call_id.clone());
    let chatter_attr = AttributeValue::S("agent".to_string());

    // Check if chat exists
    let query_req = client
        .get_item()
        .table_name(table_name)
        .key("Chatter", chatter_attr.clone())
        .key("CallID", call_id_attr.clone())
        .send()
        .await;

    match query_req {
        Ok(query_resp) => {
            if query_resp.item.is_none() {
                // Chat does not exist, insert it
                let mut item = HashMap::new();
                item.insert("CallID".to_string(), call_id_attr);
                item.insert("Chatter".to_string(), chatter_attr);
                let put_req = client
                    .put_item()
                    .table_name(table_name)
                    .set_item(Some(item))
                    .send()
                    .await;

                match put_req {
                    Ok(_) => {
                        println!("Chat created in database: {}", call_id);
                        Ok(())
                    }
                    Err(e) => Err(MessageError::DatabaseError(e.into())),
                }
            } else {
                // Chat already exists
                println!("Chat already exists: {}", call_id);
                Ok(())
            }
        }
        Err(e) => Err(MessageError::DatabaseError(e.into())),
    }
}
