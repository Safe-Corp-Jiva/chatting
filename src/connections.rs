pub enum ConnectionType {
    Copilot(String),
    People(String, String),
}

impl ConnectionType {
    pub fn instantiate_connection_type(primary_id: &str, secondary_id: &str) -> ConnectionType {
        if let "copilot" = secondary_id {
            ConnectionType::Copilot(String::from(secondary_id))
        } else {
            ConnectionType::People(String::from(primary_id), String::from(secondary_id))
        }
    }
}
