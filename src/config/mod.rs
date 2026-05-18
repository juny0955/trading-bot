mod app_config;
mod runtime;

use std::sync::Arc;

pub use crate::binance::config::*;
pub use app_config::AppConfig;
pub use runtime::*;

pub type SharedConfig = Arc<AppConfig>;
