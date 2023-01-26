use flutter_rust_bridge::SyncReturn;

pub fn hello() -> SyncReturn<String> {
    SyncReturn("hello!".to_string())
}

pub fn hello_async() -> String {
    "hello!".to_string()
}
