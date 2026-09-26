use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ScanRow {
    pub ip: Ipv4Addr,
    pub mac: String,
    pub hostname: String,
    pub latency_ms: u128,
    pub known: bool,
    pub inventory_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrafficRow {
    pub pid: u32,
    pub process: String,
    pub path: String,
    pub cpu_percent: f32,
    pub memory_mib: f64,
    pub connections: usize,
    pub upload_bps: u64,
    pub download_bps: u64,
    pub ppid: Option<u32>,
    pub foreground: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionRow {
    pub protocol: String,
    pub local: String,
    pub remote: String,
    pub state: String,
    pub pid: u32,
    pub process: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceRecord {
    pub id: String,
    pub mac: String,
    pub ip: String,
    pub hostname: String,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkProvider {
    pub name: String,
    pub kind: String,
    pub host: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ByteCounters {
    pub sent: u64,
    pub received: u64,
}
