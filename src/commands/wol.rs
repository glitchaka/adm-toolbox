use std::net::UdpSocket;

use super::{CommandOutput, device};

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(mac_text) = args.first() else {
        return Ok(CommandOutput::error(
            "wol: uso: wol AA:BB:CC:DD:EE:FF [BROADCAST]",
            2,
        ));
    };

    let resolved_mac = match parse_mac(mac_text) {
        Ok(mac) => (mac, mac_text.to_ascii_uppercase()),
        Err(_) => {
            let Some(stored) = device::resolve_name_to_mac(mac_text)? else {
                return Ok(CommandOutput::error(
                    format!("wol: no es una MAC válida ni un equipo inventariado: {mac_text}"),
                    2,
                ));
            };
            (parse_mac(&stored)?, stored)
        }
    };
    let (mac, display_mac) = resolved_mac;

    let broadcast = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("255.255.255.255:9");

    let target = if broadcast.contains(':') {
        broadcast.to_owned()
    } else {
        format!("{broadcast}:9")
    };

    let mut packet = [0_u8; 102];
    packet[..6].fill(0xFF);

    for index in 0..16 {
        let start = 6 + index * 6;
        packet[start..start + 6].copy_from_slice(&mac);
    }

    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    socket.send_to(&packet, &target)?;

    Ok(CommandOutput::ok(format!(
        "magic packet sent to {} via {}\n",
        display_mac,
        target
    )))
}

fn parse_mac(value: &str) -> anyhow::Result<[u8; 6]> {
    let normalized = value.replace('-', ":");
    let parts: Vec<&str> = normalized.split(':').collect();

    if parts.len() != 6 {
        anyhow::bail!("MAC inválida: {value}");
    }

    let mut mac = [0_u8; 6];

    for (index, part) in parts.iter().enumerate() {
        mac[index] = u8::from_str_radix(part, 16)
            .map_err(|_| anyhow::anyhow!("MAC inválida: {value}"))?;
    }

    Ok(mac)
}
