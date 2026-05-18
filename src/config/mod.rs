mod app_config;
pub mod binance_config;
mod runtime;

use std::sync::Arc;

pub use app_config::AppConfig;
pub use runtime::*;

pub type SharedConfig = Arc<AppConfig>;
