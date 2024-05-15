use core::fmt;
use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use serde::{Deserialize, Deserializer};
use uuid::Uuid as UUID;

use crate::{agents::DBItem, messages::MessageError};

fn default_sender() -> String {
    "Copilot".to_string()
}

#[derive(Deserialize, Debug)]
pub struct CopilotMessage {
    #[serde(default = "uuid::Uuid::new_v4", skip_deserializing)]
    message_id: UUID,
    action: String,
    output: String,
    #[serde(default = "default_sender", skip_deserializing)]
    sender: String,
}

impl fmt::Display for CopilotMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Message ID: {}, Action: {}, Output: {}, Sender: {}",
            self.message_id, self.action, self.output, self.sender
        )
    }
}

impl CopilotMessage {
    pub fn new(message_id: UUID, action: String, output: String) -> Self {
        Self {
            message_id,
            action,
            output,
            sender: "Copilot".to_string(),
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

        Ok(Self {
            message_id: UUID::parse_str(message_id).unwrap(),
            action: action.to_string(),
            output: output.to_string(),
            sender: "Copilot".to_string(),
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
        Ok(item)
    }

    pub fn get_message(&self) -> &str {
        &self.output
    }
}
