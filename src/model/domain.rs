use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DomainStatus {
    pub hostname: String,
    pub domain: String,
    pub joined: Option<bool>,
    pub source: String,
    pub confidence: String,
    pub logon_server: Option<String>,
}
