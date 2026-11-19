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
| 2026-11-19 | `node --test desktop/ui/src/*.test.mjs` | **通过** | 15 个文件 / `tests 74` / `pass 74` / `fail 0` |
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

## §2 人工走查部分

以下十四项需要在 GUI 前手工操作。执行方式：

```
cd openink-studio/desktop
npm run dev
```

| # | 剧本 | 状态 | 观察记录 |
| --- | --- | --- | --- |
| 1 | 扫描 → 列表出现 `BtWriter（未配对）` → 点「连接」→ 自动建 SPP 口 → 握手成功 | 待人工执行 | |
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
2. **退避接线的覆盖缺口（已用变异逐条实测，不是推测）**：在 `HEAD` 的干净副本里做三组变异，跑 `cargo test --bins`：

   | 变异 | 位置 | 结果 |
   | --- | --- | --- |
   | C：把两条具体排除串换成 blanket `"Recovery "` | `desktop/src/bluetooth.rs` `should_rebind_spp` | **抓到了**：`rebind_still_retries_the_message_that_actually_means_a_dead_link` 失败，210 passed / 1 failed |
   | A：把端口存在性谓词取反（`still_listed` → `!still_listed`） | `desktop/src/device.rs:806-808` | **没抓到**：211 passed / 0 failed |
   | B：把 `Stopped` 分支改成发 `Failed` 文案 | `desktop/src/device.rs:3058` | **没抓到**：211 passed / 0 failed |

   A 与 B 抓不到的原因是同一个：两者的正确性都活在 `worker_loop` 的接线里，而该循环没有注入点 —— `open_link` 直接调 `serialport::available_ports()`，`BluetoothRecovery` 的三路 `match` 又直接 `emit`。函数本身有测试，把它们接到循环上的那一层没有。要补需要给 `worker_loop` 加可注入的 backend 抽象，属于结构调整而非补测试，本期未做。**因此 §2 第 9、10、12、13 条人工走查不能跳过。**
3. **「停止」只保证退避等待可中断**：若 `Stop` 正好落进某一次 `reconnect_bluetooth_once` 内部，仍需等完那一次（约 12 s），200 ms 上界不覆盖该情形。
4. **占用/消失区分未在非中文 Windows 上验证**：判定逻辑本身不依赖语言，但没有任何非中文环境可测。
5. **已记忆设备未落盘 `LinkKey`**（`auth=0 remembered=16`），换电脑需重新 `SetServiceState` —— 设计文档 §7 未知项 6。
6. **不同蓝牙芯片的 SPP 建口表现未验证**（Intel/Realtek/Qualcomm）—— §7 未知项 5。
7. **手机抢占同一台写字机的行为未验证** —— §7 未知项 1。
8. **端口下拉里仍会列出幽灵口与耳机口**，显示为「COM3 · 蓝牙串口 · 00:00:00:00:00:00」这类文案。它们被自动连接正确排除（MAC 可解析且非全零才会成为候选），但人工从下拉里仍选得到。属已知的展示层瑕疵，未在本期修。
9. **非 Windows 平台不提供该功能**：`backend()` 在非 Windows 上返回错误「蓝牙连接目前只在 Windows 上提供」，端口仍照旧列出但不会标成蓝牙。
