use tokio::net::TcpStream;

use crate::init;

pub enum ConnectionType {
    Copilot(String, String),
    People(String, String),
}

impl ConnectionType {
    pub async fn instantiate_connection_type(stream: &TcpStream) -> ConnectionType {
        match init::generate_params_from_url(stream).await {
            Ok((primary_id, secondary_id)) => {
                if secondary_id == "copilot" {
                    ConnectionType::Copilot(primary_id, secondary_id)
                } else if secondary_id == "supervisor" {
                    ConnectionType::People(primary_id, secondary_id)
                } else {
                    ConnectionType::People("".to_string(), "".to_string())
                }
            }
            Err(_) => {
                eprintln!("Error getting stream type");
                ConnectionType::People("".to_string(), "".to_string())
            }
        }
    }
}
