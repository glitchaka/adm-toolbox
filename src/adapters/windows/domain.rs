use std::{
    env,
    net::{IpAddr, ToSocketAddrs},
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
        native_status(None)
    }

    fn remote_status(&self, host: &str, verify: bool) -> Result<DomainStatus> {
        validate_host(host)?;

        if verify {
            native_status(Some(host))
        } else {
            inferred_remote_status(host)
        }
    }
}

#[cfg(windows)]
fn native_status(target: Option<&str>) -> Result<DomainStatus> {
    use windows_sys::Win32::NetworkManagement::NetManagement::{
        NetGetJoinInformation, NetApiBufferFree, NetSetupDomainName, NetSetupUnknownStatus,
    };
    let server = target.map(|host| host.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
    let mut name = std::ptr::null_mut();
    let mut status = NetSetupUnknownStatus;
    // NetGetJoinInformation allocates the UTF-16 buffer; free it after copying.
    unsafe {
        let error = NetGetJoinInformation(server.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()), &mut name, &mut status);
        if error != 0 { anyhow::bail!("Consulta de dominio: {}", std::io::Error::from_raw_os_error(error as i32)); }
        let domain = if name.is_null() { String::new() } else {
            let mut len = 0;
            while *name.add(len) != 0 { len += 1; }
            let domain = String::from_utf16_lossy(std::slice::from_raw_parts(name, len));
            NetApiBufferFree(name.cast());
            domain
        };
        Ok(DomainStatus {
            hostname: target.map(str::to_owned).unwrap_or_else(|| env::var("COMPUTERNAME").unwrap_or_default()),
            domain, joined: if status == NetSetupUnknownStatus { None } else { Some(status == NetSetupDomainName) },
            source: "windows-netapi".to_owned(), confidence: "confirmed".to_owned(),
            logon_server: if target.is_none() { env::var("LOGONSERVER").ok() } else { None },
        })
    }
}

#[cfg(not(windows))]
fn native_status(_target: Option<&str>) -> Result<DomainStatus> { anyhow::bail!("La consulta de dominio requiere Windows") }

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
        confidence: "inferred; use --verify for Windows API confirmation".to_owned(),
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
