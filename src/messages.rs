use aws_sdk_dynamodb::{types::AttributeValue, Client};
use serde::Deserialize;
use uuid::Uuid as UUID;

use core::fmt;

use aws_sdk_dynamodb::Error as DynamoError;

use crate::{
    agents::{AgentMessage, DBItem},
    copilot::CopilotMessage,
};

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum MessageType {
    User(AgentMessage),
    Copilot(CopilotMessage),
}

impl MessageType {
    pub fn get_value(&self) -> String {
        match self {
            MessageType::User(message) => message.get_value().to_string(),
            MessageType::Copilot(message) => message.get_message().to_string(),
        }
    }

    pub fn to_db_item(&self, call_id: &str) -> Result<DBItem, MessageError> {
        let mut item = match self {
            MessageType::User(message) => message.to_db_item(),
            MessageType::Copilot(message) => message.to_db_item(),
        }
        .unwrap();
        item.insert("CallID".to_string(), AttributeValue::S(call_id.to_string()));
        Ok(item)
    }

    pub fn from_db_item(item: DBItem) -> Result<MessageType, MessageError> {
        if item.contains_key("Action") {
            CopilotMessage::from_db_item(item).map(MessageType::Copilot)
        } else {
            AgentMessage::from_db_item(item).map(MessageType::User)
        }
    }
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::User(message) => write!(f, "{}", message),
            MessageType::Copilot(message) => write!(f, "{}", message),
        }
    }
}

// CHAT INSTANCE
#[derive(Debug, Clone)]
pub struct CallChat {
    call_id: String,
    messages: Vec<MessageType>,
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
    pub fn add_message(&mut self, message: MessageType) {
        self.messages.push(message);
    }

    /// Get all messages in the chat instance
    pub fn get_messages(&self) -> &Vec<MessageType> {
        &self.messages
    }

    /// Create a new message instance
    pub fn new_agent_message(
        &self,
        message_id: UUID,
        owner: String,
        message: String,
    ) -> AgentMessage {
        AgentMessage::new(message_id, self.call_id.clone(), owner, message)
    }

    /// Create a new copilot message instance
    pub fn new_copilot_message(&self, action: String, output: String) -> CopilotMessage {
        CopilotMessage::new(action, output)
    }

    /// Send message to database
    pub async fn send_message_to_db(
        &self,
        client: &Client,
        message: MessageType,
    ) -> Result<(), MessageError> {
        match message.to_db_item(&self.call_id) {
            Ok(item) => {
                let req = client
                    .put_item()
                    .table_name("Messages")
                    .set_item(Some(item))
                    .send()
                    .await;
                match req {
                    Ok(_) => {
                        println!("Message sent to database: {}", message);
                        Ok(())
                    }
                    Err(e) => Err(MessageError::DatabaseError(e.into())),
                }
            }
            Err(e) => {
                eprintln!("Error converting message to DB item: {}", e);
                Err(MessageError::ConversionError)
            }
        }
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
