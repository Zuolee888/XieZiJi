//! Bluetooth (SPP) support: MAC address parsing in Windows registry byte order.

use std::fmt;

/// A 6-octet Bluetooth address.
///
/// Index 0 is the first octet as printed, i.e. the most significant byte.
/// Windows keeps the address as a 48-bit big-endian value, so
/// `70:4B:CA:72:AC:A2` is stored as `0x704BCA72ACA2` — the most significant
/// byte `0x70` comes first, not byte-reversed to `0xA2AC724BCA70`.
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
}
