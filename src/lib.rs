use chrono::NaiveDateTime;
use reqwest::blocking::Client as HttpClient;
use reqwest::header::CONTENT_TYPE;
use semver::Version;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::time::Duration;
use std::panic::{self, PanicHookInfo};
use std::sync::Arc;

extern crate serde_json;

const API_ENDPOINT: &str = "https://us.i.posthog.com/capture/";
const TIMEOUT: &Duration = &Duration::from_millis(800); // This should be specified by the user

pub fn client<C: Into<ClientOptions>>(options: C) -> Client {
    let http_client = HttpClient::builder()
        .timeout(Some(*TIMEOUT))
        .build()
        .unwrap(); // Unwrap here is as safe as `HttpClient::new`
    let client = Client {
        options: options.into(),
        client: http_client,
    };

    if client.options.enable_panic_capturing {
        let panic_reporter_client = client.clone();
        let next = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let mut exception = exception_from_panic_info(info, &panic_reporter_client.options.default_distinct_id);
            if panic_reporter_client.options.on_panic_exception.is_some() {
                panic_reporter_client.options.on_panic_exception.as_ref().unwrap()(&mut exception)
            }
            let _  = panic_reporter_client.capture_exception(exception);
            next(info);
        }));
    }

    client
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Connection(msg) => write!(f, "Connection Error: {}", msg),
            Error::Serialization(msg) => write!(f, "Serialization Error: {}", msg),
            Error::Panic(msg) => write!(f, "Panic: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
pub enum Error {
    Connection(String),
    Serialization(String),
    Panic(String),
}

#[derive(Clone)]
pub struct ClientOptions {
    api_endpoint: String,
    api_key: String,
    default_distinct_id: String,
    enable_panic_capturing: bool,
    on_panic_exception: Option<Arc<dyn Fn(&mut Exception) + Send + Sync>>,
}

impl From<&str> for ClientOptions {
    fn from(api_key: &str) -> Self {
        ClientOptions {
            api_endpoint: API_ENDPOINT.to_string(),
            api_key: api_key.to_string(),
            default_distinct_id: uuid::Uuid::new_v4().to_string(),
            enable_panic_capturing: true,
            on_panic_exception: None,
        }
    }
}

impl ClientOptions {
    pub fn new<F>(api_key: &str, default_distinct_id: Option<&str>, enable_panic_capturing: bool, on_panic_exception: Option<F>) -> Self
    where F: Fn(&mut Exception) + Send + Sync + 'static
    {
        Self {
            api_endpoint: API_ENDPOINT.to_string(),
            api_key: api_key.to_string(),
            default_distinct_id: default_distinct_id.unwrap_or(&uuid::Uuid::new_v4().to_string()).to_string(),
            enable_panic_capturing,
            on_panic_exception: on_panic_exception.map(|cb| Arc::new(cb) as Arc<dyn Fn(&mut Exception) + Send + Sync>),
        }
    }
}

#[derive(Clone)]
pub struct Client {
    options: ClientOptions,
    client: HttpClient,
}

impl Client {
    pub fn capture(&self, event: Event) -> Result<(), Error> {
        let inner_event = InnerEvent::new(event, self.options.api_key.clone());
        let _res = self
            .client
            .post(self.options.api_endpoint.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(&inner_event).expect("unwrap here is safe"))
            .send()
            .map_err(|e| Error::Connection(e.to_string()))?;
        Ok(())
    }

    pub fn capture_batch(&self, events: Vec<Event>) -> Result<(), Error> {
        for event in events {
            self.capture(event)?;
        }
        Ok(())
    }

    pub fn capture_exception(&self, exception: Exception) -> Result<(), Error> {
        let event = exception.to_event();
        self.capture(event)
    }

    pub fn capture_exception_batch(&self, exceptions: Vec<Exception>) -> Result<(), Error> {
        for exception in exceptions {
            self.capture_exception(exception)?;
        }
        Ok(())
    }
}

// This exists so that the client doesn't have to specify the API key over and over
#[derive(Serialize)]
struct InnerEvent {
    api_key: String,
    event: String,
    properties: Properties,
    timestamp: Option<NaiveDateTime>,
}

impl InnerEvent {
    fn new(event: Event, api_key: String) -> Self {
        let mut properties = event.properties;

        // Add $lib_name and $lib_version to the properties
        properties.props.insert(
            "$lib_name".into(),
            serde_json::Value::String("posthog-rs".into()),
        );

        let version_str = env!("CARGO_PKG_VERSION");
        properties.props.insert(
            "$lib_version".into(),
            serde_json::Value::String(version_str.into()),
        );

        if let Ok(version) = version_str.parse::<Version>() {
            properties.props.insert(
                "$lib_version__major".into(),
                serde_json::Value::Number(version.major.into()),
            );
            properties.props.insert(
                "$lib_version__minor".into(),
                serde_json::Value::Number(version.minor.into()),
            );
            properties.props.insert(
                "$lib_version__patch".into(),
                serde_json::Value::Number(version.patch.into()),
            );
        }

        Self {
            api_key,
            event: event.event,
            properties,
            timestamp: event.timestamp,
        }
    }
}

pub trait EventBase {
    fn insert_prop<K: Into<String>, P: Serialize>(
        &mut self,
        key: K,
        prop: P,
    ) -> Result<(), Error>;
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Event {
    event: String,
    properties: Properties,
    timestamp: Option<NaiveDateTime>,
}

pub struct Exception {
    properties: Properties,
    timestamp: Option<NaiveDateTime>,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
pub struct Properties {
    distinct_id: String,
    props: HashMap<String, serde_json::Value>,
    #[serde(rename = "$lib")]
    lib: String,
    #[serde(rename = "$lib_version")]
    lib_version: String,
    #[serde(rename = "$os")]
    os: String,
    #[serde(rename = "$os_version")]
    os_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "$exception_level")]
    exception_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "$exception_list")]
    exception_list: Option<serde_json::Value>,
}

impl Properties {
    fn new<S: Into<String>>(distinct_id: S) -> Self {
        let os_info = os_info::get();
        Self {
            distinct_id: distinct_id.into(),
            props: Default::default(),
            lib: "posthog-rs".to_string(),
            lib_version: env!("CARGO_PKG_VERSION").to_string(),
            os: os_info.os_type().to_string(),
            os_version: os_info.version().to_string(),
            exception_level: None,
            exception_list: None,
        }
    }
}

impl Event {
    pub fn new<S: Into<String>>(event: S, distinct_id: S) -> Self {
        Self {
            event: event.into(),
            properties: Properties::new(distinct_id),
            timestamp: None,
        }
    }
}

impl EventBase for Event {
    /// Errors if `prop` fails to serialize
    fn insert_prop<K: Into<String>, P: Serialize>(
        &mut self,
        key: K,
        prop: P,
    ) -> Result<(), Error> {
        let as_json =
            serde_json::to_value(prop).map_err(|e| Error::Serialization(e.to_string()))?;
        let _ = self.properties.props.insert(key.into(), as_json);
        Ok(())
    }
}

impl Exception {
    pub fn new<S: Into<String>>(exception: &dyn std::error::Error, distinct_id: S) -> Self {
        Self {
            properties: Properties::new(distinct_id),
            timestamp: None,
        }
            .with_exception_level(Some("error".to_string()))
            .set_exception_list(exception)
    }

    pub fn with_exception_level(mut self, exception_level: Option<String>) -> Self {
        self.properties.exception_level = exception_level;
        self
    }
    
    fn set_exception_list(mut self, exception: &dyn std::error::Error) -> Self {
        let mut exception_info = serde_json::Map::new();
        exception_info.insert("type".into(), serde_json::Value::String(Exception::parse_exception_type(exception)));
        exception_info.insert("value".into(), serde_json::Value::String(exception.to_string()));
        let mut mechanism = serde_json::Map::new();
        mechanism.insert("handled".into(), serde_json::Value::Bool(true));
        mechanism.insert("synthetic".into(), serde_json::Value::Bool(false));
        exception_info.insert("mechanism".into(), serde_json::Value::Object(mechanism));

        //TODO: Parse and add stacktrace

        self.properties.exception_list = Some(serde_json::Value::Array(vec![serde_json::Value::Object(exception_info)]));
        self
    }

    fn parse_exception_type(exception: &dyn std::error::Error) -> String {
        let dbg = format!("{exception:?}");
        let value = exception.to_string();
    
        // A generic `anyhow::msg` will just `Debug::fmt` the `String` that you feed
        // it. Trying to parse the type name from that will result in a leading quote
        // and the first word, so quite useless.
        // To work around this, we check if the `Debug::fmt` of the complete error
        // matches its `Display::fmt`, in which case there is no type to parse and
        // we will just be using `Error`.
        let exception_type = if dbg == format!("{value:?}") {
            String::from("Error")
        } else {
            dbg.split(&[' ', '(', '{', '\r', '\n'][..])
                .next()
                .unwrap()
                .trim()
                .to_owned()
        };
        exception_type
    }

    pub fn to_event(&self) -> Event {
        let mut event = Event::new("$exception", &self.properties.distinct_id);
        event.timestamp = self.timestamp;
        event.properties = self.properties.clone();
        event
    }
}

impl EventBase for Exception {
    /// Errors if `prop` fails to serialize
    fn insert_prop<K: Into<String>, P: Serialize>(
        &mut self,
        key: K,
        prop: P,
    ) -> Result<(), Error> {
        let as_json =
            serde_json::to_value(prop).map_err(|e| Error::Serialization(e.to_string()))?;
        let _ = self.properties.props.insert(key.into(), as_json);
        Ok(())
    }
}

fn exception_from_panic_info(info: &PanicHookInfo<'_>, distinct_id: &String) -> Exception {
    let msg = message_from_panic_info(info);
    let error = Error::Panic(msg.into());
    Exception::new(&error, distinct_id)
}

pub fn message_from_panic_info<'a>(info: &'a PanicHookInfo<'_>) -> &'a str {
    match info.payload().downcast_ref::<&'static str>() {
        Some(s) => s,
        None => match info.payload().downcast_ref::<String>() {
            Some(s) => &s[..],
            None => "Box<Any>",
        },
    }
}

#[cfg(test)]
mod test_setup {
    use ctor::ctor;
    use dotenv::dotenv;

    #[ctor]
    fn load_dotenv() {
        dotenv().ok(); // Load the .env file
        println!("Loaded .env for tests");
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    // see https://us.posthog.com/project/115809/ for the e2e project

    #[test]
    fn inner_event_adds_lib_properties_correctly() {
        // Arrange
        let mut event = Event::new("unit test event", "1234");
        event.insert_prop("key1", "value1").unwrap();
        let api_key = "test_api_key".to_string();

        // Act
        let inner_event = InnerEvent::new(event, api_key);

        // Assert
        let props = &inner_event.properties.props;
        assert_eq!(
            props.get("$lib_name"),
            Some(&serde_json::Value::String("posthog-rs".to_string()))
        );
    }

    #[cfg(feature = "e2e-test")]
    #[test]
    fn get_client() {
        use std::collections::HashMap;

        let api_key = std::env::var("POSTHOG_RS_E2E_TEST_API_KEY").unwrap();
        let client = crate::client(api_key.as_str());

        let mut child_map = HashMap::new();
        child_map.insert("child_key1", "child_value1");

        let mut event = Event::new("e2e test event", "1234");
        event.insert_prop("key1", "value1").unwrap();
        event.insert_prop("key2", vec!["a", "b"]).unwrap();
        event.insert_prop("key3", child_map).unwrap();

        client.capture(event).unwrap();
    }
}
