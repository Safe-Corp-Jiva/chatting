# ChatWithDynamoDB

ChatWithDynamoDB is a simple chat application that utilizes WebSockets for real-time communication and AWS DynamoDB for storing and retrieving messages. This application is built with Rust and leverages asynchronous programming paradigms.

## Features

- Real-time chat communication using WebSockets.
- Storage and retrieval of chat messages from AWS DynamoDB.
- Full asynchronous operation using Tokio.

## Prerequisites

Before you begin, ensure you have met the following requirements:
- You have installed Rust and Cargo. Visit [Rust Getting Started](https://www.rust-lang.org/learn/get-started) for installation instructions.
- You have an AWS account and have configured AWS credentials on your machine. AWS credentials should have permissions to access DynamoDB services.

## Dependencies

This project uses the following crates:
- `aws-config`: Configuration loading for AWS SDK. 
- `aws-sdk-dynamodb`: AWS SDK for DynamoDB to manage database operations.
- `tokio`: Asynchronous runtime for Rust.
- `tokio-tungstenite`: WebSocket support for Tokio.
- `futures-util`: Utilities for working with futures and streams.
- `url`: URL parsing facilities.
- `httparse`: Parser for HTTP requests.

## Installation and Running the Application

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

## Configuration

The application requires the following environment variables to be set:
- `AWS_ACCESS_KEY_ID`: AWS credentials access key.
- `AWS_SECRET_ACCESS_KEY`: AWS credentials secret key.
- `AWS_REGION`: The region where your DynamoDB tables are located.

Alternatively, you can configure these in the `~/.aws/credentials` and `~/.aws/config` files.

## Usage

Once the server is running, connect to the WebSocket endpoint using any WebSocket client on `ws://localhost:3030`. Ensure you include query parameters for `call_id` and `owner` when establishing a connection, for example:
```
ws://localhost:3030?call_id=test_call&owner=user123
```

