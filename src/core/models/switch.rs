use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchProfile {
    pub name: String,
    pub host: String,
    pub community_env: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocatedPort {
    pub switch: String,
    pub host: String,
    pub mac: String,
    pub vlan: Option<u32>,
    pub bridge_port: i64,
    pub if_index: i64,
    pub interface: String,
    pub alias: String,
    pub pvid: Option<i64>,
    pub speed_mbps: Option<i64>,
    pub oper_status: String,
}
