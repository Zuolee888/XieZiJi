# 蓝牙连接实现计划（Bluetooth SPP Connection）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 OpenInk Studio 在 GUI 内发现、绑定并通过蓝牙 SPP 连接奎享写字机（BtWriter），与 USB 连接行为一致，并能在其他 Windows 电脑上使用。

**Architecture:** 蓝牙不另起一套操控代码 —— 一切最终落到一个 COM 口名，由现有 `serialport` 打开。新增 `desktop/src/bluetooth.rs` 承载「MAC 类型 + SPP 端口解析 + Win32 bthprops 绑定」，把可测的纯逻辑（MAC 字节序、注册表行解析、候选选择、链路自检）与不可测的系统调用分离；`device.rs` 只增加 `PortInfo` 字段、候选选择规则与重绑定逻辑。

**Tech Stack:** Rust 2021 / Tauri 2 / `serialport` 4.10.1 / `windows-sys` 0.61（feature：`Win32_Devices_Bluetooth`、`Win32_Foundation`、`Win32_System_Registry`）/ React + TypeScript（单文件 `App.tsx`）/ `node:test`。

**Spec:** `reports/bluetooth-connection-design.md`（ver 2，300 行；§0 实测摘要、§4 方案设计、§6 验收剧本、§7 未知项）

## Global Constraints

- **crate 无 `lib` target**：`desktop/src/lib.rs` 不存在。测试一律 `cargo test --bins`，**`--lib` 是硬错误**（`no library targets found`）。
- **跑 Rust 测试必须设置四个环境变量**（否则链接失败 `LNK1104`）：
  `CARGO_HOME=<repo>/.cache/cargo`、`RUSTUP_HOME=<repo>/.cache/rustup`、`TMP=<repo>/.cache/linktmp`、`TEMP=<repo>/.cache/linktmp`。
  其中 `<repo>` = `openink-studio` 目录；cargo 用 `<repo>/.cache/cargo/bin/cargo.exe`（系统 PATH 上没有 cargo）。
- **`windows-sys` 必须钉 `0.61`**。`0.52` 的 `HANDLE` 是 `isize` 且结构体无 `Default`，编译失败；离线 registry 里 0.61.2 已存在，**不需要联网**。
- **超时维持现状**：读超时 30 ms、握手/恢复窗口 3 s、ack 超时 15 s。**不要**引入 15 s 首帧超时（实测冷启最坏 1.65 s）。
- **只做 Windows**。非 Windows 走 `#[cfg]` 空实现。
- **不做**：后台静默配对、链路层加密、WinRT 配对接口（对非 MSIX 程序一律 `AccessDenied`）。
- **不要顺手改的东西**（设计文档 §4.4「不做的事」）：不改 GRBL 握手语义；不改暂停/恢复/急停路径；不对蓝牙做盲发重试（丢一行就是错一笔）；不调整 `$` 参数（实测 4 行一批 6.97 kB/s 已够用）。
- **面向用户的字符串是内联简体中文**：完整句、礼貌祈使、无感叹号、先因后「，请…」补救、无错误码。`device.rs` 的 Rust 侧诊断串保持英文（与现有风格一致）；只有直达用户的改中文。
- **必须复用现有假硬件模式**：`MockWire` 包 `std::io::Cursor` 塞进 `Link`，不碰真串口。
- **绝不运行 `tauri dev`** —— 唯一会打开真实端口的入口，只有 Task 11 由人执行。
- 仓库**没有** eslint / prettier / clippy / rustfmt，CI 也不跑测试。**不要**加调用它们的步骤。
- 工作树已有 15 个与本计划无关的未提交修改。**每次 `git add` 只列本任务的文件，禁止 `git add -A` / `git add .`**。
- 真实值（测试里直接用）：适配器 `4C23380C974C`、BtWriter `70:4B:CA:72:AC:A2`、SPP 服务 GUID `{00001101-0000-1000-8000-00805f9b34fb}`、写字机 SPP 口 `COM7`、USB `COM13`。

## Review Focus

1. **MAC 字节序反转** —— 用户会看到「设备就在旁边但连接超时」。`BLUETOOTH_ADDRESS.ullLong` 的最高字节是 MAC 第一个八位组；搞反会配对到不存在的地址并返回 `WAIT_TIMEOUT(258)`，项目里已犯过一次。→ Task 1 的测试。
2. **幽灵 SPP 口** —— 用户点「写字机」却连上耳机。`COM3`/`COM5`/`COM16` 实例内嵌 MAC 全 0；按端口号匹配必然踩到。→ Task 2 的测试。
3. **`fAuthenticated`/`fRemembered` 是 int 不是 bool** —— 用户看到「已配对」状态永远为假。BtWriter 正常值 `fAuthenticated=0`、`fRemembered=16`。→ Task 4 的 `bt_device_from` 测试。
4. **多行响应被 30 ms 超时截断** —— 用户看到握手失败或状态解析不出。`$$` 在 13 ms 处只拿到 65 字节；「发一条命令单次 read」必拿半截。→ Task 8 的 `multi_line_identity_survives_a_split_read` 测试。
5. **USB 与蓝牙同时在线** —— 用户的机械动作莫名其妙交错。两条链路都能收发 G-code。→ Task 8 的 `link_warning` 测试。
6. **端口能打开但读不到（死链路）** —— 用户点了连接、界面卡住、报一个看不懂的握手错误。`open()` 成功不代表链路活着。→ Task 7 的 `should_rebind_spp` 测试。
7. **打开失败抛英文系统错误** —— 用户看到「系统找不到指定的文件。(os error 2)」而不知道该怎么办。`NotFound` 与 `PermissionDenied` 必须变成可执行的中文建议；蓝牙占着端口时尤其常见。→ Task 9 的 `open_failures_explain_themselves_in_chinese` 测试。
8. **蓝牙断链后立刻重试必然再失败** —— 写字机重新上电后链路要几秒才恢复，现有两次无间隔重试会瞬间用光，用户看到「恢复失败」。必须指数退避 1/2/4/4/4 s 共 5 次，且**只对蓝牙生效**，USB 的重连路径一个字都不能动。→ Task 9 的 `reconnect_backoff_is_capped_and_gives_up` 测试。

---

### Task 1: `Mac` 类型与字节序

反了就是 `WAIT_TIMEOUT(258)`，先把它钉死。

**Files:**
- Create: `desktop/src/bluetooth.rs`
- Modify: `desktop/src/main.rs`（在 `mod device;` 之后加 `mod bluetooth;`）
- Test: `desktop/src/bluetooth.rs` 内联 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `pub struct Mac([u8; 6])`；`Mac::parse(&str) -> Option<Mac>`、`Mac::from_ull(u64) -> Mac`、`Mac::to_ull(self) -> u64`、`Mac::is_zero(self) -> bool`；`impl Display`（大写、冒号分隔）；`impl PartialEq/Eq/Clone/Copy/Debug`。

- [ ] **Step 1: 写失败测试**

```rust
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
```

- [ ] **Step 2: 运行，确认失败**

```
cd openink-studio/desktop
CARGO_HOME=<repo>/.cache/cargo RUSTUP_HOME=<repo>/.cache/rustup TMP=<repo>/.cache/linktmp TEMP=<repo>/.cache/linktmp \
  <repo>/.cache/cargo/bin/cargo.exe test --bins bluetooth::tests
```
Expected: 编译失败 `cannot find type Mac`。

- [ ] **Step 3: 实现 `desktop/src/bluetooth.rs`**

`Mac([u8; 6])`，索引 0 是**打印时的第一个八位组**（最高字节）。`from_ull(v)` 用 `(v >> (8 * (5 - i))) & 0xFF` 填 `i`；`to_ull` 用 `value = (value << 8) | byte` 累积。`parse` 接受带冒号（大小写无关）或裸 12 位十六进制，其他一律 `None`。`Display` 用 `{:02X}` 冒号连接。

- [ ] **Step 4: 运行，确认通过**

同一命令，Expected: `3 passed`。

- [ ] **Step 5: 提交**

```bash
git add desktop/src/bluetooth.rs desktop/src/main.rs
git commit -m "feat(bluetooth): add MAC type with registry byte order"
```

---

### Task 2: SPP 端口解析与 MAC 选口

注册表行 → COM 口，纯函数，不需要硬件。

**Files:**
- Modify: `desktop/src/bluetooth.rs`
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: Task 1 的 `Mac`。
- Produces:
  - `pub struct RegistryInstance { pub key: String, pub instance: String, pub port_name: String }`
  - `pub struct SppPort { pub port_name: String, pub mac: Mac, pub instance: String }`
  - `pub fn mac_from_instance(&str) -> Option<Mac>`
  - `pub fn spp_ports_from_instances(&[RegistryInstance]) -> Vec<SppPort>`
  - `pub fn choose_port_for_mac<'a>(&'a [SppPort], Mac) -> Option<&'a SppPort>`

- [ ] **Step 1: 写失败测试**

```rust
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
        key: key.into(), instance: instance.into(), port_name: port.into(),
    };
    let rows = vec![
        row("{00001101-0000-1000-8000-00805f9b34fb}_LOCALMFG&0000", "a&2e6f9637&0&704BCA72ACA2_C00000000", "COM7"),
        row("{00001101-0000-1000-8000-00805f9b34fb}_LOCALMFG&0046", "a&2e6f9637&0&4142BC659FA8_C00000000", "COM6"),
        row("{0000dead-0000-1000-8000-00805f9b34fb}_LOCALMFG&0000", "a&2e6f9637&0&1111111127A9_C00000000", "COM99"),
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
        instance: instance.into(), port_name: port.into(),
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
        instance: "a&2e6f9637&0&704BCA72ACA2_C00000000".into(), port_name: port.into(),
    };
    let ports = spp_ports_from_instances(&[row("COM9"), row("COM7")]);
    assert_eq!(ports.len(), 2);
    assert_eq!(
        choose_port_for_mac(&ports, Mac::parse("704BCA72ACA2").unwrap()).unwrap().port_name,
        "COM7"
    );
}
```

- [ ] **Step 2: 运行，确认失败**

`cargo test --bins bluetooth::tests`。Expected: `cannot find function mac_from_instance`。

- [ ] **Step 3: 实现**

`mac_from_instance`：按 `&` 切分，对每段取 `_` 之前的部分，若长度 12 且全为十六进制则 `Mac::parse`，找到第一个即返回 `Some`。`spp_ports_from_instances`：只保留 `key` **以** `{00001101-0000-1000-8000-00805f9b34fb}` **开头**（大小写无关）的行，解析 MAC，按 `port_name` 升序排序后返回。`choose_port_for_mac`：先判 `mac.is_zero()` 直接 `None`，再 `ports.iter().find(|p| p.mac == mac)`。

- [ ] **Step 4: 运行，确认通过**

Expected: `7 passed`。

- [ ] **Step 5: 提交**

```bash
git add desktop/src/bluetooth.rs
git commit -m "feat(bluetooth): parse SPP ports from registry instances"
```

---

### Task 3: `PortInfo` 暴露链路类型

UI 要能区分「CH340」和「蓝牙」。本任务只加字段，**不动 `ports()` 逻辑**（蓝牙标记在 Task 4 接上，那时才有注册表读取能力）。

**Files:**
- Modify: `desktop/src/device.rs:23-27`（`PortInfo`）
- Test: `desktop/src/device.rs` 内联 `mod tests`

**Interfaces:**
- Produces:
  - `pub enum PortKind { Usb, Bluetooth, Other }`，`#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]`，`#[serde(rename_all = "snake_case")]`，`Default` 为 `Other`
  - `pub struct PortInfo { pub name: String, pub description: String, pub preferred: bool, #[serde(default)] pub kind: PortKind, #[serde(default)] pub mac: Option<String> }`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn bluetooth_port_info_carries_kind_and_mac() {
    let p = PortInfo {
        name: "COM7".into(),
        description: "蓝牙 · BtWriter".into(),
        preferred: true,
        kind: PortKind::Bluetooth,
        mac: Some("70:4B:CA:72:AC:A2".into()),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["kind"], "bluetooth");
    assert_eq!(json["mac"], "70:4B:CA:72:AC:A2");
    let back: PortInfo = serde_json::from_value(json).unwrap();
    assert_eq!(back.kind, PortKind::Bluetooth);
}

#[test]
fn port_info_without_kind_still_deserializes() {
    let json = serde_json::json!({"name":"COM13","description":"USB","preferred":true});
    let p: PortInfo = serde_json::from_value(json).unwrap();
    assert_eq!(p.kind, PortKind::Other);
    assert_eq!(p.mac, None);
}
```

- [ ] **Step 2: 运行，确认失败**

`cargo test --bins device::tests::bluetooth_port_info_carries_kind_and_mac`。Expected: `missing field kind`。

- [ ] **Step 3: 实现**

加 `PortKind` 与两个字段；现有 `ports()` 里构造 `PortInfo` 的地方补 `kind: PortKind::Usb`（`UsbPort` 分支）与 `kind: PortKind::Other, mac: None`（`_` 分支）；`preferred` 分支保持原样。`ch340 preferred` 判定不变。

- [ ] **Step 4: 运行，确认通过且无回归**

`cargo test --bins` → Expected: `186 passed; 0 failed; 10 ignored`（基线 177 + Task 1 的 3 + Task 2 的 4 + 本任务 2）。

- [ ] **Step 5: 提交**

```bash
git add desktop/src/device.rs
git commit -m "feat(device): tag ports with link kind and bluetooth MAC"
```

---

### Task 4: Windows 后端（bthprops + 注册表）

到这里才第一次碰系统 API。可测的部分（错误码文案、int 转 bool）与系统调用分开。

**Files:**
- Modify: `desktop/Cargo.toml`（`[dependencies]` 之后加 target 段）
- Modify: `desktop/src/bluetooth.rs`（加 backend 模块 + `spp_ports()`）
- Modify: `desktop/src/device.rs:173-199`（`ports()` 接上蓝牙标记）
- Test: 两处内联 `mod tests`

**Interfaces:**
- Consumes: Task 1 的 `Mac`、Task 2 的 `SppPort` / `RegistryInstance` / `spp_ports_from_instances`、Task 3 的 `PortKind` / `PortInfo`。
- Produces:
  - `pub struct BtDevice { pub mac: String, pub name: String, pub remembered: bool, pub authenticated: bool }`（`Serialize, Deserialize, Clone, Debug, PartialEq`）
  - `pub trait BluetoothBackend: Send + Sync { fn scan(&self, seconds: u8) -> Result<Vec<BtDevice>, String>; fn spp_ports(&self) -> Result<Vec<SppPort>, String>; fn ensure_spp(&self, mac: Mac) -> Result<(), String>; fn forget(&self, mac: Mac) -> Result<(), String>; }`
  - `pub fn backend() -> Box<dyn BluetoothBackend>`（Windows 真实实现；其他平台 `EmptyBackend` 全部返回 `Err("蓝牙连接目前只在 Windows 上提供")`）
  - `pub fn describe_error(&u32) -> String`（纯函数）
  - `pub fn bt_device_from(&str, &str, i32, i32) -> BtDevice`（纯函数）

- [ ] **Step 1: 加依赖并确认离线可解析**

`desktop/Cargo.toml`：

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = [
    "Win32_Devices_Bluetooth",
    "Win32_Foundation",
    "Win32_System_Registry",
] }
```

验证：`<repo>/.cache/cargo/bin/cargo.exe build --offline` → Expected: 成功（0.61.2 已在 `.cache/cargo/registry`，不需联网）。

- [ ] **Step 2: 写失败测试（只测纯函数）**

```rust
#[test]
fn win32_error_codes_become_actionable_chinese() {
    assert!(describe_error(0).contains("成功"));
    assert!(describe_error(258).contains("没有响应"));            // WAIT_TIMEOUT
    assert!(describe_error(1244).contains("系统设置里配对"));      // ERROR_NOT_AUTHENTICATED
    assert!(describe_error(259).contains("蓝牙适配器"));           // ERROR_NO_MORE_ITEMS
    assert!(describe_error(9999).contains("9999"));                // 未知码回显原码
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
```

- [ ] **Step 3: 运行，确认失败**

`cargo test --bins bluetooth::tests::win32`。Expected: `cannot find function describe_error`。

- [ ] **Step 4: 实现**

- `describe_error`：`0` → `"成功"`；`258` → `"设备没有响应，请确认写字机已开机且未被手机占用"`；`1244` → `"该设备需要先在系统设置里配对，请先在 Windows 蓝牙设置中完成配对后重试"`；`259` → `"未找到蓝牙适配器或设备列表已结束，请确认本机蓝牙已开启"`；其他 → `format!("蓝牙操作失败（错误码 {rc}）")`。
- `bt_device_from(mac, name, remembered: i32, authenticated: i32)` → `BtDevice { mac: mac.to_uppercase(), name: name.into(), remembered: remembered != 0, authenticated: authenticated != 0 }`。
- `scan(seconds)`：`BluetoothFindFirstRadio` → `BluetoothFindFirstDevice`（`BLUETOOTH_DEVICE_SEARCH_PARAMS` 四个 `fReturn*` 全 `TRUE`、`fIssueInquiry = TRUE`、`cTimeoutMultiplier = seconds`、`hRadio`）；`szName` 取到第一个 0 为止经 `String::from_utf16_lossy`；`Address.Anonymous.ullLong` → `Mac::from_ull` → `bt_device_from`。结束时 `BluetoothFindDeviceClose` / `BluetoothFindRadioClose`。
- `ensure_spp(mac)`：`BluetoothGetDeviceInfo`（**先把 `dwSize` 设为 `size_of::<BLUETOOTH_DEVICE_INFO>()`**，`Address.Anonymous.ullLong = mac.to_ull()`）→ `BluetoothSetServiceState(radio, &info, &spp_guid(), BLUETOOTH_SERVICE_ENABLE)` → `rc != 0` 时返回 `describe_error(rc)`。
- `spp_guid()`：手写 `GUID { data1: 0x0000_1101, data2: 0x0000, data3: 0x1000, data4: [0x80, 0x00, 0x00, 0x80, 0x5f, 0x9b, 0x34, 0xfb] }`（不要依赖 `GUID::from_u128`）。
- `spp_ports()`：`RegOpenKeyExW(HKEY_LOCAL_MACHINE, "SYSTEM\\CurrentControlSet\\Enum\\BTHENUM")` → `RegEnumKeyExW` 取一级子键 → 对每个再枚举二级实例 → 读 `Device Parameters\\PortName`（`REG_SZ`，`RegQueryValueExW`）→ 组装 `RegistryInstance` → 交给 `spp_ports_from_instances`。
- `forget(mac)`：`BluetoothRemoveDevice`。
- `bluetooth::spp_ports()` 便捷函数 = `backend().spp_ports().unwrap_or_default()`。

**注意**：`windows-sys 0.61` 的 `HANDLE` 是 `*mut c_void`，用 `std::ptr::null_mut()` 初始化；`BLUETOOTH_DEVICE_INFO` 有 `Default`，可 `..Default::default()`。

- [ ] **Step 5: 接进 `ports()`**

`DeviceService::ports()` 末尾：`for spp in bluetooth::spp_ports()` 找到同名 `PortInfo`（没有就 push 一条新的），设 `kind = PortKind::Bluetooth`、`mac = Some(spp.mac.to_string())`、`description = format!("蓝牙串口 · {}", spp.mac)`、`preferred = true`。排序保持 `(!p.preferred, p.name.clone())`。

**注意**：注册表行里只有 MAC，没有设备名，所以描述里写不到 `BtWriter` —— 设备名只出现在「扫描蓝牙设备」列表里（Task 6 的 `BtDevice.name`）。设计文档 §4.4 表格里写的是 `蓝牙串口 · BtWriter 70:4B:CA:72:AC:A2`，本计划简化为 `蓝牙串口 · <MAC>`，是同一个信息量下能拿到的部分。

- [ ] **Step 6: 运行，确认通过**

`cargo test --bins` → Expected: `188 passed; 0 failed; 10 ignored`。

- [ ] **Step 7: 用现成工具交叉验证注册表读取**

```
cd tools/btprobe && dotnet build -c Release -v q --nologo
bin/Release/net9.0-windows10.0.26100.0/win-x64/btprobe.exe ports
```
Expected: 输出包含 `COM7=704BCA72ACA2`。Rust 侧的真实调用在 Task 11 的 GUI 走查中确认。

- [ ] **Step 8: 提交**

```bash
git add desktop/Cargo.toml desktop/Cargo.lock desktop/src/bluetooth.rs desktop/src/device.rs
git commit -m "feat(bluetooth): add windows bthprops backend and tag SPP ports"
```

---

### Task 5: 自动连接接受唯一蓝牙候选

`AutoConnect` 现在硬性要求「恰好一个 CH340」，蓝牙口永远选不中。

**Files:**
- Modify: `desktop/src/device.rs:1301-1332`（`DeviceCommand::AutoConnect` 分支）
- Test: `desktop/src/device.rs` 内联 `mod tests`

**Interfaces:**
- Consumes: Task 3 的 `PortInfo` / `PortKind`。
- Produces: `pub fn auto_select_candidate(&[PortInfo]) -> Result<&PortInfo, String>`

- [ ] **Step 1: 写失败测试**

```rust
fn port(name: &str, kind: PortKind, preferred: bool) -> PortInfo {
    PortInfo { name: name.into(), description: String::new(), preferred, kind, mac: None }
}

#[test]
fn auto_select_accepts_a_single_bluetooth_port() {
    let ports = [port("COM7", PortKind::Bluetooth, true), port("COM6", PortKind::Other, false)];
    assert_eq!(auto_select_candidate(&ports).unwrap().name, "COM7");
}

#[test]
fn auto_select_prefers_usb_and_refuses_ambiguity() {
    let both = [port("COM13", PortKind::Usb, true), port("COM7", PortKind::Bluetooth, true)];
    assert_eq!(auto_select_candidate(&both).unwrap().name, "COM13");

    let two_usb = [port("COM13", PortKind::Usb, true), port("COM14", PortKind::Usb, true)];
    assert!(auto_select_candidate(&two_usb).is_err());

    let none = [port("COM6", PortKind::Other, false)];
    assert!(auto_select_candidate(&none).is_err());
}
```

- [ ] **Step 2: 运行，确认失败**

`cargo test --bins device::tests::auto_select_accepts_a_single_bluetooth_port`。Expected: `cannot find function auto_select_candidate`。

- [ ] **Step 3: 实现**

优先级：`preferred && kind == Usb` 恰好一个 → 选它；否则 `preferred && kind == Bluetooth` 恰好一个 → 选它；否则 `Err("请手动选择串口：自动识别要求恰好一台 CH340 或一台已绑定的蓝牙写字机")`。`AutoConnect` 分支改成 `match auto_select_candidate(&p)`，成功路径复用现有 `connect` 逻辑。

- [ ] **Step 4: 运行，确认通过**

`cargo test --bins` → Expected: **`196 passed; 0 failed; 10 ignored`**（原计划写 190；`park`/`flip_y` 在途改动提前提交时已进 190，本任务的 2 个测试 + review 修复时补的 4 个零 MAC/歧义用例 + 复核时补的 2 个幽灵口用例，合计落在 196）。

- [ ] **Step 5: 提交**

```bash
git add desktop/src/device.rs
git commit -m "feat(device): let auto-connect pick a single bluetooth port"
```

---

### Task 6: IPC 命令与事件

把后端接到 UI 上。

**Files:**
- Modify: `desktop/src/commands.rs`（`Request` 加变体、`poll_device` 抽 `map_event`）
- Modify: `desktop/src/device.rs`（`DeviceCommand` / `DeviceEvent` 加变体、worker 加分支）
- Test: 两处内联 `mod tests`

**Interfaces:**
- Consumes: Task 4 的 `BtDevice` / `backend()`；Task 2 的 `choose_port_for_mac`。
- Produces:
  - `Request::BtScan` / `Request::BtConnect { mac: String, swap_xy: bool, flip_y: bool }` / `Request::BtForget { mac: String }`
  - `DeviceCommand::BtScan` / `BtConnect { mac: String, swap_xy: bool, flip_y: bool }` / `BtForget { mac: String }`
  - **`BtConnect` 必须带 `swap_xy` / `flip_y`**，与 `Connect` / `AutoConnect` / `Recover` 三个变体保持一致（三者都已有这两个字段）。worker 里的 `swap_xy` 是局部变量，只由这三个变体赋值；`FLIP_Y_RUNTIME` 的默认值是 `false`，而 UI 的默认值是 **`true`**（`App.tsx:541-543`，本机 `$3=7` 需要 Y 翻转）。少了这两个字段，用户走「扫描蓝牙设备 → 连接」时轴设置会停在默认值 —— 写出来的字整篇 Y 轴镜像，而且没有任何报错。
  - `DeviceEvent::BtDevices(Vec<BtDevice>)` / `DeviceEvent::BtPortNotReady { mac: String, reason: String }`
  - `commands::map_event(DeviceEvent) -> serde_json::Value`（从 `poll_device` 抽出）
  - JSON：`{"type":"bt_devices","devices":[...]}`、`{"type":"bt_error","message":"..."}`

- [ ] **Step 1: 写失败测试**

`device.rs`（或 `commands.rs`）：

```rust
#[test]
fn bt_requests_round_trip_through_serde() {
    let r: Request = serde_json::from_value(serde_json::json!({"type":"bt_scan"})).unwrap();
    assert!(matches!(r, Request::BtScan));
    let r: Request = serde_json::from_value(
        serde_json::json!({"type":"bt_connect","mac":"70:4B:CA:72:AC:A2"}),
    ).unwrap();
    assert!(matches!(r, Request::BtConnect { ref mac } if mac == "70:4B:CA:72:AC:A2"));
}
```

`commands.rs`：

```rust
#[test]
fn bt_devices_event_becomes_poll_payload() {
    let v = map_event(DeviceEvent::BtDevices(vec![crate::bluetooth::BtDevice {
        mac: "70:4B:CA:72:AC:A2".into(),
        name: "BtWriter".into(),
        remembered: true,
        authenticated: false,
    }]));
    assert_eq!(v["type"], "bt_devices");
    assert_eq!(v["devices"][0]["name"], "BtWriter");
}
```

- [ ] **Step 2: 运行，确认失败**

`cargo test --bins bt_`。Expected: `unknown variant bt_scan`。

- [ ] **Step 3: 实现**

`Request` 加三个变体（续用现有 `#[serde(tag = "type", rename_all = "snake_case")]`）；`device_command` 转发成 `DeviceCommand`。把 `poll_device` 的 `match` 抽成 `fn map_event(e: DeviceEvent) -> Value`，`poll_device` 变成 `.map(map_event)`。

worker 分支：
- `BtScan` → `backend().scan(8)`；`Ok(d)` 发 `BtDevices(d)`，`Err(e)` 发 `Error(e)`。
- `BtConnect { mac, swap_xy: requested, flip_y: invert_y }` → `swap_xy = requested; set_flip_y(invert_y);` **先做这两步，与 `Connect` / `AutoConnect` 分支完全一致**，再做：解析 `Mac`（失败发 `BtPortNotReady{mac, reason:"MAC 地址无法解析，请重新扫描设备列表"}`）→ `ensure_spp` → `spp_ports()` → `choose_port_for_mac`（找不到发 `BtPortNotReady{mac, reason:"系统还没有为该设备建立串口，请确认写字机已开机且未被手机占用"}`）→ 拿 `port_name` 走**现有** `connect(&name, swap_xy)`。成功体照抄 `Connect` 分支（`link/connected_port/last_received/held_after_stop/status/calibration/registration_*` 全都要设，然后发 `AutoOrigin(true)` / `CalibrationChanged{origin:None}` / `Connected(name)` / `Status(s)`）。**不要只写一半就发 `Connected`** —— 少了 `status`/`held_after_stop` 会让后续 `?` 轮询报错。
- `BtForget { mac }` → `forget` → 再执行一次 `BtScan` 的分支逻辑刷新列表。

另外，蓝牙分支里 `DeviceCommand::BtConnect` 也必须加入 `device.rs:1361` 那条「已连接则拒绝」的守卫：

```rust
DeviceCommand::Connect { .. } | DeviceCommand::AutoConnect { .. } | DeviceCommand::BtConnect { .. } if link.is_some() => emit(
    DeviceEvent::Error("Disconnect before connecting again".into()),
),
```

否则蓝牙面板能在 USB 已连接时再开一条链路 —— 那正是设计文档 §4.7 要防的双链路互相干扰。

- [ ] **Step 4: 运行，确认通过**

`cargo test --bins` → Expected: **`198 passed; 0 failed; 10 ignored`**。

> 计数说明：计划最初按 177 的干净基线写（T6=192）。`park` + `flip_y` 那批在途改动已在 `6f00043` 单独提交（+3 个测试），Task 5 又加了 +2（零 MAC 等 4 个测试减去原有的重复项），所以现行基线是 **196**。本任务新增 2 个测试 → **198**。后续任务都从 198 起算。

- [ ] **Step 5: 提交**

```bash
git add desktop/src/commands.rs desktop/src/device.rs
git commit -m "feat(ipc): expose bluetooth scan bind and forget commands"
```

---

### Task 7: 死链路识别与 SPP 重绑定

口能打开但读不到 —— 现有代码会干等 3 s 握手超时后直接失败。蓝牙下应重建端口再试一次。

**Files:**
- Modify: `desktop/src/bluetooth.rs`（`should_rebind_spp`）
- Modify: `desktop/src/device.rs`（蓝牙连接路径包一层重试）
- Test: 两处内联 `mod tests`

**Interfaces:**
- Consumes: Task 3 的 `PortKind`（`bluetooth.rs` 里需 `use crate::device::PortKind;`）、Task 4 的 `backend()`。
- Produces: `pub fn should_rebind_spp(kind: PortKind, attempt: u8, error: &str) -> bool`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn rebind_only_for_bluetooth_first_failure() {
    let read_timeout = "Read-only GRBL handshake incomplete (missing: status)";
    assert!(should_rebind_spp(PortKind::Bluetooth, 0, read_timeout));
    assert!(!should_rebind_spp(PortKind::Bluetooth, 1, read_timeout));
    assert!(!should_rebind_spp(PortKind::Usb, 0, read_timeout));
}

#[test]
fn rebind_skips_errors_a_new_port_cannot_fix() {
    // A held/alarmed controller must never be retried by silently rebinding.
    assert!(!should_rebind_spp(PortKind::Bluetooth, 0, "Controller is Hold:0. Connection refused"));
    assert!(!should_rebind_spp(PortKind::Bluetooth, 0, "Extra ACK during handshake"));
    assert!(!should_rebind_spp(PortKind::Bluetooth, 0, "GRBL rejected the read-only connection query: error:9"));
    // Stale bytes mean someone must inspect the queue; retrying only hides the diagnosis.
    assert!(!should_rebind_spp(PortKind::Bluetooth, 0, "Unexpected stale ACK before identity query; no $I, reset, unlock or resume was sent"));
}
```

- [ ] **Step 2: 运行，确认失败**

`cargo test --bins bluetooth::tests::rebind`。Expected: `cannot find function should_rebind_spp`。

- [ ] **Step 3: 实现**

返回 `kind == PortKind::Bluetooth && attempt == 0 && !error.contains("Controller is ") && !error.contains("Extra ACK") && !error.contains("Unexpected stale ACK") && !error.contains("GRBL rejected")`。

> **为什么必须排除 stale ACK**：`device.rs:986` 的原文是「Unexpected stale ACK before identity query; no $I, reset, unlock or resume was sent. **Inspect any queued controller commands before reconnecting**」—— 它明确要求人工检查后再重连。重绑端口只会再次拿到同一批陈旧字节，把「请人工确认」变成一次无意义的自动重试，还让用户看到第二次的错误而不是原始诊断。

`device.rs` 里把蓝牙连接包成「失败后重绑一次」。**两个分支拿信息的方式不同，别写成一样**：

- `BtConnect { mac, .. }` 分支：`mac` 已经在手上，直接用。
- `DeviceCommand::Connect { port: name, .. }` 分支：**它只有 `port: String`，没有 `PortInfo`** —— 拿不到 `kind`，更拿不到 `mac`。所以**只在 `connect` 返回 `Err` 之后**才去查：

```rust
Err(e) => {
    // Only enumerate on the failure path: the success path for USB must stay
    // byte-for-byte what it was.
    let rebind = DeviceService::ports().ok()
        .and_then(|p| p.into_iter().find(|p| p.name == name))
        .filter(|p| p.kind == PortKind::Bluetooth)
        .and_then(|p| p.mac)
        .filter(|_| crate::bluetooth::should_rebind_spp(PortKind::Bluetooth, 0, &e));
    match rebind.and_then(|mac| rebind_once(&mac, swap_xy)) {
        Some(Ok((l, s))) => { /* 成功体与首个分支一致 */ }
        _ => emit(DeviceEvent::Error(e)),
    }
}
```

**关键点：不要在连接前查 `ports()`。** 那会让每一次 USB 连接都多一次串口+注册表枚举，并且引入一个「枚举失败导致本来能连上的 USB 连不上」的新故障模式 —— 与「USB 路径完全不动」直接冲突。枚举只发生在**已经失败**的路径上，那里本来就什么也没连上。

`rebind_once(mac, swap_xy)` 这个 helper（两个分支共用，避免复制粘贴）必须**完整复用 `connect`**：
```rust
fn rebind_once(mac: &str, swap_xy: bool) -> Result<(Link, Status), String> {
    // BtPortError has no `describe`; match it out the way the BtConnect arm does.
    let name = match bluetooth_port_for(mac) {
        Ok(name) => name,
        Err(BtPortError::NotReady(reason)) | Err(BtPortError::Failed(reason)) => {
            return Err(reason)
        }
    };
    connect(&name, swap_xy)
}
```

**不要手写 `open_link` + `handshake` 的组合。** 真正的 `connect`（`device.rs:790-796`）是三段：`open_link` → `recover_session(l, 3s, false, true)` → `handshake`。少了中间那段 `recover_session`，重绑出来的链路就跳过了「先确认 Idle、再发 $I」的恢复握手 —— 那正是 `device.rs:791-793` 注释所依赖的前置条件（「Recovery starts with realtime status only, so a retained Hold cannot swallow an identity query」）。重绑是为了修「端口没了」，不是为了绕开恢复握手；跳过它会让一条本来正常的链路在 Hold 状态下发出 `$I` 并被固件吞掉，最终以一条难以理解的握手超时报错收场。

复用 `bluetooth_port_for`（Task 6 已建的 helper）而不是再写一遍 `ensure_spp` → `spp_ports` → `choose_port_for_mac`，因为它的错误分类（`BtPortError::NotReady` / `Failed`）在这里同样成立。**最多两次**，第二次失败原样返回错误。USB 路径完全不动。

**顺带修一处 Task 6 留下的产品缺陷（本任务拥有失败路径，所以放这里）**：`bluetooth_port_for`（`device.rs:1276`）在 `ensure_spp` 之后**只查一次** `spp_ports()`，查不到就立刻返回 `BtPortNotReady`。这对「已记忆过」的设备够用 —— 那种情况下端口在 rc=0 后立刻出现（我已实测）。但**从未配对过的设备走的是另一条路径**：Windows 要先完成发现与 SDP 查询才建口，可能晚几百毫秒。结果是用户第一次点「连接」必然看到「系统还没有为该设备建立串口」，必须再点一次才成功 —— 而那正是设计文档 §6 剧本 1（全新电脑）要一次通过的场景。

改成有界轮询。轮询体必须接受「取一次端口列表」的闭包，否则写死在真实 `backend()` 里就没法测：

```rust
fn poll_for_port<F>(mut list: F, address: Mac, budget: Duration) -> Result<String, BtPortError>
where F: FnMut() -> Result<Vec<SppPort>, String>
{
    let end = Instant::now() + budget;
    loop {
        match choose_port_for_mac(&list().map_err(BtPortError::Failed)?, address) {
            Some(port) => return Ok(port.port_name.clone()),
            None if Instant::now() >= end => {
                return Err(BtPortError::NotReady(
                    "系统还没有为该设备建立串口，请确认写字机已开机且未被手机占用".into(),
                ))
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn wait_for_spp_port(mac: &str, budget: Duration) -> Result<String, BtPortError> {
    let address = crate::bluetooth::Mac::parse(mac).ok_or_else(|| {
        BtPortError::NotReady("MAC 地址无法解析，请重新扫描设备列表".into())
    })?;
    let backend = crate::bluetooth::backend();
    backend.ensure_spp(address).map_err(BtPortError::Failed)?;
    poll_for_port(|| backend.spp_ports(), address, budget)
}
```

- 预算 **2 秒**（20 次 × 100 ms）。实测冷启建口在 1.0–1.1 s 量级，空闲 150 s 后最坏 1.65 s，2 s 覆盖得住；再长会让「设备根本不在」的情况变成一次漫长等待。
- `ensure_spp` **只调一次**，在 `wait_for_spp_port` 里、不在循环里 —— 重复调用没有意义，且每次都可能触发系统级重协商。
- 每次重查都重新走 `choose_port_for_mac`（端口号可能变化），不要缓存第一次的结果。
- `bluetooth_port_for` 改为薄封装：`wait_for_spp_port(mac, Duration::from_secs(2))`。**`BtConnect` 的调用点不改**。
- 两个新测试（不需要硬件，用 `Vec<Vec<SppPort>>` 的迭代器当 `list`）：
  - `spp_port_poll_returns_the_port_that_appears_late`：第一次空、第二次命中 → `Ok("COM7")`。
  - `spp_port_poll_gives_up_with_a_chinese_reason`：始终空 → `Err(BtPortError::NotReady(reason))` 且 `reason` 含「还没有为该设备建立串口」。

- [ ] **Step 4: 运行，确认通过**

`cargo test --bins` → Expected: **`203 passed; 0 failed; 10 ignored`**（199 + 本任务的 4 个：`should_rebind_spp` 两条断言、`rebind_skips_errors_a_new_port_cannot_fix`、上面两个轮询测试）。

- [ ] **Step 5: 提交**

```bash
git add desktop/src/bluetooth.rs desktop/src/device.rs
git commit -m "fix(bluetooth): rebind the SPP port once when the link is dead"
```

---

### Task 8: 多链路互斥（`$I` 链路自检）

USB 与蓝牙同时在线时两条链路都能收发 G-code。

**关键约束：不要为了自检再发一次 `$I`。** `handshake` 已经在确认 Idle 之后发过 `$I` 并收到回复，只是把 `observations` 吞在了内部。把它带出来即可 —— 对已经建立的链路追加普通命令，正是 `device.rs:965-968` 注释里禁止的做法（「Never add ordinary lines to a held controller」）。

**Files:**
- Modify: `desktop/src/bluetooth.rs`（`LinkMode` / `parse_link_mode` / `link_warning`，全部为纯函数）
- Modify: `desktop/src/device.rs:964-1020`（`handshake` 返回值加 `Vec<String>`）、`desktop/src/device.rs:790-796`（`connect` 透传）
- Modify: `desktop/src/commands.rs`（`map_event` 加 `link_warning`）
- Test: 两处内联 `mod tests`

> 上面两处行号是当前 HEAD（`b8383ea`）的实际位置。若任务执行时因 Task 6/7 的改动而漂移，用 `grep -n "fn handshake\|^fn connect" desktop/src/device.rs` 重新定位，**不要照抄行号去盲改**。

**Interfaces:**
- Consumes: 现有 `handshake` 内部的 `observations: Vec<String>`（已含 `[MSG:Mode=BT:...]` 行）、Task 3 的 `PortKind`（`bluetooth.rs` 里需 `use crate::device::PortKind;`）。
- Produces:
  - `bluetooth::LinkMode { BluetoothConnected(String), BluetoothIdle, Unknown }`（`PartialEq, Debug, Clone`）
  - `bluetooth::parse_link_mode(&[String]) -> LinkMode`（纯函数）
  - `bluetooth::link_warning(PortKind, &LinkMode) -> Option<String>`（纯函数）
  - `device::handshake(Link, Duration) -> Result<(Link, Status, Vec<String>), String>`（**第三个元素是新增的 observations**）
  - `device::connect(&str, bool) -> Result<(Link, Status, Vec<String>), String>`
  - `DeviceEvent::LinkWarning(String)` → `{"type":"link_warning","message":"..."}`

- [ ] **Step 1: 写失败测试**

`bluetooth.rs`（纯函数，两个测试）：

```rust
#[test]
fn parses_bluetooth_link_state_from_real_identity_output() {
    let bt = vec![
        "[VER:1.3a.20221114:]".to_string(),
        "[OPT:MPHB]".to_string(),
        "[MSG:Mode=BT:Name=BtWriter(70:4B:CA:72:AC:A2):Status=Connected with 4C:23:38:0C:97:4C]".to_string(),
        "ok".to_string(),
    ];
    assert_eq!(parse_link_mode(&bt), LinkMode::BluetoothConnected("4C:23:38:0C:97:4C".into()));

    let usb = vec![
        "[VER:1.3a.20221114:]".to_string(),
        "[OPT:MPHB]".to_string(),
        "[MSG:Mode=BT:Name=BtWriter(70:4B:CA:72:AC:A2):Status=Not connected]".to_string(),
        "ok".to_string(),
    ];
    assert_eq!(parse_link_mode(&usb), LinkMode::BluetoothIdle);

    assert_eq!(parse_link_mode(&["ok".to_string()]), LinkMode::Unknown);
    assert_eq!(parse_link_mode(&[]), LinkMode::Unknown);
}

#[test]
fn warns_only_when_bluetooth_holds_the_link_we_did_not_open() {
    let peer = LinkMode::BluetoothConnected("4C:23:38:0C:97:4C".into());
    assert!(link_warning(PortKind::Usb, &peer).is_some());
    assert!(link_warning(PortKind::Bluetooth, &peer).is_none());
    assert!(link_warning(PortKind::Usb, &LinkMode::BluetoothIdle).is_none());
    assert!(link_warning(PortKind::Usb, &LinkMode::Unknown).is_none());
}
```

`device.rs`（分片读取，必须放在能访问私有 `Link` 的模块里）：

```rust
#[test]
fn multi_line_identity_survives_a_split_read() {
    // The wire delivers $I's reply in two chunks — the exact shape that makes a
    // single read() return half a line on this hardware.
    struct ChunkWire {
        chunks: Vec<Vec<u8>>,
        sent: Arc<Mutex<Vec<u8>>>,
    }
    impl Read for ChunkWire {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.chunks.is_empty() {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "t"));
            }
            let chunk = self.chunks.remove(0);
            let n = chunk.len().min(buf.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            Ok(n)
        }
    }
    impl Write for ChunkWire {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.sent.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let sent = Arc::new(Mutex::new(Vec::new()));
    let mut link = Link {
        port: Box::new(ChunkWire {
            chunks: vec![
                b"[VER:1.3a.20221114:]\r\n[OPT:MPHB]\r\n[MSG:Mode=BT:Name=BtWri".to_vec(),
                b"ter(70:4B:CA:72:AC:A2):Status=Connected with 4C:23:38:0C:97:4C]\r\nok\r\n".to_vec(),
            ],
            sent: sent.clone(),
        }),
        rx: vec![],
        swap_xy: false,
    };
    let mut lines = vec![];
    while lines.last().map(String::as_str) != Some("ok") {
        lines.extend(link.lines().unwrap());
    }
    assert_eq!(
        crate::bluetooth::parse_link_mode(&lines),
        crate::bluetooth::LinkMode::BluetoothConnected("4C:23:38:0C:97:4C".into())
    );
}
```

（若 `device.rs` 的 tests 模块尚未导入 `Read`/`Write`/`Arc`/`Mutex`，按现有 `mock_wire_splits_crlf_and_writes_single_lf` 的写法补齐。）

- [ ] **Step 2: 运行，确认失败**

`cargo test --bins bluetooth::tests::parses_bluetooth`。Expected: `cannot find enum LinkMode`。

- [ ] **Step 3: 实现**

`bluetooth.rs`：`parse_link_mode` 找以 `[MSG:Mode=BT:` 开头的行；若其后含 `Status=Connected with `，取该子串之后到 `]` 之前的部分（保留原大小写）作为对端 MAC → `BluetoothConnected`；若含 `Status=Not connected` → `BluetoothIdle`；否则 `Unknown`。

`link_warning(kind, mode)`：仅当 `kind == PortKind::Usb` 且 `mode` 是 `BluetoothConnected(_)` 时返回 `"机械同时被蓝牙链路占用，两条链路同时下发指令会让动作交错。请关闭其中一条连接后再开始书写"`。

`device.rs`：把 `handshake` 的 `let mut observations = vec![];`（`:974`）在成功分支 `:1017` 一并返回 `Ok((l, status.unwrap(), observations))`，失败分支不受影响。`connect`（`:790-796`）改为返回三元组，把 observations 透传。

**签名改动的两个调用点必须区别对待（这是本任务最容易做错的地方）**：

1. `connect`（`:790`）—— 连接路径，**透传**三元组给调用方，用于取 `LinkMode`。
2. `recover_session_impl` 里的 `handshake(l, timeout)`（`:1208`）—— **恢复路径**，它把结果解构成 `let (mut l, mut status) = handshake(...)`。这里**只取前两个元素**，写成 `let (mut l, mut status, _) = handshake(l, timeout)`。

**不要在 `:1208` 那里把 observations 转成 `DeviceEvent`。** 恢复发生在一条**已经断掉的**旧链路上，它关心的是「控制器重启后能不能重新握手」，与「当前是哪条链路」无关；在那里发 `LinkWarning` 会在每次自动恢复时都弹一次链路提示，而用户此刻真正需要看到的是恢复是否成功。链路自检只在**新建立的连接**上做一次。

**也不要为了少改一处而给 `handshake` 加默认参数或重载。** 直接改成三元组、把两个调用点都显式写出来；`_` 那处要带一行注释说明为什么丢弃（见上）。

所有调用点（`connect`、`AutoConnect`、`BtConnect`、`Recover` 相关路径）同步更新。改完后**全仓库 grep `handshake(` 确认没有遗漏**——包括 `#[cfg(test)]` 里的 `mock_handshake` 辅助函数（`:3664-3680`）：它自己也调用了 `handshake` 并把结果 `.map(|(_, status)| status)`，必须改成 `.map(|(_, status, _)| status)`，否则测试模块编译不过，而这类错误只在跑测试时才暴露。

连接成功后按连接的端口 `kind`（USB 从 `serialport` 判断，蓝牙从 `PortInfo.kind`）取 `link_warning(kind, &parse_link_mode(&observations))`，为 `Some(msg)` 时发 `DeviceEvent::LinkWarning(msg)`。`commands.rs` 的 `map_event` 加 `DeviceEvent::LinkWarning(m) => json!({"type":"link_warning","message":m})`。

- [ ] **Step 4: 运行，确认通过**

`cargo test --bins` → Expected: **`206 passed; 0 failed; 10 ignored`**（**+3**：本任务的 3 个新测试 —— `parse_link_mode`、`link_warning`、分片读取）。

- [ ] **Step 5: 提交**

```bash
git add desktop/src/bluetooth.rs desktop/src/device.rs desktop/src/commands.rs
git commit -m "feat(device): warn when both USB and bluetooth links are live"
```

---

### Task 9: 连接失败文案与蓝牙断链退避

`open_link` 现在把任何 `io::Error` 原样抛给用户（「系统找不到指定的文件。」）；断链重连只有两次无间隔尝试，对蓝牙不够。**USB 的重连行为必须一字不动。**

**Files:**
- Modify: `desktop/src/bluetooth.rs`（`describe_open_error` / `reconnect_delay`，均为纯函数）
- Modify: `desktop/src/device.rs:674-691`（`open_link` 用 `describe_open_error`）、`desktop/src/device.rs:2424-2511`（只为蓝牙加退避分支）
- Test: `desktop/src/bluetooth.rs` 内联 `mod tests`

**Interfaces:**
- Consumes: Task 7 的 `should_rebind_spp`、Task 4 的 `backend()` / `choose_port_for_mac`、Task 3 的 `PortKind`。
- Produces:
  - `pub fn describe_open_error(kind: std::io::ErrorKind) -> Option<&'static str>`
  - `pub fn reconnect_delay(attempt: u8) -> Option<std::time::Duration>`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn open_failures_explain_themselves_in_chinese() {
    use std::io::ErrorKind;
    assert!(describe_open_error(ErrorKind::NotFound).unwrap().contains("端口已消失"));
    assert!(describe_open_error(ErrorKind::PermissionDenied).unwrap().contains("被其它程序占用"));
    assert!(describe_open_error(ErrorKind::Other).is_none());
}

#[test]
fn reconnect_backoff_is_capped_and_gives_up() {
    use std::time::Duration;
    assert_eq!(reconnect_delay(0), Some(Duration::from_secs(1)));
    assert_eq!(reconnect_delay(1), Some(Duration::from_secs(2)));
    assert_eq!(reconnect_delay(2), Some(Duration::from_secs(4)));
    assert_eq!(reconnect_delay(3), Some(Duration::from_secs(4)));
    assert_eq!(reconnect_delay(4), Some(Duration::from_secs(4)));
    assert_eq!(reconnect_delay(5), None);
}
```

- [ ] **Step 2: 运行，确认失败**

`cargo test --bins bluetooth::tests::open_failures`。Expected: `cannot find function describe_open_error`。

- [ ] **Step 3: 实现**

- `describe_open_error`：`NotFound` → `Some("端口已消失，请重新扫描设备列表")`；`PermissionDenied` → `Some("端口被其它程序占用，请关闭占用该端口的程序后重试")`；其余 `None`。
  **这一条同时就是设计文档 §4.7 第 2 点「跨进程排他」的实现**：Windows 的 COM 口默认独占打开（`serialport` 的 `CreateFile` 不共享），第二个进程开同一端口会直接拿到 `PermissionDenied`，不需要额外写排他探测代码 —— 缺的只是把错误翻译成人话。
- `reconnect_delay`：`attempt < 5` → `Some(Duration::from_secs(1u64 << attempt.min(2)))`；否则 `None`。
- `open_link`（`desktop/src/device.rs:674-691`）：`.open()` 的 `Err(e)` 分支改成 `return Err(match crate::bluetooth::describe_open_error(e.kind()) { Some(msg) => msg.to_string(), None => e.to_string() })`。**超时、串口参数、DTR/RTS 一律不动。**
- 记住连接时的链路身份：`DeviceService` 加一个 `connected_link: Option<(PortKind, Option<String>)>` 字段，与 `connected_port` 同时写入（`Connect` / `AutoConnect` / `BtConnect` / 重连成功 各一处）、同时清空。
- 重连分支（`desktop/src/device.rs:2424-2511`）：**先判 `connected_link`**。`Some((PortKind::Bluetooth, Some(mac)))` → 走新的退避循环；其余（含 USB）**原样保留现有两次尝试的代码，一个字都不改**。退避循环：`for attempt in 0.. { let Some(delay) = reconnect_delay(attempt) else { break }; std::thread::sleep(delay); if recovery.stop_requested.load(...) { break } … }`，每轮内先 `backend().ensure_spp(mac)` + `spp_ports()` + `choose_port_for_mac` 拿新端口名，再 `open_link` + `recover_session_with_reference(l, 3s, false, false, &mut physical_reference)`。5 轮全败发 `"Automatic recovery failed after five bounded attempts: {reason}"`（**这句是 Rust 侧诊断串，按 Global Constraints 保持英文**）。成功后沿用现有 `discard_recovery_commands` / `Connected` / `Status` / `Stopped` 收尾。

- [ ] **Step 4: 运行，确认通过**

`cargo test --bins` → Expected: **`208 passed; 0 failed; 10 ignored`**（206 + 本任务的 2 个：`open_failures_explain_themselves_in_chinese`、`reconnect_backoff_is_capped_and_gives_up`）。

- [ ] **Step 5: 提交**

```bash
git add desktop/src/bluetooth.rs desktop/src/device.rs
git commit -m "fix(bluetooth): explain open failures and back off reconnect attempts"
```

---

### Task 10: UI —— 蓝牙设备面板

**Files:**
- Create: `desktop/ui/src/btPanelModel.ts`、`desktop/ui/src/btPanelModel.test.mjs`
- Modify: `desktop/ui/src/App.tsx`（端口下拉 `:3248-3251`、`poll_device` 的 `switch`、新增蓝牙面板）

**Interfaces:**
- Consumes: `poll_device` 的 `bt_devices` / `bt_error` / `link_warning`；`PortInfo.kind` / `PortInfo.mac`。
- Produces: `btPanelModel.ts` 导出 `portLabel(p)`、`deviceLabel(d)`、`silentReconnectTarget(rememberedMac, ports)`。

- [ ] **Step 1: 写失败测试**

`desktop/ui/src/btPanelModel.test.mjs`：

```js
import test from "node:test";
import assert from "node:assert/strict";
import {portLabel, deviceLabel, silentReconnectTarget} from "./btPanelModel.ts";

test("usb ports keep the CH340 badge and bluetooth ports say bluetooth", () => {
  assert.equal(
    portLabel({name:"COM13", description:"USB-SERIAL CH340 1A86:7523", preferred:true, kind:"usb"}),
    "COM13 · CH340 USB-SERIAL CH340 1A86:7523"
  );
  assert.equal(
    portLabel({name:"COM7", description:"蓝牙串口 · 70:4B:CA:72:AC:A2", preferred:true, kind:"bluetooth", mac:"70:4B:CA:72:AC:A2"}),
    "COM7 · 蓝牙串口 · 70:4B:CA:72:AC:A2"
  );
  assert.equal(
    portLabel({name:"COM6", description:"Serial port", preferred:false, kind:"other"}),
    "COM6 Serial port"
  );
});

test("legacy ports without a kind still render", () => {
  assert.equal(portLabel({name:"COM5", description:"Serial port", preferred:false}), "COM5 Serial port");
});

test("bluetooth devices show pairing state in chinese", () => {
  assert.equal(deviceLabel({mac:"70:4B:CA:72:AC:A2", name:"BtWriter", remembered:true, authenticated:false}), "BtWriter（已记忆，未认证）");
  assert.equal(deviceLabel({mac:"70:4B:CA:72:AC:A2", name:"BtWriter", remembered:false, authenticated:false}), "BtWriter（未配对）");
  assert.equal(deviceLabel({mac:"41:42:BC:65:9F:A8", name:"ROSE", remembered:true, authenticated:true}), "ROSE（已配对）");
});

test("silent reconnect only targets the remembered mac that is really present", () => {
  assert.equal(silentReconnectTarget(null, []), null);
  assert.equal(silentReconnectTarget("70:4B:CA:72:AC:A2", [{name:"COM13", kind:"usb"}]), null);
  assert.equal(
    silentReconnectTarget("70:4B:CA:72:AC:A2", [{name:"COM7", kind:"bluetooth", mac:"70:4B:CA:72:AC:A2"}]),
    "COM7"
  );
});
```

- [ ] **Step 2: 运行，确认失败**

`node --test desktop/ui/src/btPanelModel.test.mjs`。Expected: 找不到模块。

- [ ] **Step 3: 实现**

`btPanelModel.ts` 纯函数，无 React 依赖。`portLabel`：`kind === "usb"` → `"{name} · CH340 {description}"`；`kind === "bluetooth"` → `"{name} · {description}"`（**描述里已经由 Rust 侧拼好「蓝牙串口 · <MAC>」，前端不要再拼一次 MAC**）；其他（含 `kind` 缺失）→ `"{name} {description}"`。`deviceLabel`：`authenticated` → `"{name}（已配对）"`；`remembered` → `"{name}（已记忆，未认证）"`；否则 `"{name}（未配对）"`。`silentReconnectTarget(rememberedMac, ports)`：`rememberedMac` 为空返回 `null`；否则找 `kind === "bluetooth" && mac === rememberedMac` 的那条，返回其 `name`，找不到返回 `null`。

`App.tsx`：端口下拉 `:3248-3251` 改成 `{portLabel(p)}`；`poll_device` 的 `switch` 加 `"bt_devices"`（存进新 state）、`"bt_error"` 与 `"link_warning"`（走现有 `notify(message, true)`）；在 `port-row` 下方加「扫描蓝牙设备」按钮 → `command({type:"bt_scan"})`，设备列表每项给「连接」→ `command({type:"bt_connect", mac})` 与「忘记」→ `command({type:"bt_forget", mac})`。用现有 `<button className="icon-button bordered">` 风格与中文文案。

**记住上次成功的 MAC（不是 COM 名），启动时静默重连一次**：连接成功时把该端口的 `mac` 写进 `localStorage`（键名沿现有约定，如 `"openink-bt-mac"`，读写方式照 `App.tsx:534` 的 `localStorage` 写法）；启动后第一次收到 `ports` 时调 `silentReconnectTarget(savedMac, ports)`，非 `null` 且当前未连接、且 `autoConnect` 为真 → `command({type:"connect", port, swap_xy, flip_y})`。**只尝试一次**，失败**不弹窗**，只把状态设为 `{ state: "蓝牙未连接" }`（不要覆盖现有的 `{ state: "未连接" }` 默认值语义）。原因是设计文档 §4.4 要求「失败不弹窗，只在状态栏提示」——每次启动都弹一个红色提示会让用户以为坏了。

**另外满足设计文档 §4.6 的「一键改回 USB」**：在连接状态区域（`connected` 为真时）显示当前链路类型；当蓝牙已连接时，紧邻显示一个「改用 USB 连接」按钮 —— 先 `command({type:"disconnect"})`，再从 `ports` 里挑 `kind === "usb"` 的那条 `command({type:"connect", port, swap_xy, flip_y})`。这样机器被手机抢占时用户不必翻菜单即可切回有线。

- [ ] **Step 4: 运行，确认通过**

```
node --test desktop/ui/src/btPanelModel.test.mjs
cd desktop/ui && node ./node_modules/typescript/bin/tsc -b
```
Expected: 测试全绿；`tsc` exit 0 无输出。

- [ ] **Step 5: 提交**

```bash
git add desktop/ui/src/btPanelModel.ts desktop/ui/src/btPanelModel.test.mjs desktop/ui/src/App.tsx
git commit -m "feat(ui): add bluetooth device panel and link-kind port labels"
```

---

### Task 11: 真机验收与文档

**Files:**
- Create: `docs/calibration/bluetooth-verification.md`
- Modify: `CHANGELOG.md`（顶部加未发布条目）

**Interfaces:**
- Consumes: 前十个任务的全部实现。

- [ ] **Step 1: 准备「全新电脑」的等效起点**

```
cd tools/btprobe
bin/Release/net9.0-windows10.0.26100.0/win-x64/btprobe.exe spoff 70:4B:CA:72:AC:A2
bin/Release/net9.0-windows10.0.26100.0/win-x64/btprobe.exe ports
```
Expected: `COM7` 已从列表消失。

- [ ] **Step 2: 构建并启动 GUI（唯一允许 `tauri dev` 的时刻，必须由人在场）**

```
cd openink-studio/desktop
npm run dev
```

- [ ] **Step 3: 走查设计文档 §6 的八条剧本，逐条记录**

1. 「扫描蓝牙设备」→ 列表出现 `BtWriter（未配对）` → 点「连接」→ 自动建 SPP 口 → 握手成功 → 状态栏显示已连接。
2. 关闭 GUI 再打开 → **不再手动点连接**，界面自动静默重连上一次的 MAC 并成功；若写字机没开机，状态栏显示「蓝牙未连接」且**不弹窗**。
3. 连接中拔掉写字机电源再上电 → 5 次退避内恢复，且不重复下发运动指令。
4. 列表里不出现 `COM3`/`COM5`/`COM16`（幽灵口）与耳机口 `COM4`/`COM6`/`COM17`。
5. 蓝牙连接下空跑一次 → 轨迹与 USB 一致。
6. 断开 → 改连 USB → 行为与 0.5.2 一致。
7. USB 已连时再点蓝牙「连接」→ 出现明确的拒绝或警告，不出现两个会话同时下发。
8. 蓝牙连上后移出范围 10 s 再移回 → 出现「重建端口并重试」，不干等。

- [ ] **Step 4: 写验收记录**

`docs/calibration/bluetooth-verification.md`：每条一行「日期 / 结果 / 观察到的行为」，附 `$I` 原文与冷启动耗时。**失败的条目照实写「未通过」并记下错误串**，不要写「基本正常」。文件开头另起一节写明兼容性口径（设计文档 §4.5）：**Windows 10 1809+ / Windows 11 x64，不需要安装额外驱动，不需要管理员权限**；本节结论以本机实测为准，即 `Windows 11 26200 x64 / 非管理员账户` 下八条全跑过。

- [ ] **Step 5: 全量回归**

```
cd openink-studio/desktop
CARGO_HOME=<repo>/.cache/cargo RUSTUP_HOME=<repo>/.cache/rustup TMP=<repo>/.cache/linktmp TEMP=<repo>/.cache/linktmp \
  <repo>/.cache/cargo/bin/cargo.exe test --bins
node --test desktop/ui/src/*.test.mjs
cd ui && node ./node_modules/typescript/bin/tsc -b
```
Expected: **`208 passed; 0 failed; 10 ignored`**；UI 测试全绿；`tsc` exit 0。

- [ ] **Step 6: 提交**

```bash
git add docs/calibration/bluetooth-verification.md CHANGELOG.md
git commit -m "docs: record bluetooth real-machine verification"
```

---

## 已知遗留（本计划不解决）

- 设计文档 §4.2 的「名称 `BtWriter` 弱匹配」一档**故意不实现**：自动连接只认 MAC（Task 5 的 `auto_select_candidate` 里的蓝牙候选全部来自 `choose_port_for_mac` 的 MAC 强匹配），比按名字猜更严格。名字只在 UI 里给人看，不参与选口。
- 已记忆设备未落盘 `LinkKey`（`auth=0 remembered=16`），换电脑需重新 `SetServiceState` —— 设计文档 §7 未知项 6。
- 不同蓝牙芯片（Intel/Realtek/Qualcomm）的 SPP 建口表现未验证 —— §7 未知项 5。
- 手机抢占同一台写字机的行为未验证 —— §7 未知项 1。
