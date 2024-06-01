use core::fmt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aws_sdk_dynamodb::types::AttributeValue;
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::serde_as;
use uuid::Uuid as UUID;

use crate::agents::AgentMessage;
use crate::messages::{Chat, ChatMessage};
use crate::{agents::DBItem, messages::MessageError};

fn default_sender() -> String {
    "Copilot".to_string()
}

fn default_serialize_uuid<S>(uuid: &UUID, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&uuid.to_string())
}

fn current_time() -> SystemTime {
    SystemTime::now()
}

fn deserialize_timestamp<'de, D>(_deserilizer: D) -> Result<SystemTime, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(SystemTime::now())
}

#[serde_as]
#[derive(Serialize, Debug, Clone)]
pub struct CopilotSendMessage {
    input: String,
    chat_history: Vec<ChatMessage>,
}

impl CopilotSendMessage {
    pub fn new(input: String) -> Self {
        CopilotSendMessage {
            input,
            chat_history: Vec::new(),
        }
    }

    pub fn from_agent_message(message: AgentMessage, chat: Arc<Chat>) -> Self {
        let input = message.get_value().to_string();
        let chat_history = chat.get_chat_history();
        CopilotSendMessage {
            input,
            chat_history,
        }
    }
}

#[serde_as]
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CopilotMessage {
    #[serde(
        default = "uuid::Uuid::new_v4",
        skip_deserializing,
        serialize_with = "default_serialize_uuid"
    )]
    message_id: UUID,
    #[serde(deserialize_with = "parse_defaults")]
    action: String,
    #[serde(deserialize_with = "parse_defaults")]
    output: String,
    #[serde(default = "default_sender", skip_deserializing)]
    sender: String,
    #[serde(default = "current_time", deserialize_with = "deserialize_timestamp")]
    timestamp: SystemTime,
}

fn parse_defaults<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Deserialize::deserialize(d).map(|x: Option<String>| {
        println!("x: {:?}", x);
        x.unwrap_or_default()
    })
}

impl fmt::Display for CopilotMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Message ID: {}, Action: {}, Output: {}, Sender: {}, Timestamp: {:?}",
            self.message_id, self.action, self.output, self.sender, self.timestamp
        )
    }
}

impl CopilotMessage {
    pub fn new(action: String, output: String) -> Self {
        Self {
            message_id: UUID::new_v4(),
            action,
            output,
            sender: "Copilot".to_string(),
            timestamp: SystemTime::now(),
        }
    }

    pub fn from_db_item(item: HashMap<String, AttributeValue>) -> Result<Self, MessageError> {
        let message_id = item
            .get("MessageID")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| MessageError::InvalidAttribute("MessageID".to_string()))?;

        let action = item
            .get("Action")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| MessageError::InvalidAttribute("Action".to_string()))?;

        let output = item
            .get("Output")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| MessageError::InvalidAttribute("Output".to_string()))?;

        let timestamp = item
            .get("Timestamp")
            .and_then(|v| v.as_n().ok())
            .and_then(|n| n.parse().ok())
            .map(|millis| UNIX_EPOCH + std::time::Duration::from_millis(millis))
            .ok_or_else(|| MessageError::InvalidAttribute("Timestamp".to_string()))?;

        Ok(Self {
            message_id: UUID::parse_str(message_id).unwrap(),
            action: action.to_string(),
            output: output.to_string(),
            sender: "Copilot".to_string(),
            timestamp,
        })
    }

    pub fn to_db_item(&self) -> Result<DBItem, MessageError> {
        let mut item: HashMap<String, AttributeValue> = HashMap::new();
        item.insert(
            "MessageID".to_string(),
            AttributeValue::S(self.message_id.to_string()),
        );
        item.insert(
            "Action".to_string(),
            AttributeValue::S(self.action.to_string()),
        );
        item.insert(
            "Output".to_string(),
            AttributeValue::S(self.output.to_string()),
        );
        item.insert(
            "Sender".to_string(),
            AttributeValue::S("Copilot".to_string()),
        );
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

    pub fn get_message(&self) -> &str {
        &self.output
    }

    pub fn get_timestamp(&self) -> SystemTime {
        self.timestamp
    }

    pub fn get_message_id(&self) -> UUID {
        self.message_id
    }
}
