use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct FngResponse {
    pub name: String,
    pub data: Vec<FngData>,
}

#[derive(Debug, Deserialize)]
pub struct FngData {
    pub value: String,
    #[serde(rename = "value_classification")]
    pub status: FngStatus,
    pub timestamp: String,
    pub time_until_update: String,
}

#[derive(Debug, Deserialize)]
pub enum FngStatus {
    #[serde(rename = "Extreme Fear")]
    ExtremeFear,
    #[serde(rename = "Fear")]
    Fear,
    #[serde(rename = "Neutral")]
    Neutral,
    #[serde(rename = "Greed")]
    Greed,
    #[serde(rename = "Extreme Greed")]
    ExtremeGreed,
}

impl FngStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            FngStatus::ExtremeFear => "Extreme Fear",
            FngStatus::Fear => "Fear",
            FngStatus::Neutral => "Neutral",
            FngStatus::Greed => "Greed",
            FngStatus::ExtremeGreed => "Extreme Greed",
        }
    }
}
