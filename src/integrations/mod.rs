// src/integrations/mod.rs — App integration layer

pub mod credentials;
pub mod discord;
pub mod email;
pub mod google_docs;
pub mod google_sheets;
pub mod imessage;
pub mod msoffice;
pub mod msteams;
pub mod notion;
pub mod registry;
pub mod slack;
pub mod telegram;
pub mod tools;
pub mod types;
pub mod watcher;

pub use credentials::IntegrationCredentials;
pub use types::{DocumentAdapter, IncomingMessage, MessagingAdapter};
