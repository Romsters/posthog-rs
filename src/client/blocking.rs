use std::time::Duration;
use std::panic;

use reqwest::{blocking::Client as HttpClient, header::CONTENT_TYPE};

use crate::{event::InnerEvent, Error, Event, Exception};

use super::{ClientOptions, exception_from_panic_info};

#[derive(Clone)]
pub struct Client {
    options: ClientOptions,
    client: HttpClient,
}

impl Client {
    pub fn capture(&self, event: Event) -> Result<(), Error> {
        let inner_event = InnerEvent::new(event, self.options.api_key.clone());

        let payload =
            serde_json::to_string(&inner_event).map_err(|e| Error::Serialization(e.to_string()))?;

        self.client
            .post(&self.options.api_endpoint)
            .header(CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .map_err(|e| Error::Connection(e.to_string()))?;

        Ok(())
    }

    pub fn capture_batch(&self, events: Vec<Event>) -> Result<(), Error> {
        let events: Vec<_> = events
            .into_iter()
            .map(|event| InnerEvent::new(event, self.options.api_key.clone()))
            .collect();

        let payload =
            serde_json::to_string(&events).map_err(|e| Error::Serialization(e.to_string()))?;

        self.client
            .post(&self.options.api_endpoint)
            .header(CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .map_err(|e| Error::Connection(e.to_string()))?;

        Ok(())
    }

    pub fn capture_exception(&self, exception: Exception) -> Result<(), Error> {
        let event = exception.to_event();
        self.capture(event)
    }

    pub fn capture_exception_batch(&self, exceptions: Vec<Exception>) -> Result<(), Error> {
        let events: Vec<_> = exceptions
            .into_iter()
            .map(|exception| exception.to_event())
            .collect();
        self.capture_batch(events)
    }
}

pub fn client<C: Into<ClientOptions>>(options: C) -> Client {
    let options = options.into();
    let http_client = HttpClient::builder()
        .timeout(Duration::from_secs(options.request_timeout_seconds))
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
