// src/soul/mod.rs — Soul loading + injection

pub mod evolution;
pub mod loader;
pub mod sovereign;

pub use loader::{load_soul, Soul, SoulSource};
