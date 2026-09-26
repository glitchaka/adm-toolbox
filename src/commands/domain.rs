use std::{
    env,
    net::{IpAddr, ToSocketAddrs},
    process::Command,
};

use dns_lookup::lookup_addr;
use serde::Serialize;

use super::CommandOutput;

#[derive(Debug, Clone, Serialize)]
struct DomainStatus {
    hostname: String,
    domain: String,
    joined: Option<bool>,
    source: String,
    confidence: String,
    logon_server: Option<String>,
}

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    let sub = args.first().map(String::as_str).unwrap_or("status");

    match sub {
        "status" => status(&args[1..]),
        _ => Ok(CommandOutput::error(
            format!("domain: subcomando desconocido: {sub}"),
            2,
        )),
    }
}

fn status(args: &[String]) -> anyhow::Result<CommandOutput> {
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
        cim_status(None).unwrap_or_else(|_| local_environment_status())
    } else if verify {
        let target = target.unwrap();
        validate_host(target)?;
        cim_status(Some(target))?
    } else {
        inferred_remote_status(target.unwrap())?
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
        result
            .joined
            .map(|value| if value { "yes" } else { "no" })
            .unwrap_or("unknown"),
        result.source,
        result.confidence,
        result.logon_server.as_deref().unwrap_or("-"),
    )))
}

fn cim_status(target: Option<&str>) -> anyhow::Result<DomainStatus> {
    let command = if let Some(target) = target {
        format!(
            "$r=Get-CimInstance Win32_ComputerSystem -ComputerName '{target}' -ErrorAction Stop; $r | Select-Object Name,Domain,PartOfDomain | ConvertTo-Json -Compress"
        )
    } else {
        "$r=Get-CimInstance Win32_ComputerSystem -ErrorAction Stop; $r | Select-Object Name,Domain,PartOfDomain | ConvertTo-Json -Compress".to_owned()
    };

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "CIM no disponible: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let hostname = value
        .get("Name")
        .and_then(|value| value.as_str())
        .unwrap_or("desconocido")
        .to_owned();
    let domain = value
        .get("Domain")
        .and_then(|value| value.as_str())
        .unwrap_or("desconocido")
        .to_owned();
    let joined = value.get("PartOfDomain").and_then(|value| value.as_bool());

    Ok(DomainStatus {
        hostname,
        domain,
        joined,
        source: if target.is_some() {
            "remote-cim".to_owned()
        } else {
            "local-cim".to_owned()
        },
        confidence: "confirmed".to_owned(),
        logon_server: if target.is_none() {
            env::var("LOGONSERVER").ok().filter(|value| !value.is_empty())
        } else {
            None
        },
    })
}

fn local_environment_status() -> DomainStatus {
    let computer = env::var("COMPUTERNAME").unwrap_or_else(|_| "desconocido".to_owned());
    let user_domain = env::var("USERDOMAIN").unwrap_or_else(|_| "desconocido".to_owned());
    let dns_domain = env::var("USERDNSDOMAIN").unwrap_or_default();

    let joined_guess = !user_domain.eq_ignore_ascii_case(&computer)
        && !user_domain.eq_ignore_ascii_case("WORKGROUP")
        && user_domain != "desconocido";

    DomainStatus {
        hostname: computer,
        domain: if dns_domain.is_empty() {
            user_domain
        } else {
            dns_domain
        },
        joined: Some(joined_guess),
        source: "local-environment".to_owned(),
        confidence: "inferred".to_owned(),
        logon_server: env::var("LOGONSERVER").ok().filter(|value| !value.is_empty()),
    }
}

fn inferred_remote_status(target: &str) -> anyhow::Result<DomainStatus> {
    validate_host(target)?;

    let (hostname, domain) = if let Ok(ip) = target.parse::<IpAddr>() {
        let hostname = lookup_addr(&ip).unwrap_or_else(|_| target.to_owned());
        let domain = domain_from_fqdn(&hostname);
        (hostname, domain)
    } else {
        let mut addresses: Vec<IpAddr> = (target, 0)
            .to_socket_addrs()?
            .map(|socket| socket.ip())
            .collect();
        addresses.sort();
        addresses.dedup();

        let fqdn = addresses
            .first()
            .and_then(|ip| lookup_addr(ip).ok())
            .unwrap_or_else(|| target.to_owned());

        let domain = domain_from_fqdn(&fqdn);
        (fqdn, domain)
    };

    Ok(DomainStatus {
        hostname,
        domain: domain.unwrap_or_else(|| "desconocido".to_owned()),
        joined: None,
        source: "dns".to_owned(),
        confidence: "inferred; use --verify for CIM confirmation".to_owned(),
        logon_server: None,
    })
}

fn domain_from_fqdn(hostname: &str) -> Option<String> {
    let (_, suffix) = hostname.split_once('.')?;
    if suffix.is_empty() {
        None
    } else {
        Some(suffix.to_owned())
    }
}

fn validate_host(host: &str) -> anyhow::Result<()> {
    if host.is_empty()
        || host
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '_')))
    {
        anyhow::bail!("host inválido: {host}");
    }

    Ok(())
}
