use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub mac: String,
    pub name: String,
    pub notes: Option<String>,
}
