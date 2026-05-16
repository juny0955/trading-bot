pub use crate::types::{FngData, FngStatus};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct FngResponse {
    pub name: String,
    pub data: Vec<FngData>,
}
