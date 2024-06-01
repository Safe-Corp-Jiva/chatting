use aws_sdk_dynamodb::{types::AttributeValue, Client, Error as DynamoError};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, TimestampMilliSeconds};
use std::{
    collections::HashMap,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid as UUID;

use crate::{
    agents::{AgentMessage, DBItem},
    copilot::CopilotMessage,
};

#[derive(Debug, Deserialize, Serialize, Clone)]
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

    pub fn to_db_item(&self, chat_id: &str) -> Result<DBItem, MessageError> {
        let mut item = match self {
            MessageType::User(message) => message.to_db_item(),
            MessageType::Copilot(message) => message.to_db_item(),
        }
        .unwrap();
        item.insert("ChatID".to_string(), AttributeValue::S(chat_id.to_string()));
        Ok(item)
    }

    pub fn from_db_item(item: DBItem) -> Result<MessageType, MessageError> {
        if item.contains_key("Action") {
            CopilotMessage::from_db_item(item).map(MessageType::Copilot)
        } else {
            AgentMessage::from_db_item(item).map(MessageType::User)
        }
    }

    pub fn to_chat_message(&self, chat_id: String) -> ChatMessage {
        match self {
            MessageType::User(message) => ChatMessage::new(
                chat_id,
                message.get_sender().to_string(),
                message.get_value().to_string(),
            ),
            MessageType::Copilot(message) => ChatMessage::new(
                chat_id,
                "Copilot".to_string(),
                message.get_message().to_string(),
            ),
        }
    }

    pub fn get_timestamp(&self) -> SystemTime {
        match self {
            MessageType::User(message) => message.get_timestamp(),
            MessageType::Copilot(message) => message.get_timestamp(),
        }
    }
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageType::User(message) => write!(f, "{}", message),
            MessageType::Copilot(message) => write!(f, "{}", message),
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct ChatMessage {
    chat_id: String,
    message: String,
    sender: String,
}

impl ChatMessage {
    pub fn new(chat_id: String, message: String, sender: String) -> Self {
        Self {
            chat_id,
            message,
            sender,
        }
    }
}

// CHAT INSTANCE
#[derive(Debug, Clone)]
pub struct Chat {
    chat_id: String,
    messages: Vec<ChatMessage>,
}

impl Chat {
    pub fn new(agent_id: String, secondary_id: String) -> Self {
        let chat_id = format!("{}-{}", agent_id, secondary_id);
        Self {
            chat_id,
            messages: Vec::new(),
        }
    }

    pub fn get_chat_history(&self) -> Vec<ChatMessage> {
        self.messages.clone()
    }

    /// Necessary getters
    pub fn get_chat_id(&self) -> String {
        self.chat_id.clone()
    }

    pub fn get_agent_id(&self) -> String {
        self.chat_id.split('-').next().unwrap().to_string()
    }

    pub fn get_secondary_id(&self) -> String {
        self.chat_id.split('-').last().unwrap().to_string()
    }

    /// Add a message to the chat instance
    pub fn add_message(&mut self, message: MessageType) {
        let message_to_push = message.to_chat_message(self.get_chat_id());
        self.messages.push(message_to_push)
    }

    /// Create a new message instance
    pub fn new_agent_message(
        &self,
        message_id: UUID,
        owner: String,
        message: String,
    ) -> AgentMessage {
        AgentMessage::new(message_id, self.get_chat_id(), owner, message)
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
        match message.to_db_item(&self.get_chat_id()) {
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

impl fmt::Display for Chat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Chat ID: {}", self.chat_id)
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

pub fn handle_copilot_messages(
    message: MessageType,
    chat: &mut Chat,
    client: &Client,
) -> Result<(), MessageError> {
    chat.add_message(message.clone());
    chat.send_message_to_db(client, message)
}
