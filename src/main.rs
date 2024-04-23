use tokio::net::TcpListener;
use websocket::{result::WebSocketResult, server::WsServer};

#[tokio::main]
async fn main() -> WebSocketResult<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Listening on: {}", listener.local_addr()?);
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(async move {
            if let Err(e) = WsServer::accept(stream).await {
                eprintln!("Error: {:?}", e);
            }
        });
    }
    Ok(())
}
