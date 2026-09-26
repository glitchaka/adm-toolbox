use std::net::UdpSocket;

use anyhow::Result;

use crate::core::ports::WakeOnLanSender;

pub struct UdpWakeOnLanSender;

impl WakeOnLanSender for UdpWakeOnLanSender {
    fn send(&self, mac: [u8; 6], broadcast: &str) -> Result<()> {
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
        Ok(())
    }
}
