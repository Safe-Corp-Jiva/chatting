use anyhow::Error;
use tokio::net::TcpStream;

use crate::init;

pub enum ConnectionType {
    Copilot(String, String),
    People(String, String),
}

impl ConnectionType {
    pub async fn instantiate_connection_type(stream: &TcpStream) -> Result<ConnectionType, Error> {
        match init::generate_params_from_url(stream).await {
            Ok((primary_id, secondary_id)) => {
                if secondary_id == "copilot" {
                    Ok(ConnectionType::Copilot(primary_id, secondary_id))
                } else if secondary_id == "supervisor" {
                    Ok(ConnectionType::People(primary_id, secondary_id))
                } else {
                    Err(anyhow::anyhow!("Error getting stream type"))
                }
            }
            Err(_) => Err(anyhow::anyhow!("Error getting stream type")),
        }
    }
}
