use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, instrument, trace, warn};
use ts_rs::TS;

use crate::error::BackendError;

#[derive(Debug, Serialize, Deserialize, TS, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[instrument(skip_all, name = "js")]
#[tauri::command]
pub async fn log_js(message: String, level: Option<Level>) -> Result<(), BackendError> {
    match level {
        Some(Level::Trace) => trace!(message),
        Some(Level::Debug) => debug!(message),
        Some(Level::Info) => info!(message),
        Some(Level::Warn) => warn!(message),
        Some(Level::Error) => error!(message),
        None => info!(message),
    }

    Ok(())
}
