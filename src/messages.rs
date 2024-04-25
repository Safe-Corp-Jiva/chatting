use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use tokio_tungstenite::tungstenite::protocol::Message;

#[derive(Debug)]
pub struct CallMessage {
    message_id: String,
    call_id: String,
    owner: String,
    message: String,
}

#[derive(Debug)]
pub struct CallChat {
    call_id: String,
    messages: Vec<Message>,
}

impl CallMessage {
    pub fn new(message_id: String, call_id: String, owner: String, message: String) -> Self {
        Self {
            message_id,
            call_id,
            owner,
            message,
        }
    }

    pub fn from_db_item(item: HashMap<String, AttributeValue>) -> Self {
        let message_id = item.get("MessageID").unwrap().as_s().unwrap().to_string();
        let call_id = item.get("CallID").unwrap().as_s().unwrap().to_string();
        let owner = item.get("Owner").unwrap().as_s().unwrap().to_string();
        let message = item.get("Message").unwrap().as_s().unwrap().to_string();

        Self {
            message_id,
            call_id,
            owner,
            message,
        }
    }

    pub fn get_message_id(&self) -> &str {
        &self.message_id
    }

    pub fn get_call_id(&self) -> &str {
        &self.call_id
    }

    pub fn get_value(&self) -> &str {
        &self.message
    }

    pub fn get_owner(&self) -> &str {
        &self.owner
    }

    pub fn to_db_item(&self) -> HashMap<String, AttributeValue> {
        let mut item = HashMap::new();
        item.insert(
            "MessageID".to_string(),
            AttributeValue::S(self.message_id.clone()),
        );
        item.insert(
            "CallID".to_string(),
            AttributeValue::S(self.call_id.clone()),
        );
        item.insert(
            "Message".to_string(),
            AttributeValue::S(self.message.clone()),
        );
        item.insert("Owner".to_string(), AttributeValue::S(self.owner.clone()));

        item
    }
}

impl CallChat {
    pub fn new(call_id: String, owner: String) -> Self {
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
        self.messages
            .push(Message::Text(message.get_value().to_string()));
    }
    /// Get all messages in the chat instance
    pub fn get_messages(&self) -> &Vec<Message> {
        &self.messages
    }
    /// Create a new message instance
    pub fn new_message(&self, message_id: String, owner: String, message: String) -> CallMessage {
        CallMessage::new(message_id, self.call_id.clone(), owner, message)
    }
}
