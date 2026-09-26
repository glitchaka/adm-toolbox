use std::{env, net::SocketAddr, time::Duration};

use anyhow::Result;
use snmp2::{Oid, SyncSession, Value};

use crate::model::{
    ports::SwitchLocator,
    switch::{LocatedPort, SwitchProfile},
};

pub struct SnmpSwitchLocator;

impl SwitchLocator for SnmpSwitchLocator {
    fn locate(
        &self,
        profile: &SwitchProfile,
        mac: [u8; 6],
        mac_text: &str,
        vlan: Option<u32>,
    ) -> Result<Option<LocatedPort>> {
        let community = env::var(&profile.community_env).map_err(|_| {
            anyhow::anyhow!(
                "falta la variable de entorno {} con la comunidad SNMP",
                profile.community_env
            )
        })?;

        let address = snmp_address(&profile.host);
        let mut session = SyncSession::new_v2c(
            address.as_str(),
            community.as_bytes(),
            Some(Duration::from_secs(2)),
            0,
        )
        .map_err(|error| anyhow::anyhow!("SNMP: {error}"))?;

        let bridge_port = if let Some(vlan) = vlan {
            let mut oid = vec![1_u64, 3, 6, 1, 2, 1, 17, 7, 1, 2, 2, 1, 2, vlan as u64];
            oid.extend(mac.iter().map(|byte| *byte as u64));
            snmp_get_integer(&mut session, &oid)?
        } else {
            let mut oid = vec![1_u64, 3, 6, 1, 2, 1, 17, 4, 3, 1, 2];
            oid.extend(mac.iter().map(|byte| *byte as u64));
            snmp_get_integer(&mut session, &oid)?
        };

        let Some(bridge_port) = bridge_port else {
            return Ok(None);
        };

        let if_index = snmp_get_integer(
            &mut session,
            &[1, 3, 6, 1, 2, 1, 17, 1, 4, 1, 2, bridge_port as u64],
        )?
        .unwrap_or(bridge_port);

        let interface = snmp_get_string(
            &mut session,
            &[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 1, if_index as u64],
        )?
        .or_else(|| {
            snmp_get_string(
                &mut session,
                &[1, 3, 6, 1, 2, 1, 2, 2, 1, 2, if_index as u64],
            )
            .ok()
            .flatten()
        })
        .unwrap_or_else(|| format!("ifIndex {if_index}"));

        let alias = snmp_get_string(
            &mut session,
            &[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 18, if_index as u64],
        )?
        .unwrap_or_default();

        let pvid = snmp_get_integer(
            &mut session,
            &[1, 3, 6, 1, 2, 1, 17, 7, 1, 4, 5, 1, 1, bridge_port as u64],
        )
        .ok()
        .flatten();

        let speed_mbps = snmp_get_integer(
            &mut session,
            &[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 15, if_index as u64],
        )
        .ok()
        .flatten();

        let oper_status = match snmp_get_integer(
            &mut session,
            &[1, 3, 6, 1, 2, 1, 2, 2, 1, 8, if_index as u64],
        )
        .ok()
        .flatten()
        {
            Some(1) => "up".to_owned(),
            Some(2) => "down".to_owned(),
            Some(3) => "testing".to_owned(),
            Some(4) => "unknown".to_owned(),
            Some(5) => "dormant".to_owned(),
            Some(6) => "notPresent".to_owned(),
            Some(7) => "lowerLayerDown".to_owned(),
            Some(value) => value.to_string(),
            None => "-".to_owned(),
        };

        Ok(Some(LocatedPort {
            switch: profile.name.clone(),
            host: profile.host.clone(),
            mac: mac_text.to_owned(),
            vlan,
            bridge_port,
            if_index,
            interface,
            alias,
            pvid,
            speed_mbps,
            oper_status,
        }))
    }

    fn capabilities(&self) -> &'static str {
        "provider: snmp2-rust\nprotocol: SNMPv2c\nmode: read-only\nbridge-mib: yes\nq-bridge-mib: VLAN lookup when --vlan is supplied\nif-mib: interface name, alias, speed, status\ncredentials: environment variable reference\n"
    }
}

fn snmp_get_integer(session: &mut SyncSession, components: &[u64]) -> Result<Option<i64>> {
    let oid = Oid::from(components)
        .map_err(|error| anyhow::anyhow!("OID inválido: {error:?}"))?;
    let mut response = session
        .get(&oid)
        .map_err(|error| anyhow::anyhow!("SNMP GET: {error}"))?;

    let Some((_name, value)) = response.varbinds.next() else {
        return Ok(None);
    };

    match value {
        Value::Integer(value) => Ok(Some(value)),
        Value::Unsigned32(value) => Ok(Some(value as i64)),
        Value::Counter32(value) => Ok(Some(value as i64)),
        Value::NoSuchInstance | Value::NoSuchObject | Value::EndOfMibView => Ok(None),
        other => Err(anyhow::anyhow!("SNMP devolvió un tipo inesperado: {other:?}")),
    }
}

fn snmp_get_string(session: &mut SyncSession, components: &[u64]) -> Result<Option<String>> {
    let oid = Oid::from(components)
        .map_err(|error| anyhow::anyhow!("OID inválido: {error:?}"))?;
    let mut response = session
        .get(&oid)
        .map_err(|error| anyhow::anyhow!("SNMP GET: {error}"))?;

    let Some((_name, value)) = response.varbinds.next() else {
        return Ok(None);
    };

    match value {
        Value::OctetString(value) => Ok(Some(String::from_utf8_lossy(value).into_owned())),
        Value::NoSuchInstance | Value::NoSuchObject | Value::EndOfMibView => Ok(None),
        other => Ok(Some(format!("{other:?}"))),
    }
}

fn snmp_address(host: &str) -> String {
    if host.parse::<SocketAddr>().is_ok() {
        host.to_owned()
    } else if host.contains(':') {
        format!("[{host}]:161")
    } else {
        format!("{host}:161")
    }
}
