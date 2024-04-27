use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt};

use aws_sdk_dynamodb::{types::AttributeValue, Client, Error as DynamoError};
use tokio_tungstenite::tungstenite::protocol::Message;

use uuid::Uuid as UUID;

// MESSAGE INSTANCE
#[derive(Debug, Deserialize, Clone)]
pub struct CallMessage {
    #[serde(default = "uuid::Uuid::new_v4", skip_deserializing)]
    message_id: UUID,
    call_id: String,
    sender: String,
    message: String,
}

type DBItem = HashMap<String, AttributeValue>;

impl fmt::Display for CallMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Message ID: {}, Call ID: {}, Sender: {}, Message: {}",
            self.message_id, self.call_id, self.sender, self.message
        )
    }
}

impl Into<Message> for CallMessage {
    fn into(self) -> Message {
        Message::Text(self.message)
    }
}

impl CallMessage {
    pub fn new(message_id: UUID, call_id: String, sender: String, message: String) -> Self {
        Self {
            message_id,
            call_id,
            sender,
            message,
        }
    }

    pub fn from_db_item(item: HashMap<String, AttributeValue>) -> Result<Self, MessageError> {
        let message_id = item
            .get("MessageID")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| MessageError::InvalidAttribute("MessageID".to_string()))?;

        let call_id = item
            .get("CallID")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| MessageError::InvalidAttribute("CallID".to_string()))?;

        let sender = item
            .get("Sender")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| MessageError::InvalidAttribute("Sender".to_string()))?;

        let message = item
            .get("Message")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| MessageError::InvalidAttribute("Message".to_string()))?;

        Ok(Self {
            message_id: UUID::parse_str(message_id).unwrap(),
            call_id: call_id.to_string(),
            sender: sender.to_string(),
            message: message.to_string(),
        })
    }
    pub fn get_message_id(&self) -> String {
        self.message_id.to_string()
    }

    pub fn get_call_id(&self) -> &str {
        &self.call_id
    }

    pub fn get_value(&self) -> &str {
        &self.message
    }

    pub fn get_owner(&self) -> &str {
        &self.sender
    }

    pub fn to_db_item(&self) -> Result<DBItem, MessageError> {
        let mut item = HashMap::new();
        println!("Message: {:?}", self);
        item.insert(
            "MessageID".to_string(),
            AttributeValue::S(self.message_id.to_string()),
        );
        item.insert(
            "CallID".to_string(),
            AttributeValue::S(self.call_id.clone()),
        );
        item.insert(
            "Message".to_string(),
            AttributeValue::S(self.message.clone()),
        );
        item.insert("Sender".to_string(), AttributeValue::S(self.sender.clone()));

        Ok(item)
    }
}

// CHAT INSTANCE
#[derive(Debug)]
pub struct CallChat {
    call_id: String,
    messages: Vec<CallMessage>,
}

impl CallChat {
    pub fn new(call_id: String) -> Self {
        Self {
            call_id,
            messages: Vec::new(),
        }
    }
    /// Necessary getters
    pub fn get_call_id(&self) -> &str {
        &self.call_id
    }
    /// Add a message to the chat instance
    pub fn add_message(&mut self, message: CallMessage) {
        self.messages.push(message);
    }
    /// Get all messages in the chat instance
    pub fn get_messages(&self) -> &Vec<CallMessage> {
        &self.messages
    }
    /// Create a new message instance
    pub fn new_message(&self, message_id: UUID, owner: String, message: String) -> CallMessage {
        CallMessage::new(message_id, self.call_id.clone(), owner, message)
    }
}

// ERROR IMPLEMENTATION
#[derive(Debug)]
pub enum MessageError {
    InvalidAttribute(String),
    DatabaseError(DynamoError),
    ConversionError,
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageError::InvalidAttribute(attr) => {
                write!(f, "missing or invalid attribute: {}", attr)
            }
            MessageError::DatabaseError(e) => write!(f, "database error: {}", e),
            MessageError::ConversionError => write!(f, "data conversion error"),
        }
    }
}

impl std::error::Error for MessageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MessageError::DatabaseError(e) => Some(e),
            _ => None,
        }
    }
}

// METHODS FOR MESSAGES
pub async fn send_message_to_db(client: &Client, message: CallMessage) -> Result<(), MessageError> {
    match message.to_db_item() {
        Ok(item) => {
            let req = client
                .put_item()
                .table_name("Messages")
                .item("MessageID", item.get("MessageID").unwrap().clone())
                .item("Sender", item.get("Sender").unwrap().clone())
                .item("CallID", item.get("CallID").unwrap().clone())
                .item("Message", item.get("Message").unwrap().clone());
            match req.send().await {
                Ok(_) => {
                    println!("Message sent to database: {}", message);
                    Ok(())
                }
                Err(e) => Err(MessageError::DatabaseError(e.into()).into()),
            }
        }
        Err(_) => Err(MessageError::ConversionError.into()),
    }
}
