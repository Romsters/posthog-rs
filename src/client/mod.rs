use crate::API_ENDPOINT;
use crate::Exception;
use derive_builder::Builder;
use std::sync::Arc;
use std::panic::PanicHookInfo;
use std::fmt::{Display, Formatter};

#[cfg(not(feature = "async-client"))]
mod blocking;
#[cfg(not(feature = "async-client"))]
pub use blocking::client;
#[cfg(not(feature = "async-client"))]
pub use blocking::Client;

#[cfg(feature = "async-client")]
mod async_client;
#[cfg(feature = "async-client")]
pub use async_client::client;
#[cfg(feature = "async-client")]
pub use async_client::Client;

#[derive(Builder, Clone)]
pub struct ClientOptions {
    #[builder(default = "API_ENDPOINT.to_string()")]
    api_endpoint: String,
    api_key: String,

    #[builder(default = "30")]
    request_timeout_seconds: u64,

    #[builder(default = "uuid::Uuid::new_v4().to_string()")]
    default_distinct_id: String,
    #[builder(default = "true")]
    enable_panic_capturing: bool,
    on_panic_exception: Option<Arc<dyn Fn(&mut Exception) + Send + Sync>>,
}

impl From<&str> for ClientOptions {
    fn from(api_key: &str) -> Self {
        ClientOptionsBuilder::default()
            .api_key(api_key.to_string())
            .build()
            .expect("We always set the API key, so this is infallible")
    }
}

fn exception_from_panic_info(info: &PanicHookInfo<'_>, distinct_id: &String) -> Exception {
    let msg = message_from_panic_info(info);
    let error = SyntheticError::Panic(msg.into());
    Exception::new(&error, distinct_id)
}

fn message_from_panic_info<'a>(info: &'a PanicHookInfo<'_>) -> &'a str {
    match info.payload().downcast_ref::<&'static str>() {
        Some(s) => s,
        None => match info.payload().downcast_ref::<String>() {
            Some(s) => &s[..],
            None => "Box<Any>",
        },
    }
}

impl Display for SyntheticError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SyntheticError::Panic(msg) => write!(f, "Panic: {}", msg),
        }
    }
}

impl std::error::Error for SyntheticError {}

#[derive(Debug)]
pub enum SyntheticError {
    Panic(String),
}
