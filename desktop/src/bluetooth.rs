//! Bluetooth (SPP) support: MAC address parsing in Windows registry byte order,
//! turning Windows registry rows into SPP COM ports, and the Windows backend
//! (bthprops + registry) that lists devices and creates SPP ports.

use serde::{Deserialize, Serialize};
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

/// A discovered (or remembered) Bluetooth device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
    pub remembered: bool,
    pub authenticated: bool,
}

/// The system Bluetooth operations this app needs.
///
/// A SPP device appears to Windows as a virtual COM port once its service is
/// enabled, so the rest of the app keeps using the plain serial code.
pub trait BluetoothBackend: Send + Sync {
    fn scan(&self, seconds: u8) -> Result<Vec<BtDevice>, String>;
    fn spp_ports(&self) -> Result<Vec<SppPort>, String>;
    fn ensure_spp(&self, mac: Mac) -> Result<(), String>;
    fn forget(&self, mac: Mac) -> Result<(), String>;
}

/// Turns a Win32 error code into text a user can act on.
pub fn describe_error(rc: u32) -> String {
    match rc {
        0 => "成功".into(),
        258 => "设备没有响应，请确认写字机已开机且未被手机占用".into(),
        1244 => "该设备需要先在系统设置里配对，请先在 Windows 蓝牙设置中完成配对后重试".into(),
        259 => "未找到蓝牙适配器或设备列表已结束，请确认本机蓝牙已开启".into(),
        other => format!("蓝牙操作失败（错误码 {other}）"),
    }
}

/// Builds a [`BtDevice`] from the raw Win32 fields.
///
/// `fRemembered` / `fAuthenticated` are `BOOL`s that carry flags (`16`, `8`),
/// not `0`/`1`, so they must be compared against zero rather than read as bool.
pub fn bt_device_from(mac: &str, name: &str, remembered: i32, authenticated: i32) -> BtDevice {
    BtDevice {
        mac: mac.to_uppercase(),
        name: name.into(),
        remembered: remembered != 0,
        authenticated: authenticated != 0,
    }
}

/// SPP ports currently registered with Windows.
pub fn spp_ports() -> Vec<SppPort> {
    backend().spp_ports().unwrap_or_default()
}

#[cfg(not(windows))]
pub struct EmptyBackend;

#[cfg(not(windows))]
impl BluetoothBackend for EmptyBackend {
    fn scan(&self, _seconds: u8) -> Result<Vec<BtDevice>, String> {
        Err(NOT_ONLY_WINDOWS.into())
    }
    fn spp_ports(&self) -> Result<Vec<SppPort>, String> {
        Err(NOT_ONLY_WINDOWS.into())
    }
    fn ensure_spp(&self, _mac: Mac) -> Result<(), String> {
        Err(NOT_ONLY_WINDOWS.into())
    }
    fn forget(&self, _mac: Mac) -> Result<(), String> {
        Err(NOT_ONLY_WINDOWS.into())
    }
}

#[cfg(not(windows))]
const NOT_ONLY_WINDOWS: &str = "蓝牙连接目前只在 Windows 上提供";

/// The platform backend.
#[cfg(windows)]
pub fn backend() -> Box<dyn BluetoothBackend> {
    Box::new(win::WindowsBackend)
}

#[cfg(not(windows))]
pub fn backend() -> Box<dyn BluetoothBackend> {
    Box::new(EmptyBackend)
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::{mem, ptr};
    use windows_sys::core::GUID;
    use windows_sys::Win32::Devices::Bluetooth::{
        BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindFirstRadio,
        BluetoothFindNextDevice, BluetoothFindRadioClose,
        BluetoothGetDeviceInfo, BluetoothRemoveDevice, BluetoothSetServiceState,
        BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS, BLUETOOTH_FIND_RADIO_PARAMS,
        BLUETOOTH_SERVICE_ENABLE,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HANDLE, TRUE,
    };
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_READ, REG_SZ,
    };

    pub struct WindowsBackend;

    const BTHENUM: &str = "SYSTEM\\CurrentControlSet\\Enum\\BTHENUM";

    /// The Serial Port Profile service GUID, written out by hand so no
    /// conversion helper is needed.
    fn spp_guid() -> GUID {
        GUID {
            data1: 0x0000_1101,
            data2: 0x0000,
            data3: 0x1000,
            data4: [0x80, 0x00, 0x00, 0x80, 0x5f, 0x9b, 0x34, 0xfb],
        }
    }

    /// Closes only the radio find handle. The radio handle it yields is a
    /// separate object and is closed by the `Radio` guard instead.
    struct RadioFind(windows_sys::Win32::Devices::Bluetooth::HBLUETOOTH_RADIO_FIND);

    impl Drop for RadioFind {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    BluetoothFindRadioClose(self.0);
                }
            }
        }
    }

    struct DeviceFind(windows_sys::Win32::Devices::Bluetooth::HBLUETOOTH_DEVICE_FIND);

    impl Drop for DeviceFind {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    BluetoothFindDeviceClose(self.0);
                }
            }
        }
    }

    /// A radio handle must be closed with `CloseHandle`, not by the find API.
    struct Radio(HANDLE);

    impl Drop for Radio {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Reads up to the first NUL and no further: `szName` is 248 wide chars and
    /// the device name is much shorter, so converting all of it would append a
    /// wall of NULs.
    fn name_from_sz(sz: &[u16; 248]) -> String {
        let end = sz.iter().position(|&c| c == 0).unwrap_or(sz.len());
        String::from_utf16_lossy(&sz[..end])
    }

    /// The error code the API just failed with. `GetLastError` is only
    /// meaningful immediately after the failing call, so it is read at each
    /// failure site; `fallback` covers a code that was never set.
    fn error_code(fallback: u32) -> u32 {
        let rc = unsafe { GetLastError() };
        if rc == ERROR_SUCCESS {
            fallback
        } else {
            rc
        }
    }

    /// Opens the first Bluetooth radio. The find handle is closed before
    /// returning; the radio itself is closed by the caller via `Radio`.
    fn first_radio() -> Result<Radio, String> {
        let mut radio: HANDLE = ptr::null_mut();
        let params = BLUETOOTH_FIND_RADIO_PARAMS {
            dwSize: mem::size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
        };
        let find = unsafe { BluetoothFindFirstRadio(&params, &mut radio) };
        if find.is_null() {
            return Err(describe_error(error_code(ERROR_NO_MORE_ITEMS)));
        }
        // The guard closes the find handle on every path below, including the
        // early return for a null radio.
        let find = RadioFind(find);
        if radio.is_null() {
            return Err(describe_error(error_code(ERROR_NO_MORE_ITEMS)));
        }
        drop(find);
        Ok(Radio(radio))
    }

    impl BluetoothBackend for WindowsBackend {
        fn scan(&self, seconds: u8) -> Result<Vec<BtDevice>, String> {
            let radio = first_radio()?;
            let params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
                dwSize: mem::size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
                fReturnAuthenticated: TRUE,
                fReturnRemembered: TRUE,
                fReturnUnknown: TRUE,
                fReturnConnected: TRUE,
                fIssueInquiry: TRUE,
                cTimeoutMultiplier: seconds,
                hRadio: radio.0,
            };
            let mut info = BLUETOOTH_DEVICE_INFO {
                dwSize: mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
                ..Default::default()
            };
            let find = unsafe { BluetoothFindFirstDevice(&params, &mut info) };
            if find.is_null() {
                // "Nothing found" and "could not look" both return NULL, so the
                // two must be told apart here: ERROR_NO_MORE_ITEMS is the normal
                // empty result the caller renders as an empty list, while
                // anything else (access denied, a radio that vanished mid-scan,
                // a bad parameter) is a real failure the user has to see —
                // otherwise a broken adapter reads as "your plotter is off".
                let rc = error_code(ERROR_NO_MORE_ITEMS);
                if rc == ERROR_NO_MORE_ITEMS {
                    return Ok(Vec::new());
                }
                return Err(describe_error(rc));
            }
            let find = DeviceFind(find);
            let mut devices = Vec::new();
            loop {
                info.dwSize = mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32;
                let mac = Mac::from_ull(unsafe { info.Address.Anonymous.ullLong });
                devices.push(bt_device_from(
                    &mac.to_string(),
                    &name_from_sz(&info.szName),
                    info.fRemembered,
                    info.fAuthenticated,
                ));
                if unsafe { BluetoothFindNextDevice(find.0, &mut info) } == 0 {
                    // Same distinction as above: the end of the list is normal,
                    // a failure part-way through is not. A truncated list would
                    // hide devices from the picker, so report it instead.
                    let rc = error_code(ERROR_NO_MORE_ITEMS);
                    if rc != ERROR_NO_MORE_ITEMS {
                        return Err(describe_error(rc));
                    }
                    break;
                }
            }
            drop(find);
            Ok(devices)
        }

        fn spp_ports(&self) -> Result<Vec<SppPort>, String> {
            let root = wide(BTHENUM);
            let mut key: HKEY = ptr::null_mut();
            let rc = unsafe {
                RegOpenKeyExW(HKEY_LOCAL_MACHINE, root.as_ptr(), 0, KEY_READ, &mut key)
            };
            if rc != ERROR_SUCCESS {
                return Err(describe_error(rc));
            }
            let root = RegKey(key);

            let mut rows = Vec::new();
            for service in subkeys(root.0)? {
                let Some(service_key) = open_subkey(root.0, &service) else {
                    continue;
                };
                let service_key = RegKey(service_key);
                let Ok(instances) = subkeys(service_key.0) else {
                    continue;
                };
                for instance in instances {
                    let Some(port_name) = port_name_of(service_key.0, &instance) else {
                        continue;
                    };
                    rows.push(RegistryInstance {
                        key: service.clone(),
                        instance,
                        port_name,
                    });
                }
            }
            Ok(spp_ports_from_instances(&rows))
        }

        fn ensure_spp(&self, mac: Mac) -> Result<(), String> {
            let radio = first_radio()?;
            // dwSize must be set before every BluetoothGetDeviceInfo call; the
            // API reads it to decide the struct layout and returns garbage
            // otherwise.
            let mut info = BLUETOOTH_DEVICE_INFO {
                dwSize: mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
                Address: windows_sys::Win32::Devices::Bluetooth::BLUETOOTH_ADDRESS {
                    Anonymous: windows_sys::Win32::Devices::Bluetooth::BLUETOOTH_ADDRESS_0 {
                        ullLong: mac.to_ull(),
                    },
                },
                ..Default::default()
            };
            info.dwSize = mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32;
            let rc = unsafe { BluetoothGetDeviceInfo(radio.0, &mut info) };
            if rc != ERROR_SUCCESS {
                return Err(describe_error(rc));
            }
            let guid = spp_guid();
            let rc = unsafe { BluetoothSetServiceState(radio.0, &info, &guid, BLUETOOTH_SERVICE_ENABLE) };
            if rc != ERROR_SUCCESS {
                return Err(describe_error(rc));
            }
            Ok(())
        }

        fn forget(&self, mac: Mac) -> Result<(), String> {
            let address = windows_sys::Win32::Devices::Bluetooth::BLUETOOTH_ADDRESS {
                Anonymous: windows_sys::Win32::Devices::Bluetooth::BLUETOOTH_ADDRESS_0 {
                    ullLong: mac.to_ull(),
                },
            };
            let rc = unsafe { BluetoothRemoveDevice(&address) };
            if rc != ERROR_SUCCESS {
                return Err(describe_error(rc));
            }
            Ok(())
        }
    }

    struct RegKey(HKEY);

    impl Drop for RegKey {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    RegCloseKey(self.0);
                }
            }
        }
    }

    fn open_subkey(parent: HKEY, name: &str) -> Option<HKEY> {
        let name = wide(name);
        let mut key: HKEY = ptr::null_mut();
        let rc = unsafe { RegOpenKeyExW(parent, name.as_ptr(), 0, KEY_READ, &mut key) };
        if rc != ERROR_SUCCESS {
            return None;
        }
        Some(key)
    }

    /// Enumerates the immediate subkeys of `parent`.
    fn subkeys(parent: HKEY) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        let mut index = 0;
        loop {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            let rc = unsafe {
                RegEnumKeyExW(
                    parent,
                    index,
                    buf.as_mut_ptr(),
                    &mut len,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if rc == ERROR_NO_MORE_ITEMS {
                break;
            }
            if rc != ERROR_SUCCESS {
                return Err(describe_error(rc));
            }
            names.push(String::from_utf16_lossy(&buf[..len as usize]));
            index += 1;
        }
        Ok(names)
    }

    /// Reads `Device Parameters\PortName` for one instance, as `REG_SZ`.
    fn port_name_of(service_key: HKEY, instance: &str) -> Option<String> {
        let params = open_subkey(service_key, instance)?;
        let params = RegKey(params);
        let device_params = open_subkey(params.0, "Device Parameters")?;
        let device_params = RegKey(device_params);

        let value_name = wide("PortName");
        let mut ty = 0u32;
        let mut bytes = [0u8; 64];
        let mut size = bytes.len() as u32;
        let rc = unsafe {
            RegQueryValueExW(
                device_params.0,
                value_name.as_ptr(),
                ptr::null(),
                &mut ty,
                bytes.as_mut_ptr(),
                &mut size,
            )
        };
        if rc != ERROR_SUCCESS || ty != REG_SZ {
            return None;
        }
        let chars: Vec<u16> = bytes[..size as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
        let port = String::from_utf16_lossy(&chars[..end]);
        (!port.is_empty()).then_some(port)
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
    fn win32_error_codes_become_actionable_chinese() {
        assert!(describe_error(0).contains("成功"));
        assert!(describe_error(258).contains("没有响应")); // WAIT_TIMEOUT
        assert!(describe_error(1244).contains("系统设置里配对")); // ERROR_NOT_AUTHENTICATED
        assert!(describe_error(259).contains("蓝牙适配器")); // ERROR_NO_MORE_ITEMS
        assert!(describe_error(9999).contains("9999")); // 未知码回显原码
    }

    #[test]
    fn remembered_and_authenticated_are_ints_not_bools() {
        // BtWriter reports fAuthenticated=0, fRemembered=16 — both true-ish/absent.
        let writer = bt_device_from("70:4B:CA:72:AC:A2", "BtWriter", 16, 0);
        assert!(writer.remembered);
        assert!(!writer.authenticated);
        let headset = bt_device_from("41:42:BC:65:9F:A8", "ROSE OpenFeel", 16, 8);
        assert!(headset.remembered);
        assert!(headset.authenticated);
        let fresh = bt_device_from("AA:BB:CC:DD:EE:FF", "X", 0, 0);
        assert!(!fresh.remembered && !fresh.authenticated);
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
