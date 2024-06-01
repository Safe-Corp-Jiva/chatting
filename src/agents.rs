use serde::{Deserialize, Serialize};
use serde_with::{serde_as, TimestampMilliSeconds};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::HashMap, fmt};

use aws_sdk_dynamodb::types::AttributeValue;
use tokio_tungstenite::tungstenite::protocol::Message;

use uuid::Uuid as UUID;

use crate::messages::MessageError;

fn default_serialize_uuid<S>(uuid: &UUID, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&uuid.to_string())
}

fn current_time() -> SystemTime {
    SystemTime::now()
}

fn deserialize_timestamp<'de, D>(deserilizer: D) -> Result<SystemTime, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(SystemTime::now())
}

#[serde_as]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentMessage {
    #[serde(
        default = "uuid::Uuid::new_v4",
        skip_deserializing,
        serialize_with = "default_serialize_uuid"
    )]
    message_id: UUID,
    chat_id: String,
    sender: String,
    message: String,
    #[serde(default = "current_time", deserialize_with = "deserialize_timestamp")]
    timestamp: SystemTime,
}

pub type DBItem = HashMap<String, AttributeValue>;

impl fmt::Display for AgentMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Message ID: {}, Call ID: {}, Sender: {}, Message: {}, Timestamp: {:?}",
            self.message_id, self.chat_id, self.sender, self.message, self.timestamp
        )
    }
}

impl Into<Message> for AgentMessage {
    fn into(self) -> Message {
        Message::Text(self.message)
    }
}

impl AgentMessage {
    pub fn new(message_id: UUID, chat_id: String, sender: String, message: String) -> Self {
        Self {
            message_id,
            chat_id,
            sender,
            message,
            timestamp: SystemTime::now(),
        }
    }

    pub fn from_db_item(item: HashMap<String, AttributeValue>) -> Result<Self, MessageError> {
        let message_id = item
            .get("MessageID")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| MessageError::InvalidAttribute("MessageID".to_string()))?;

        let chat_id = item
            .get("ChatID")
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

        let timestamp = item
            .get("Timestamp")
            .and_then(|v| v.as_n().ok())
            .and_then(|n| n.parse().ok())
            .map(|millis| UNIX_EPOCH + std::time::Duration::from_millis(millis))
            .ok_or_else(|| MessageError::InvalidAttribute("Timestamp".to_string()))?;

        Ok(Self {
            message_id: UUID::parse_str(message_id).unwrap(),
            chat_id: chat_id.to_string(),
            sender: sender.to_string(),
            message: message.to_string(),
            timestamp,
        })
    }

    pub fn get_message_id(&self) -> UUID {
        self.message_id
    }

    pub fn get_chat_id(&self) -> &str {
        &self.chat_id
    }

    pub fn get_value(&self) -> &str {
        &self.message
    }

    pub fn get_sender(&self) -> &str {
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
            AttributeValue::S(self.chat_id.clone()),
        );
        item.insert(
            "Message".to_string(),
            AttributeValue::S(self.message.clone()),
        );
        item.insert("Sender".to_string(), AttributeValue::S(self.sender.clone()));
        item.insert(
            "Timestamp".to_string(),
            AttributeValue::N(
                self.timestamp
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
                    .to_string(),
            ),
        );

        Ok(item)
    }
    pub fn get_timestamp(&self) -> SystemTime {
        self.timestamp
    }
}
