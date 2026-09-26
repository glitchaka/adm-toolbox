use std::{env, sync::Arc};

use anyhow::Result;

use crate::core::{CommandOutput, ports::DomainProbe};

pub struct DomainService {
    probe: Arc<dyn DomainProbe>,
}

impl DomainService {
    pub fn new(probe: Arc<dyn DomainProbe>) -> Self {
        Self { probe }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        match args.first().map(String::as_str).unwrap_or("status") {
            "status" => self.status(&args[1..]),
            other => Ok(CommandOutput::error(
                format!("domain: subcomando desconocido: {other}"),
                2,
            )),
        }
    }

    fn status(&self, args: &[String]) -> Result<CommandOutput> {
        let target = args.iter().find(|arg| !arg.starts_with('-')).map(String::as_str);
        let verify = args.iter().any(|arg| arg == "--verify" || arg == "--cim");
        let json = args.iter().any(|arg| arg == "--json");

        let local_name = env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_owned());
        let is_local = target.is_none_or(|target| {
            target.eq_ignore_ascii_case(&local_name)
                || target.eq_ignore_ascii_case("localhost")
                || target == "127.0.0.1"
                || target == "::1"
        });

        let result = if is_local {
            self.probe.local_status()?
        } else {
            self.probe.remote_status(target.unwrap(), verify)?
        };

        if json {
            return Ok(CommandOutput::ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&result)?
            )));
        }

        Ok(CommandOutput::ok(format!(
            "hostname: {}\ndomain: {}\njoined: {}\nsource: {}\nconfidence: {}\nlogon_server: {}\n",
            result.hostname,
            result.domain,
            result.joined.map(|value| if value { "yes" } else { "no" }).unwrap_or("unknown"),
            result.source,
            result.confidence,
            result.logon_server.as_deref().unwrap_or("-"),
        )))
    }
}
