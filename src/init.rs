use aws_sdk_dynamodb::Client;

pub struct ChatClient {
    client: Client,
    owner: String,
    call_id: String,
}

impl ChatClient {
    pub fn new(c: Client, o: String, c_id: String) -> Self {
        Self {
            client: c,
            owner: o,
            call_id: c_id,
        }
    }
}
