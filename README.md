# JivaChat

JivaChat is a simple chat application that utilizes WebSockets for real-time communication and AWS DynamoDB for storing and retrieving messages. This application is built with Rust and leverages asynchronous programming paradigms. It also sends notifications to the Amplify instance being utilized, and also has support for a copilot conection.

## Features

- Real-time chat communication using WebSockets.
- Storage and retrieval of chat messages from AWS DynamoDB.
- Notification service embedded within message handling.
- Graceful, non-panic errors.
- Full asynchronous operation using Tokio.

## Prerequisites

Before you begin, ensure you have met the following requirements:
- You have installed Rust and Cargo. Visit [Rust Getting Started](https://www.rust-lang.org/learn/get-started) for installation instructions.
> The following command can be run as your installation command for Unix Systems:
```shell
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
> On Windows, you can download `rustup-init.exe` from the [Rust documentation](https://forge.rust-lang.org/infra/other-installation-methods.html).

- You have an AWS account and have configured AWS credentials on your machine. AWS credentials should have permissions to access DynamoDB services.
> All of the steps to setup an AWS Account with DynamoDB access can be found in the [AWS Docuemntation](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/SettingUp.DynamoWebService.html).

## Dependencies

This project uses the following crates:
### AWS
- [aws-config](https://crates.io/crates/aws-config): Configuration crate for AWS SDKs with Rust.
- [aws-sdk-dynamodb](https://crates.io/crates/aws-sdk-dynamodb): DynamoDB SDK for Rust.
### Serialization
- [serde_with](https://crates.io/crates/serde-with): Used for custom deserialization. Used along with UUID in this case to deserialize the structs with new UUIDs.
- [serde](https://crates.io/crates/serde): Serde is a deserialization crate, it essentially parses structs into specific formats and vice versa. 
- [serde_json](https://crates.io/crates/serde-json): Specifically used for parsing json. (Such as the usage of the `json!` macro within [main.rs](src/main.rs).
### Errors
- [anyhow](https://crates.io/crates/anyhow): Error handling. One might want to use [`thiserror`](https://crates.io/crates/thiserror) in other instances.
### Requests
- [reqwest](https://crates.io/crates/reqwest): Send http requests.
- [futures-util](https://crates.io/crates/futures-util): Utilities for resolving futures.wq
- [url](https://crates.io/crates/url): Helper for URL parsing.
- [httparse](https://crates.io/crates/httparse): Parses url from request.
### Networking
- [tokio](https://crates.io/crates/tokio): Networking abstraction for Rust (somewhat like NodeJS for Javascript)
- [tokio-tungstenite](https://crates.io/crates/tokio-tungstenite): Superset of Tokio specifically utilized for Websockets.
### Misc
- [uuid](https://crates.io/crates/uuid): Generation of UUIDs for message deserialization.
- [dotenv](https://crates.io/crates/dotenv): Same as the npm version, parses the environment.

The versioning of each crate can be seen in the [Cargo.toml](Cargo.toml)

## Installation and Running the Application

### To run with cargo: 
1. **Clone the repository:**
   ```bash
   git clone [https://yourrepositoryurl.git](https://github.com/Safe-Corp-Jiva/chatting.git)
   cd chatting
   ```

2. **Build the application:**
   ```bash
   cargo build
   ```

3. **Run the application:**
   ```bash
   cargo run
   ```

   Ensure that your AWS credentials are properly configured as the application needs to connect to AWS DynamoDB.

### To run with Docker:
1. **Build the Docker image:**
```bash
docker build -t <desired-image-name>
```
2. **Run the Docker image**:
```bash
docker run -p 3030:3030 --env-file ./.env
```
> Note: Make sure you pass in the environment file, otherwise the server won't be able to connect properly to AWS.

## Configuration

The application requires the following environment variables to be set:
### AWS Variables:
- `AWS_ACCESS_KEY_ID`: AWS credentials access key.
- `AWS_SECRET_ACCESS_KEY`: AWS credentials secret key.
- `AWS_REGION`: The region where your DynamoDB tables are located.

> Alternatively, you can configure these in the `~/.aws/credentials` and `~/.aws/config` files.

### Misc Variables:
**This is only used if you have a copilot connected.** 
> If you only have human interactions, the server will gracefully let you know that it can´t find a copilot endpoint, but it won´t panic.
- `COPILOT_ENDPOINT`: The endpoint for the copilot instance.

### Amplify Variables: 
**These are only important if you want notification support.**
- `GRAPHQL_API_URL`: The URL of the GraphQL instance managed by your Amplify domain.
- `GRAPHQL_API_KEY`: The API Key for the GraphQL endpoint in the Amplify domain.


## Usage

Once the server is running, connect to the WebSocket endpoint using any WebSocket client on `ws://localhost:3030`. Ensure you include query parameters for `agent_id` and `secondary_id` when establishing a connection, for example:
```
ws://localhost:3030?agentID={agent_id}&secondaryID={"supervisor" | "copilot"}
```

The server is setup to detect `ConnectionType` from the parameter URL, and has different functionality based on connection type:

### Human Connections
- Broadcast messages to all users.
- Send notifications on message sent to db.

### Copilot Connections
- Sends a POST request to the `COPILOT_ENDPOINT` that is detected in the environment.
- Streams the response of the Copilot, and ensures that messages are sent in the proper format.
**IMPORTANT:** A copilot message must have the following structure:
```json
{
   "action":{action to be executed},
   "output":{part of the full message being streamed by the copilot}
}
```
If it doesn't, the server will not continue parsing, and might delete the connection (especially if it detects EOF within the streaming response).
