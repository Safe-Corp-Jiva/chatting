use serde::Deserialize;
use std::{collections::HashMap, fmt};

use aws_sdk_dynamodb::types::AttributeValue;
use tokio_tungstenite::tungstenite::protocol::Message;

use uuid::Uuid as UUID;

use crate::messages::MessageError;

// MESSAGE INSTANCE
#[derive(Debug, Deserialize, Clone)]
pub struct AgentMessage {
    #[serde(default = "uuid::Uuid::new_v4", skip_deserializing)]
    message_id: UUID,
    call_id: String,
    sender: String,
    message: String,
}

pub type DBItem = HashMap<String, AttributeValue>;

impl fmt::Display for AgentMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Message ID: {}, Call ID: {}, Sender: {}, Message: {}",
            self.message_id, self.call_id, self.sender, self.message
        )
    }
}

impl Into<Message> for AgentMessage {
    fn into(self) -> Message {
        Message::Text(self.message)
    }
}

impl AgentMessage {
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
