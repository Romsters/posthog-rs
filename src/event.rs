use std::collections::HashMap;

use chrono::NaiveDateTime;
use semver::Version;
use serde::Serialize;

use crate::Error;

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

#[derive(Serialize, Debug, PartialEq, Eq)]
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
            lib: env!("CARGO_PKG_NAME").to_string(),
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

// This exists so that the client doesn't have to specify the API key over and over
#[derive(Serialize)]
pub struct InnerEvent {
    api_key: String,
    event: String,
    properties: Properties,
    timestamp: Option<NaiveDateTime>,
}

impl InnerEvent {
    pub fn new(event: Event, api_key: String) -> Self {
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

#[cfg(test)]
pub mod tests {
    use crate::{event::InnerEvent, Event, EventBase};

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
}
