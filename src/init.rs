use crate::messages::Chat;
use crate::messages::MessageError;
use crate::messages::MessageType;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use httparse::Request;
use httparse::EMPTY_HEADER;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use uuid::Uuid;

use crate::messages;
use url::Url;

pub async fn get_messages_from_db(
    client: &Client,
    chat_id: String,
) -> Result<Vec<MessageType>, aws_sdk_dynamodb::Error> {
    let mut messages = Vec::new();

    let chat_id = AttributeValue::S(chat_id.to_string());

    let response = client
        .scan()
        .table_name("Messages")
        .filter_expression("CallID = :chat_id")
        .expression_attribute_values(":chat_id", chat_id.clone())
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
            println!("No messages found for chat_id: {:?}", chat_id);
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
    let chat_id = url
        .query_pairs()
        .find(|(k, _)| k == "agentID")
        .map(|(_, v)| v)
        .unwrap_or_default();

    let owner = url
        .query_pairs()
        .find(|(k, _)| k == "secondaryID")
        .map(|(_, v)| v)
        .unwrap_or_default();
    Ok((chat_id.to_string(), owner.to_string()))
}

pub async fn send_chat_to_db(chat: Arc<Chat>, client: Arc<Client>) -> Result<(), MessageError> {
    let table_name = "Chats";
    let chat_id = chat.get_chat_id().to_string();
    let agent_id = chat.get_agent_id().to_string();
    let secondary_id = chat.get_secondary_id().to_string();

    // Check if chat exists
    let query_req = client
        .get_item()
        .table_name(table_name)
        .key("AgentID", AttributeValue::S(agent_id.clone()))
        .key("SecondaryID", AttributeValue::S(secondary_id.clone()))
        .send()
        .await;

    match query_req {
        Ok(query_resp) => {
            if query_resp.item.is_none() {
                // Chat does not exist, insert it
                let mut item = HashMap::new();
                item.insert("AgentID".to_string(), AttributeValue::S(agent_id.clone()));
                item.insert(
                    "SecondaryID".to_string(),
                    AttributeValue::S(secondary_id.clone()),
                );
                let put_req = client
                    .put_item()
                    .table_name(table_name)
                    .set_item(Some(item))
                    .send()
                    .await;

                match put_req {
                    Ok(_) => {
                        println!("Chat created in database: {}", chat_id);
                        Ok(())
                    }
                    Err(e) => Err(MessageError::DatabaseError(e.into())),
                }
            } else {
                // Chat already exists
                println!("Chat already exists: {}", chat_id);
                Ok(())
            }
        }
        Err(e) => Err(MessageError::DatabaseError(e.into())),
    }
}
