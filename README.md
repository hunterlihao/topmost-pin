# 窗口置顶助手（TopMostPin）

把**任意窗口**钉在最上层 —— 一个便携的 Windows 桌面小工具。双击即用，无需安装。

- **两种置顶方式**：鼠标指向目标窗口按 `Ctrl+Alt+P`，或在界面列表里点窗口左边的图钉
- **便携**：单目录，数据全在 `data\`，删掉文件夹即彻底卸载
- **单色 light 界面**：黑白两色，无彩色。设计说明见 [`docs/design.md`](docs/design.md)

---

## 技术栈

**Tauri v2 + Rust + Vue 3 + Vite**，单 exe 便携版，零安装、零运行时依赖（除系统自带的 WebView2）。

选型基于本机实测，不是偏好：

| 事实 | 影响 |
|---|---|
| `cargo` 在本机**无法联网**（schannel 凭证问题） | 只能用离线缓存中的 crate。`tauri-plugin-global-shortcut`、`global-hotkey`、`image`、`async-broadcast` 都不在缓存中 → 全部不用 |
| Go 路线需要 Wails v2 作 WebView 宿主 | 缓存里只有 wails v1.16.9；且 gcc 缺失时 cgo 路线不可行 → 排除 |
| Windows 11 自带 WebView2 运行时 | Tauri 的运行前提已满足 |
| `windows-sys 0.61.2`、`winreg 0.55`、Tauri v2 全家桶 | 均已在离线缓存 → 直接调用 Win32，无需任何绑定生成器 |
| JDK 为 1.8（无 jpackage）、.NET 只有运行时无 SDK | 两者均无法构建可交付产物 → 排除 |

**核心结论：全局热键与窗口置顶全部走 `windows-sys` 直调，不依赖任何 Tauri 插件。**

---

## 快速开始

```powershell
# 开发（Vite HMR + Tauri 热重载）
./scripts/dev.ps1

# 产出便携版 -> dist-portable\窗口置顶助手\
./scripts/build-portable.ps1
```

构建流水线依次执行：`pnpm install` → `vue-tsc` 类型检查 → `vite build` → `cargo test` → `cargo build --release` → 组装目录。

全程 `CARGO_NET_OFFLINE=true`。

---

## 用法

| 操作 | 结果 |
|---|---|
| 鼠标指向窗口，按 `Ctrl+Alt+P` | 置顶；再按一次取消 |
| 界面列表点左侧图钉 | 同上 |
| 界面点「全部取消」 | 一次取消所有置顶 |
| 关闭主窗口 | 收进托盘（程序继续守护已置顶窗口） |
| 双击托盘图标 | 重新打开界面 |
| 托盘右键 → 退出 | 真正退出，并自动取消全部置顶 |

**热键是一组候选，不是一个固定值。** 依次尝试 `Ctrl+Alt+P` → `Ctrl+Shift+P` →
`Ctrl+Alt+T` → `Ctrl+Alt+A` → `Ctrl+Alt+W` → `Ctrl+Alt+K`，用第一个能注册成功的，
并把**实际生效的那个**显示在界面报头和设置页。全部被占用时，键帽会变成虚线，
提示也改说「点图钉置顶」。

这样做的原因很实际：本机 `Ctrl+Alt+P` 已被其它程序注册
（`RegisterHotKey` 返回 1409 `ERROR_HOTKEY_ALREADY_REGISTERED`），
写死单一组合在装了同类工具的机器上会直接失效。

已置顶的窗口在列表里**整行反相**（实心墨底），并排在列表最上方；与其余窗口之间以一条
2px 粗墨线分隔 —— 这条线就是「水面线」，之上是被抬起的窗口。

---

## 架构

```
src-tauri/src/
├── winapi.rs      Win32 直调层：枚举 / 标题 / 类名 / 进程名 / 置顶读写
├── pin.rs         置顶集合管理（Win32 通过 Win32Ops trait 注入，可脱离真实窗口测试）
├── hotkey.rs      热键线程：RegisterHotKey + SetTimer + 自有 GetMessage 循环
├── autostart.rs   winreg 直写 HKCU\...\Run
├── tray.rs        动态托盘菜单（已置顶窗口即为可点击的取消入口）
├── commands.rs    Tauri 命令层
├── events.rs      pin://changed / pin://error 事件
├── state.rs       全局状态
└── lib.rs         装配

src/               Vue 3 前端（types / api / stores / views / components）
```

### 两条独立的消息泵

```
主线程   ── tao 的 Win32 事件循环（WebView2 窗口 + Tauri 命令）
热键线程 ── 自有 GetMessageW 循环
             ├─ WM_HOTKEY → 切换光标下窗口的置顶
             └─ WM_TIMER  → 每秒重断言 + 剔除已销毁窗口
```

**为什么热键必须自建线程**：`RegisterHotKey(hwnd=NULL, ...)` 投递的 `WM_HOTKEY` 是**线程消息**
（窗口句柄为空）。Tauri 主线程的 `DispatchMessage` 对没有窗口的消息无处分发，会静默丢弃它。

---

## 三个关键设计决策

### 1. 置顶状态依据「自维护集合」，不读 `WS_EX_TOPMOST`

提示框、输入法候选窗、桌宠等窗口**天然**就带 topmost 位。若按 EXStyle 判断「是否已置顶」，
光标指向这类窗口时第一次按键会变成「取消置顶」—— 用户看到的是什么都没发生。

实测旁证：本机 `dsh-pet 桌宠（桌面模式）` 启动即带 topmost 位。

### 2. 重断言定时器与热键解耦

每秒把被别的程序抢回去的 `WS_EX_TOPMOST` 重新置位。只在**用户可见状态真的变化**时
（窗口被销毁而剔除）才发事件 —— 否则前端会被每秒唤醒一次。

### 3. 退出时自动取消全部置顶

程序退出后没人再维护置顶，留着只会让那些窗口永远赖在最上层。异常崩溃留下的残留，
重新运行并点一次「全部取消」即可清除。

---

## 测试与验证状态

```powershell
cd src-tauri
cargo test --offline --features custom-protocol                    # 11 项
cargo test --offline --features custom-protocol -- --ignored       # +6 项真实窗口 E2E
```

| 层 | 数量 | 内容 |
|---|---|---|
| 单元测试 | 9 | 置顶集合状态机：切换往返、幂等、失败不入集合、重断言补位、并发一致性 |
| 集成测试 | 2 | 枚举过滤契约（非空标题、不含本进程）、进程名解析 |
| 真实窗口 E2E | 6 | `SetWindowPos` 真的改写 `WS_EX_TOPMOST`；**跨进程**置顶（含从子线程调用，模拟热键线程）；PinManager 全生命周期；窗口销毁后自动剔除；本进程窗口被拒绝；沙箱环境探针 |

另有三个脚本，全部可重复运行：

| 脚本 | 作用 | 实测结果 |
|---|---|---|
| `scripts/winprobe.ps1` | P/Invoke 直读 `WS_EX_TOPMOST`，给验收提供不依赖本程序自报的判据 | 可用 |
| `scripts/smoke-test.ps1` | 启动产物：GUI 子系统（无控制台黑窗）、主窗口创建、便携数据目录、WebView2 缓存、单实例、**截图验证界面真的渲染了**（不是白屏） | 11/11 通过 |
| `scripts/acceptance-test.ps1` | 真实投递 `WM_HOTKEY`（等价按键）后用独立探针断言目标窗口已置顶 | 见下方说明 |

### 关于端到端验收：一个环境限制

`acceptance-test.ps1` 在本项目的开发环境（DSH 自动化沙箱）里**无法完成置顶断言**，退出码 2。
这不是产品缺陷，已经用 A/B 对照证明：

| 调用方 | 进程深度 | 对系统外窗口（explorer / chrome / idea64）调用 `SetWindowPos` |
|---|---|---|
| DSH 直接派生的 pwsh | 直接子进程 | **成功**（`返回=True err=0`） |
| `cargo test` 派生的测试进程 | 孙进程 | **失败** `ERROR_ACCESS_DENIED(5)` |
| 被测应用（`Start-Process` 启动） | 孙进程 | **失败** `ERROR_ACCESS_DENIED(5)` |

同一 API、同一窗口、同一标志位，**只有进程深度不同**。一份用法显然正确的纯 Rust 控制台程序
（`probe_pinning_a_window_outside_the_sandbox`）与被测应用表现完全一致 —— 因此这是沙箱按进程深度
施加的限制，而不是代码问题。

`acceptance-test.ps1` 已经把这个对照做成**内置的控制条件**：它先跑 Rust 探针，探针也被拒时
就判定为环境限制并跳过（退出码 2），避免把环境问题误报成产品缺陷。

**最终确认方式**：把 `dist-portable\窗口置顶助手\` 整个目录拷到任意位置，**在资源管理器里双击 exe**
（这样它就是资源管理器的子进程，不受自动化沙箱约束），然后指向任意窗口按热键。


---

## 踩过的坑

1. **`[dependencies.windows-sys]` 子表会吞掉后续依赖**
   TOML 的表头一直生效到下一个表头。写成子表后，`serde`、`dirs` 等全部被归入该子表，
   静默失效 → 编译报「cannot find module `dirs`」。**必须写成内联表。**

2. **`RegisterHotKey` / `MOD_*` 不在 `WindowsAndMessaging`**
   它们在 `Win32::UI::Input::KeyboardAndMouse`，需要额外开 `Win32_UI_Input_KeyboardAndMouse`
   feature。

3. **`cargo metadata --offline` 会失败，但 `cargo build --offline` 正常**
   `tauri-plugin-single-instance` 依赖 Linux 专用的 `zbus` → `async-broadcast`，
   而后者不在缓存中。metadata 会解析**所有平台**，build 只取当前 target 所需。

4. **`tauri-plugin-autostart` 离线不可用**
   依赖链含未缓存的 `async-broadcast`。改用 `winreg` 直写注册表。

5. **`HWND` 在 windows-sys 里是裸指针且非 `Send`**
   不能直接放进跨线程集合。对外统一用 `isize`（`HwndRaw`），只在调用瞬间转回 `HWND`。

6. **release profile 不能用 `codegen-units = 1` + `lto = "fat"`**
   本机带实时防病毒扫描，rustc 写入后立即删除 40MB+ 目标文件时会被占用，报 `os error 32`。
   已改用 `lto = "thin"` + `codegen-units = 16`。

---

## 已知限制

- **以管理员身份运行的程序无法被置顶**。这是 Windows 的完整性级别限制（UIPI），
  非提权进程无法操作更高完整性级别的窗口。界面会明确提示，不会静默失败。
- **`Ctrl+Alt+P` 可能与其它软件冲突**。注册失败时界面提示「热键未生效」，
  此时仍可用界面列表正常置顶。
- **不显示窗口图标**。图标需要 PNG 编码，而 `image` crate 不在离线缓存中。
  列表改用等宽字体的进程名 + 类名 —— 这本来也更可扫读。
- **不持久化置顶状态**。窗口句柄重启后即失效，存了也没有意义。

---

## 系统要求

Windows 10 / 11 64 位，需要 WebView2 运行时（Win10 1803+ 与 Win11 已预装）。
