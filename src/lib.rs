mod client;
mod error;
mod event;

const API_ENDPOINT: &str = "https://us.i.posthog.com/capture/";

// Public interface - any change to this is breaking!
// Client
pub use client::client;
pub use client::Client;
pub use client::ClientOptions;
pub use client::ClientOptionsBuilder;

// Error
pub use error::Error;

// EventBase
pub use event::EventBase;

// Event
pub use event::Event;

// Exception
pub use event::Exception;