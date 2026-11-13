//! GMAudio core: the asset library and the audio engine.
//!
//! Nothing in this crate knows about the UI. The Tauri shell forwards
//! commands in and events out.

pub mod analyze;
pub mod bundle;
pub mod db;
pub mod engine;
pub mod freesound;
pub mod error;
pub mod library;
pub mod model;
pub mod project;
pub mod search;
pub mod secrets;
pub mod tempo;
pub mod youtube;

pub use error::{Error, Result};
