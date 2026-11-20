# 蓝牙连接验收记录

## 兼容性口径

适用范围：**Windows 10 1809+ / Windows 11 x64**。

- **不需要安装额外驱动**：走的是 Windows 自带的蓝牙 SPP 虚拟串口，配对成功后由系统创建 COM 口，程序只打开那个口。
- **不需要管理员权限**：配对用的是 `bthprops` 的 `BluetoothSetServiceState`，当前用户态即可完成。
- 本节结论以本机实测为准，即 **Windows 11 26200 x64 / 非管理员账户（`zuolee`）** 下的结果。

## 状态说明

本文分两部分：

- **§1 自动可验证部分** —— 由脚本或测试直接跑出结果，已执行，附原始输出。
- **§2 人工走查部分** —— 设计文档 §6 的十四项，需要在 GUI 前操作（扫描、点连接、拔电源、移出范围）。**截至本文档创建时这十四项全部尚未由人工执行**，下表状态一律写「待人工执行」，不预填通过。

## §1 自动可验证部分

### 1.1 单元与集成测试

执行环境：`cargo` 与 `node` 均为仓库内工具链，`TMP`/`TEMP` 指向 `.cache/linktmp`（否则链接器报 `LNK1104`）。

| 日期 | 项目 | 结果 | 观察 |
| --- | --- | --- | --- |
| 2026-11-19 | `cargo test --bins` | **通过** | `211 passed; 0 failed; 10 ignored; finished in 3.03s` |
| 2026-11-19 | `node --test desktop/ui/src/*.test.mjs` | **通过** | 15 个文件 / `tests 75` / `pass 75` / `fail 0` |
| 2026-11-20 | `cargo test --bins`（修 rc=87 后） | **通过** | `213 passed` / `0 failed` / `10 ignored` |
| 2026-11-19 | `node ./node_modules/typescript/bin/tsc -b` | **通过** | exit 0，无输出 |

10 个 ignored 全部是需要外部素材的既有用例（手写样本、真实项目文件等），与蓝牙无关；177→211 的增量即本次蓝牙功能的测试。

### 1.2 本机蓝牙硬件与 SPP 口现状

执行：`tools/btprobe/.../btprobe.exe ports` 与 `... ls`。

```
=== bluetooth SPP nodes ===
  蓝牙链接上的标准串行 (COM3)    mac=00:00:00:00:00:00   LOCALMFG&0000\...&0&000000000000_00000002   <- 幽灵口
  蓝牙链接上的标准串行 (COM16)   mac=00:00:00:00:00:00   LOCALMFG&0000\...&0&000000000000_00000013   <- 幽灵口
  蓝牙链接上的标准串行 (COM5)    mac=00:00:00:00:00:00   LOCALMFG&0000\...&0&000000000000_00000016   <- 幽灵口
  蓝牙链接上的标准串行 (COM4)    mac=11:11:11:11:27:A9   LOCALMFG&0046\...&0&1111111127A9_C00000000   <- 耳机
  蓝牙链接上的标准串行 (COM6)    mac=41:42:BC:65:9F:A8   LOCALMFG&0046\...&0&4142BC659FA8_C00000000   <- 耳机
  蓝牙链接上的标准串行 (COM7)    mac=70:4B:CA:72:AC:A2   LOCALMFG&0046\...&0&704BCA72ACA2_C00000000   <- 写字机
  蓝牙链接上的标准串行 (COM17)   mac=A4:62:1A:9C:26:EA   VID&000105d6_PID&000a\...&0&A4621A9C26EA_C00000000  <- 耳机
```

`BtWriter 70:4B:CA:72:AC:A2 auth=0 remembered=16 connected=0 class=0x001F00` —— **未认证但已记忆**，`auth=0` 说明 Windows 未落盘 `LinkKey`。

### 1.3 冷状态下的「端口消失 → 重建 → 打开 → 握手」

Step 1 要求的「全新电脑等效起点」已执行：`btprobe spoff 70:4B:CA:72:AC:A2` 后 `ports` 里 `70:4B:CA:72:AC:A2` 计数为 0（COM7 消失）；随后 `btprobe spp 70:4B:CA:72:AC:A2` 重新创建，COM7 回来，实例路径与前次一致。

重新绑定后立即以 `device.rs` 同样的串口参数打开并握手：

```
first_status_ms=49.4
status: [MSG:BT Connected with 4C:23:38:0C:97:4C]
<Idle|MPos:0.000,0.000,0.000|FS:0,0|Pn:XY>
```

随后的 `$I` 原文：

```
[VER:1.3a.20221114:]
[OPT:MPHB]
[MSG:Mode=BT:Name=BtWriter(70:4B:CA:72:AC:A2):Status=Connected with 4C:23:38:0C:97:4C]
ok
```

结论：**`BluetoothSetServiceState(ENABLE)` 会重建 SPP 口；重建后冷启动首字节约 49 ms**，远在握手窗口 3 s 之内。`$I` 报 `Status=Connected with <本机适配器 MAC>`，说明链路确实握到了写字机而不是别的设备。

> 注意：这验证的是「删口后重建」这一条路径，**没有**验证「设备断电导致端口留下陈旧行、再由 `ensure_spp` 驱逐」那条承重假设 —— 那需要拔机器电源，见 §2 第 14 条。

### 1.4 端口占用的错误分类

`describe_open_error` 按「端口是否仍在 `available_ports()` 列表里」区分占用与消失，不依赖 OS 文本（文本是本地化的）。这项行为有单元测试覆盖（`open_failures_explain_themselves_in_chinese` 的三态断言），**但没有跨进程实测**，见 §2 第 12 条。

### 1.5 已启用的 SPP 服务不是失败（2026-11-20 真机发现）

`BluetoothSetServiceState(ENABLE)` 在服务**已经启用**时返回 `ERROR_INVALID_PARAMETER`（87）。实测三态：

| 服务状态 | 返回 |
| --- | --- |
| 未启用（`spoff` 之后） | `rc=0` |
| 已启用（刚启用过） | `rc=87` |
| 已启用（早已启用） | `rc=87` |

`:84` 曾把这个返回值直接当失败，于是「扫描 → 点连接」在一台**已配对**的机器上必然报「蓝牙操作失败（错误码 87）」，而「自动识别连接」因为不经过 `ensure_spp` 反而成功。这个差异只有真机走查能撞到：当时 `cargo test --bins` 211 全绿，`rc=87` 一次都没被触发。

现行修法：`ensure_spp` 的错误被保留而不是立即返回，端口出现即成功；只有「enable 失败**且**端口没出现」才把系统错误报给用户。两条测试钉住这个分界（`an_already_enabled_spp_service_is_not_a_failure`、`a_failed_enable_with_no_port_still_reports_the_system_error`），变异验证：回退该修复 → 前者失败（212 passed / 1 failed）。

顺带补掉了 §3 第 2 条里 M6 变异（整个跳过 `ensure_spp`）无法被杀死的缺口 —— `wait_for_spp_port` 抽出 `wait_for_spp_port_with`，后端可注入。

### 1.6 启动静默重连的两条路径

启动时记住的 MAC 有两条路：端口仍在列表里就直接 `connect`，端口不在（机器关机时 Windows 会丢掉 SPP 行）就退回 `bt_connect` 让后端先重建服务。**这个决策已被纯函数 `launchReconnect` 钉住**（`desktop/ui/src/btPanelModel.test.mjs`，含「没有记住 MAC → 不尝试」「端口在 → 用端口」「端口不在 → 用 MAC」「别的机器/同名 USB 口不得顶替」五组）。变异验证：把 `{ mac: rememberedMac }` 退回改成 `null` → 74 passed / 1 failed，抓到了。

**接线（App.tsx 里把 `launchReconnect` 的结果发成哪个命令）仍未覆盖** —— 没有任何 UI 测试 import `App.tsx`，见 §3 第 2 条的末段。

## §2 人工走查部分

以下十四项需要在 GUI 前手工操作。执行方式：

```
cd openink-studio/desktop
npm run dev
```

| # | 剧本 | 状态 | 观察记录 |
| --- | --- | --- | --- |
| 1 | 扫描 → 列表出现 `BtWriter（已记忆，未认证）` → 点「连接」→ 自动建 SPP 口 → 握手成功。**若本机从未与该机器配对过，预期先失败并提示去 Windows 设置配对**（见 §3 第 13 条），配对后重试成功 | **2026-11-20 通过**（走查中发现并修掉了 rc=87 缺陷，见 §1.5） | 端口下拉列出 COM7 蓝牙串口与 COM13 CH340；扫描出 `BtWriter（已记忆，未认证）`；修复后点「连接」直接成功 |
| 2 | 关闭 GUI 再打开 → 自动静默重连上次的 MAC；机器没开机时状态栏显示「蓝牙未连接」且不弹窗 | 待人工执行 | |
| 3 | 连接中拔电源再上电 → 5 次退避内恢复，不重复下发运动指令 | 待人工执行 | |
| 4 | 设备列表里不出现幽灵口与耳机口 | 待人工执行 | |
| 5 | 蓝牙下空跑一次 → 轨迹与 USB 一致 | 待人工执行 | |
| 6 | 断开 → 改连 USB → 行为与 0.5.2 一致 | 待人工执行 | |
| 7 | USB 已连时再点蓝牙「连接」→ 明确拒绝，不出现两个会话 | 待人工执行 | |
| 8 | 连上后移出范围 10 s 再移回 → 重建端口并重试，不干等 | 待人工执行 | |
| 9 | 恢复中按「停止」→ 约 200 ms 内中止，显示已停止，不出现「五次尝试都失败」 | 待人工执行 | |
| 10 | 掉线后日志面板每轮一行「蓝牙重连第 N/5 次失败：…」 | 待人工执行 | |
| 11 | USB 拔线制造断链 → 错误串与 0.5.2 逐字节一致 | 待人工执行 | |
| 12 | 用别的程序占住 COM13 再点连接 → 显示「端口被其它程序占用…」；解除占用后正常 | 待人工执行 | |
| 13 | 写字机关机后点它的旧端口 → 显示「端口已消失…」而不是「被占用」 | 待人工执行 | |
| 14 | 掉线重连时若端口号变化（COM7→COM8），后续点动与作业发到新端口 | 待人工执行 | |

第 3、9、10、14 条尤其重要：退避循环的**接线**没有自动化测试（`worker_loop` 无注入点，只有 `reconnect_delay` 与 `wait_in_slices` 两个纯函数有单测），真断链重连只能靠人工跑到。

## §3 已知遗留

1. **`poll_for_port` 的承重假设未验证**：`BluetoothSetServiceState(ENABLE)` 是否真能驱逐失败的那条陈旧注册表行。若不能，`poll_for_port` 会立刻返回同一个 COM 名、第二次 `connect` 同样失败。看 §2 第 14 条 —— 若重绑后端口号**从不变化**，该假设就不成立。
2. **接线层的覆盖缺口（已用变异逐条实测，不是推测）**：在 `HEAD` 的干净副本里做变异，跑 `cargo test --bins`：

   | 变异 | 位置 | 结果 |
   | --- | --- | --- |
   | C：把两条具体排除串换成 blanket `"Recovery "` | `desktop/src/bluetooth.rs` `should_rebind_spp` | **抓到了**：`rebind_still_retries_the_message_that_actually_means_a_dead_link` 失败，210 passed / 1 failed |
   | A：把端口存在性谓词取反（`still_listed` → `!still_listed`） | `desktop/src/device.rs:802-810` | **没抓到**：211 passed / 0 failed |
   | B：把 `Stopped` 分支改成发 `Failed` 文案 | `desktop/src/device.rs` 断链块的 `BluetoothRecovery` 三路 match | **没抓到**：211 passed / 0 failed |
   | M1：`handshake` 不再收集 observations | `desktop/src/device.rs` `handshake` | **没抓到**：211 passed / 0 failed |
   | M2：删掉 `emit_link_warning` 的函数体 | `desktop/src/device.rs` `emit_link_warning` | **没抓到**：211 passed / 0 failed |
   | M3：`log_idle_bluetooth_link` 加 `if false &&` 前置 | `desktop/src/device.rs` `log_idle_bluetooth_link` | **没抓到**：211 passed / 0 failed |
   | M4：`still_listed` 恒置 `None` | `desktop/src/device.rs:802-810` | **没抓到**：211 passed / 0 failed |
   | M5：把 `failed_identity` 的读取移到 `connected_link = None` 之后 | `desktop/src/device.rs` 断链块 | **没抓到**：211 passed / 0 failed |
   | M6：`wait_for_spp_port` 里跳过 `ensure_spp` | `desktop/src/device.rs` `wait_for_spp_port` | **已能抓到**（2026-11-20 抽出 `wait_for_spp_port_with` 后可注入后端，见 §1.5；此前是没抓到） |

   没抓到的五条原因相同：正确性活在 `worker_loop` 的接线里，而该循环没有注入点 —— `open_link` 直接调 `serialport::available_ports()`、`handshake` 直接读串口、`BluetoothRecovery` 的三路 `match` 又直接 `emit`。**把它们整体删掉，测试套件依然全绿**：observation → `LinkWarning` / idle 日志这条链路、端口下拉的重绑路径、蓝牙退避的触发点，都没有自动化测试兜底。要补需要给 `worker_loop` 加可注入的 backend 抽象，属于结构调整而非补测试，本期未做。**因此 §2 第 3、9、10、12、13、14 条人工走查不能跳过。**

   UI 侧相反：`btPanelModel.test.mjs` 的 5 组变异抓到 4 组（去掉 `p.kind === "bluetooth"`、交换 `authenticated`/`remembered`、去掉 CH340 标记、删掉 `portLabel` 的蓝牙分支都会失败），只有删掉 `if (!rememberedMac) return null;` 守卫能存活，无害。但**没有任何 UI 测试 import `App.tsx`**，所以本次在投票环里做的状态修补（`pendingBtMac` 清理、`connectedVia` 推导、`pendingUsbSwitch`、启动重连）都不在测试覆盖内 —— 其中启动重连的**决策**已被 `launchReconnect` 的单测钉住，接线仍未。
3. **「停止」只保证退避等待可中断**：若 `Stop` 正好落进某一次 `reconnect_bluetooth_once` 内部，仍需等完那一次（约 12 s），200 ms 上界不覆盖该情形。
4. **占用/消失区分未在非中文 Windows 上验证**：判定逻辑本身不依赖语言，但没有任何非中文环境可测。
5. **已记忆设备未落盘 `LinkKey`**（`auth=0 remembered=16`），换电脑需重新 `SetServiceState` —— 设计文档 §7 未知项 6。
6. **不同蓝牙芯片的 SPP 建口表现未验证**（Intel/Realtek/Qualcomm）—— §7 未知项 5。
7. **手机抢占同一台写字机的行为未验证** —— §7 未知项 1。
8. **端口下拉里仍会列出幽灵口与耳机口**，显示为「COM3 · 蓝牙串口 · 00:00:00:00:00:00」这类文案。它们被自动连接正确排除（MAC 可解析且非全零才会成为候选），但人工从下拉里仍选得到。属已知的展示层瑕疵，未在本期修。
9. **非 Windows 平台不提供该功能**：`backend()` 在非 Windows 上返回错误「蓝牙连接目前只在 Windows 上提供」，端口仍照旧列出但不会标成蓝牙。
10. **`flipY` 默认开着一处 USB 行为差异（不属蓝牙，来自同分支里的 `6f00043`）**：`App.tsx:555-560` 在没有 `localStorage` 值时返回 `true`，而 0.5.2 完全没有 `flip_y`。**全新安装的机器上 USB 直连的 Y 方向与 0.5.2 相反**，直到用户自己找到开关（该开关在连接期间禁用）。这与 §6 第 6 条验收「改回 USB 行为与 0.5.2 一致」冲突。默认值 `true` 是为本店 `$3=7` 的机器设的，属机器相关配置，不是通解。
11. **`$3` 从未被读取**：UI 注释与设置说明都引用 `$3=7` 作为默认 Y 翻转的理由，但 `desktop/src` 与 `desktop/ui/src` 里没有任何地方读 `$3`（`grep '"\$3'` → 零命中）。该值是人工核对的产物，不是程序推出的结论。
12. **预览与执行对停靠点的 Y 符号不一致（不属蓝牙，可能画错位置）**：`commands.rs:604-611` 在 anchor-0 预览里对 park 取负，而 `positioning.rs:200-206` 原样透传（`positioning.rs:299-331` 的单测钉的是未取负的形式）。两者必有一个错。任何一条都会让笔停到错误的位置，建议单独排查。
13. **`ensure_spp` 会配对但不认证，从不走配对 API**：`ensure_spp` 只调 `BluetoothGetDeviceInfo` + `BluetoothSetServiceState`，全仓没有任何 `BluetoothAuthenticateDevice`/`PairAsync` 调用（设计 §4.3 禁止静默配对）。副作用是 `auth=0 remembered=16` 的「已记忆未认证」状态 —— 设备**能连但没被认证**。因此若某台全新机器在 `SetServiceState` 后仍建不出 SPP 口，`describe_error(1244)` 会提示用户先去 Windows 设置里配对。**首次连接的实际表现（1244 失败 vs 连上但空闲）尚未在本机验证**，见 §2 第 1 条。
14. **`Connect` arm 的重绑有不可达前置条件**：它要求 SPP 行仍被枚举到、且 open 因可重绑的原因失败。而在主要的死链路场景里行已经消失，`ports().ok()` 取不到 MAC → 不重绑，用户看到的是「端口已消失」。**真正能重建端口的是 `BtConnect`（它手里有 MAC）**，所以从端口下拉直接连一个蓝牙口得到的是较弱的行为。设计 §4.4 那条规则在 `bt_connect` 路径上可达，在端口下拉路径上恰好在该规则针对的场景里不可达。
15. **错误信息跨层丢失**：`BtPortError::NotReady`/`Failed` 到 IPC 层收敛成 `bt_error` 或普通 `Error`，原先产生事件的 MAC 被丢弃（本版已从 `BtPortNotReady` 里移除 `mac` 字段，因为无人读），所以 UI 无法说明**是哪台**设备建口失败。
