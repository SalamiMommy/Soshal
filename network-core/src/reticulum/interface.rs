//! Reticulum physical & logical transport interfaces (UDP, Auto Multicast, RNode Serial).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReticulumInterfaceKind {
    UdpUnicast,
    UdpMulticast,
    RNodeSerial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReticulumInterfaceStatus {
    pub name: String,
    pub kind: ReticulumInterfaceKind,
    pub bind_address: String,
    pub active: bool,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

impl ReticulumInterfaceStatus {
    pub fn new_udp(bind: &str, multicast: bool) -> Self {
        Self {
            name: if multicast {
                "UDP Multicast (Auto)".to_string()
            } else {
                "UDP Socket".to_string()
            },
            kind: if multicast {
                ReticulumInterfaceKind::UdpMulticast
            } else {
                ReticulumInterfaceKind::UdpUnicast
            },
            bind_address: bind.to_string(),
            active: true,
            rx_packets: 0,
            tx_packets: 0,
        }
    }

    pub fn new_rnode(port: &str) -> Self {
        Self {
            name: format!("RNode Serial ({port})"),
            kind: ReticulumInterfaceKind::RNodeSerial,
            bind_address: port.to_string(),
            active: true,
            rx_packets: 0,
            tx_packets: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_udp_unicast() {
        let iface = ReticulumInterfaceStatus::new_udp("0.0.0.0:4965", false);
        assert_eq!(iface.name, "UDP Socket");
        assert_eq!(iface.kind, ReticulumInterfaceKind::UdpUnicast);
        assert_eq!(iface.bind_address, "0.0.0.0:4965");
        assert!(iface.active);
        assert_eq!((iface.rx_packets, iface.tx_packets), (0, 0));
    }

    #[test]
    fn test_new_udp_multicast() {
        let iface = ReticulumInterfaceStatus::new_udp("0.0.0.0:4965", true);
        assert_eq!(iface.name, "UDP Multicast (Auto)");
        assert_eq!(iface.kind, ReticulumInterfaceKind::UdpMulticast);
        assert_eq!(iface.bind_address, "0.0.0.0:4965");
        assert!(iface.active);
        assert_eq!((iface.rx_packets, iface.tx_packets), (0, 0));
    }

    #[test]
    fn test_new_rnode() {
        let iface = ReticulumInterfaceStatus::new_rnode("/dev/ttyUSB0");
        assert_eq!(iface.name, "RNode Serial (/dev/ttyUSB0)");
        assert_eq!(iface.kind, ReticulumInterfaceKind::RNodeSerial);
        assert_eq!(iface.bind_address, "/dev/ttyUSB0");
        assert!(iface.active);
        assert_eq!((iface.rx_packets, iface.tx_packets), (0, 0));
    }
}
