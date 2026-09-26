use std::{
    env,
    net::{IpAddr, ToSocketAddrs},
    process::Command,
};

use anyhow::Result;
use dns_lookup::lookup_addr;

use crate::core::{
    models::domain::DomainStatus,
    ports::DomainProbe,
};

pub struct WindowsDomainProbe;

impl DomainProbe for WindowsDomainProbe {
    fn local_status(&self) -> Result<DomainStatus> {
        cim_status(None).or_else(|_| Ok(local_environment_status()))
    }

    fn remote_status(&self, host: &str, verify: bool) -> Result<DomainStatus> {
        validate_host(host)?;

        if verify {
            cim_status(Some(host))
        } else {
            inferred_remote_status(host)
        }
    }
}

fn cim_status(target: Option<&str>) -> Result<DomainStatus> {
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
        source: if target.is_some() { "remote-cim" } else { "local-cim" }.to_owned(),
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
        domain: if dns_domain.is_empty() { user_domain } else { dns_domain },
        joined: Some(joined_guess),
        source: "local-environment".to_owned(),
        confidence: "inferred".to_owned(),
        logon_server: env::var("LOGONSERVER").ok().filter(|value| !value.is_empty()),
    }
}

fn inferred_remote_status(target: &str) -> Result<DomainStatus> {
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
    (!suffix.is_empty()).then(|| suffix.to_owned())
}

fn validate_host(host: &str) -> Result<()> {
    if host.is_empty()
        || host
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '_')))
    {
        anyhow::bail!("host inválido: {host}");
    }

    Ok(())
}
