//! Bluetooth (SPP) support: MAC address parsing in Windows registry byte order,
//! and turning Windows registry rows into SPP COM ports.

use std::fmt;

/// Serial Port Profile service GUID: the prefix of an SPP port's registry key.
const SPP_SERVICE_GUID: &str = "{00001101-0000-1000-8000-00805f9b34fb}";

/// A 6-octet Bluetooth address.
///
/// Index 0 is the first octet as printed, i.e. the most significant byte.
/// Windows keeps the address as a 48-bit big-endian value, so
/// `70:4B:CA:72:AC:A2` is stored as `0x704BCA72ACA2` — the most significant
/// byte `0x70` comes first, not byte-reversed to `0xA2AC72CA4B70`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mac([u8; 6]);

impl Mac {
    /// Parses `70:4B:CA:72:AC:A2`, `70:4b:ca:72:ac:a2` or `704BCA72ACA2`.
    pub fn parse(text: &str) -> Option<Mac> {
        let mut bytes = [0u8; 6];
        let parts: Vec<&str> = if text.contains(':') {
            text.split(':').collect()
        } else {
            if text.len() != 12 {
                return None;
            }
            text.as_bytes()
                .chunks(2)
                .map(|chunk| std::str::from_utf8(chunk).ok())
                .collect::<Option<Vec<&str>>>()?
        };
        if parts.len() != 6 {
            return None;
        }
        for (slot, part) in bytes.iter_mut().zip(parts) {
            if part.len() != 2 || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            *slot = u8::from_str_radix(part, 16).ok()?;
        }
        Some(Mac(bytes))
    }

    /// Builds the address the way the Windows registry stores it.
    pub fn from_ull(value: u64) -> Mac {
        let mut bytes = [0u8; 6];
        for (i, slot) in bytes.iter_mut().enumerate() {
            *slot = ((value >> (8 * (5 - i))) & 0xFF) as u8;
        }
        Mac(bytes)
    }

    /// Returns the address the way the Windows registry stores it.
    pub fn to_ull(self) -> u64 {
        let mut value = 0u64;
        for byte in self.0 {
            value = (value << 8) | byte as u64;
        }
        value
    }

    /// True for `00:00:00:00:00:00`, the marker a ghost port carries.
    pub fn is_zero(self) -> bool {
        self.0 == [0u8; 6]
    }
}

impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

/// One row of the Bluetooth SPP enumeration in the Windows registry.
///
/// `key` is the service key name (service GUID plus vendor suffix in the same
/// name), `instance` the adapter/device instance below it, and `port_name` the
/// COM port assigned in `Device Parameters\PortName`.
pub struct RegistryInstance {
    pub key: String,
    pub instance: String,
    pub port_name: String,
}

/// An SPP COM port together with the peer address it is bound to.
pub struct SppPort {
    pub port_name: String,
    pub mac: Mac,
    pub instance: String,
}

/// Extracts the peer MAC embedded in a registry instance name such as
/// `a&2e6f9637&0&704BCA72ACA2_C00000000`.
pub fn mac_from_instance(instance: &str) -> Option<Mac> {
    instance.split('&').find_map(|segment| {
        let head = segment.split_once('_').map_or(segment, |(head, _)| head);
        if head.len() == 12 && head.bytes().all(|b| b.is_ascii_hexdigit()) {
            Mac::parse(head)
        } else {
            None
        }
    })
}

/// Keeps the SPP service rows and returns them sorted by port name.
pub fn spp_ports_from_instances(instances: &[RegistryInstance]) -> Vec<SppPort> {
    let guid = SPP_SERVICE_GUID.as_bytes();
    let mut ports: Vec<SppPort> = instances
        .iter()
        .filter(|row| {
            row.key.len() >= guid.len()
                && row.key.as_bytes()[..guid.len()].eq_ignore_ascii_case(guid)
        })
        .filter_map(|row| {
            let mac = mac_from_instance(&row.instance)?;
            Some(SppPort {
                port_name: row.port_name.clone(),
                mac,
                instance: row.instance.clone(),
            })
        })
        .collect();
    ports.sort_by(|a, b| a.port_name.cmp(&b.port_name));
    ports
}

/// Picks the port bound to `mac`. The all-zero address never selects anything,
/// so Windows' ghost ports can never win.
pub fn choose_port_for_mac<'a>(ports: &'a [SppPort], mac: Mac) -> Option<&'a SppPort> {
    if mac.is_zero() {
        return None;
    }
    ports.iter().find(|port| port.mac == mac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_round_trips_through_ull_in_registry_byte_order() {
        // Windows stores 70:4B:CA:72:AC:A2 as 0x704BCA72ACA2.
        let mac = Mac::parse("70:4b:ca:72:ac:a2").unwrap();
        assert_eq!(mac.to_ull(), 0x704B_CA72_ACA2);
        assert_eq!(Mac::from_ull(0x704B_CA72_ACA2), mac);
        assert_eq!(mac.to_string(), "70:4B:CA:72:AC:A2");
    }

    #[test]
    fn mac_parses_bare_hex_and_rejects_malformed() {
        assert_eq!(Mac::parse("704BCA72ACA2").unwrap().to_string(), "70:4B:CA:72:AC:A2");
        for bad in ["", "70:4B:CA:72:AC", "70:4B:CA:72:AC:A2:00", "zz:4B:CA:72:AC:A2", "70-4B-CA-72-AC-A2"] {
            assert!(Mac::parse(bad).is_none(), "{bad} should not parse");
        }
    }

    #[test]
    fn all_zero_address_is_the_ghost_port_marker() {
        assert!(Mac::parse("00:00:00:00:00:00").unwrap().is_zero());
        assert!(!Mac::parse("70:4B:CA:72:AC:A2").unwrap().is_zero());
    }

    #[test]
    fn mac_is_extracted_from_the_instance_name() {
        // Real instance names observed on this machine.
        assert_eq!(
            mac_from_instance("a&2e6f9637&0&704BCA72ACA2_C00000000").unwrap().to_string(),
            "70:4B:CA:72:AC:A2"
        );
        assert_eq!(
            mac_from_instance("a&2e6f9637&0&000000000000_C00000000").unwrap().to_string(),
            "00:00:00:00:00:00"
        );
        assert!(mac_from_instance("a&2e6f9637&0").is_none());
    }

    #[test]
    fn only_the_spp_service_key_yields_ports() {
        let row = |key: &str, instance: &str, port: &str| RegistryInstance {
            key: key.into(),
            instance: instance.into(),
            port_name: port.into(),
        };
        let rows = vec![
            row(
                "{00001101-0000-1000-8000-00805f9b34fb}_LOCALMFG&0000",
                "a&2e6f9637&0&704BCA72ACA2_C00000000",
                "COM7",
            ),
            row(
                "{00001101-0000-1000-8000-00805f9b34fb}_LOCALMFG&0046",
                "a&2e6f9637&0&4142BC659FA8_C00000000",
                "COM6",
            ),
            row(
                "{0000dead-0000-1000-8000-00805f9b34fb}_LOCALMFG&0000",
                "a&2e6f9637&0&1111111127A9_C00000000",
                "COM99",
            ),
        ];
        let ports = spp_ports_from_instances(&rows);
        assert_eq!(ports.len(), 2);
        // Sorted by port name, so COM6 precedes COM7.
        assert_eq!(ports[0].port_name, "COM6");
        assert_eq!(ports[1].port_name, "COM7");
    }

    #[test]
    fn writer_port_wins_over_headset_and_ghost_ports() {
        let row = |instance: &str, port: &str| RegistryInstance {
            key: "{00001101-0000-1000-8000-00805f9b34fb}_LOCALMFG&0000".into(),
            instance: instance.into(),
            port_name: port.into(),
        };
        let ports = spp_ports_from_instances(&[
            row("a&2e6f9637&0&000000000000_C00000000", "COM3"),
            row("a&2e6f9637&0&4142BC659FA8_C00000000", "COM6"),
            row("a&2e6f9637&0&704BCA72ACA2_C00000000", "COM7"),
        ]);
        let writer = Mac::parse("70:4B:CA:72:AC:A2").unwrap();
        assert_eq!(choose_port_for_mac(&ports, writer).unwrap().port_name, "COM7");
        assert!(choose_port_for_mac(&ports, Mac::parse("00:00:00:00:00:00").unwrap()).is_none());
        assert!(choose_port_for_mac(&ports, Mac::parse("AA:BB:CC:DD:EE:FF").unwrap()).is_none());
    }

    #[test]
    fn duplicate_instances_resolve_to_one_port_deterministically() {
        let row = |port: &str| RegistryInstance {
            key: "{00001101-0000-1000-8000-00805f9b34fb}_LOCALMFG&0000".into(),
            instance: "a&2e6f9637&0&704BCA72ACA2_C00000000".into(),
            port_name: port.into(),
        };
        let ports = spp_ports_from_instances(&[row("COM9"), row("COM7")]);
        assert_eq!(ports.len(), 2);
        assert_eq!(
            choose_port_for_mac(&ports, Mac::parse("704BCA72ACA2").unwrap()).unwrap().port_name,
            "COM7"
        );
    }
}
