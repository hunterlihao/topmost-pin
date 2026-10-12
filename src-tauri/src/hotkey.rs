//! 全局热键线程 + 置顶重断言定时器。
//!
//! ★ 为什么必须自建线程和消息循环：
//!
//! `RegisterHotKey(hwnd=NULL, ...)` 注册的热键，其 `WM_HOTKEY` 会投递到**调用线程
//! 的消息队列**，而不是某个窗口。Tauri 主线程跑的是 tao 的 Win32 消息循环，它
//! 用 `DispatchMessage` 分发消息 —— 而 hwnd 为空的消息没有窗口过程可分发，
//! 于是会被静默丢掉。所以热键必须在自己的线程上注册并自己泵消息。
//!
//! 同理，`SetTimer(hwnd=NULL, ...)` 的 `WM_TIMER` 也是线程消息，放在同一个循环里
//! 处理最自然。两条路径共用一个 `GetMessageW` 循环，退出时投一个 `WM_QUIT`。
//!
//! ★ 为什么是一组候选热键而不是一个固定值：
//! 本机实测 `Ctrl+Alt+P` 已被其它程序注册（RegisterHotKey 返回 1409
//! ERROR_HOTKEY_ALREADY_REGISTERED）。固定单一组合在装了同类工具的机器上会直接
//! 失效，所以依次尝试候选组合，用第一个成功的，并把**实际生效的那个**上报界面。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageW, KillTimer, PostThreadMessageW, SetTimer, MSG, WM_HOTKEY, WM_QUIT, WM_TIMER,
};

use crate::error::{AppError, AppResult};
use crate::events;
use crate::log;
use crate::state::AppState;
use crate::winapi;

/// 全部候选都注册失败时界面上显示的占位文案
pub const FALLBACK_HOTKEY: &str = "未生效";

/// 候选热键，按优先级从上到下尝试。
///
/// 选键原则：Ctrl+Alt 系优先（不与系统快捷键冲突），P 优先（Pin 的联想），
/// 其余作为退路。注意这里存的是**组合串**而不是修饰键掩码 —— 候选和用户自定义
/// 走同一个解析器，候选就不是绕过校验的后门（有单测盯着每个候选都能解析）。
const CANDIDATES: &[&str] = &[
    "Ctrl+Alt+P",
    "Ctrl+Shift+P",
    "Ctrl+Alt+T",
    "Ctrl+Alt+A",
    "Ctrl+Alt+W",
    "Ctrl+Alt+K",
];

const HOTKEY_ID: i32 = 1;
const TIMER_ID: usize = 1;

/// 自定义线程消息：请求换一个热键（WM_APP + 1）
const WM_APP_REKEY: u32 = 0x8000 + 1;

/// 换键时等热键线程回执的上限
const REBIND_TIMEOUT: Duration = Duration::from_secs(3);

/// `RegisterHotKey` 在组合已被占用时返回的错误码
const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

/// 重断言间隔。1s 足够追上绝大多数「抢回置顶」的行为，又不会太频繁地惊动目标进程。
const REASSERT_MS: u32 = 1000;

/* ── 组合键 ───────────────────────────────────────────────────────────── */

/// 一个热键组合：展示文案 + 修饰键掩码 + 虚拟键码。
///
/// 展示文案是**唯一**的对外表示 —— 界面显示的、落库的、日志里写的都是它。
/// 这样「用户设的」「界面显示的」「下次启动读到的」天然是同一个字符串，
/// 不会出现两套表示对不上的情况。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Combo {
    pub label: String,
    pub mods: u32,
    pub vk: u32,
}

/// 解析 `"Ctrl+Alt+P"` 形式的组合串。
///
/// 规则刻意收得很紧：
///   · **必须至少有一个修饰键** —— 否则会吞掉全局按键，单按一个 P 就置顶，
///     整个系统都没法正常打字了；
///   · 只接受字母 / 数字 / F1~F24 —— 其余键位（方向键、多媒体键……）的虚拟键码
///     在不同键盘布局下不稳定，宁可先明确不支持；
///   · 不认识的片段直接报错，不做「猜一个最接近的」—— 静默降级会让用户
///     以为设置生效了，然后按下去没反应。
pub fn parse(input: &str) -> AppResult<Combo> {
    let mut mods = 0u32;
    let mut vk: Option<u32> = None;

    for raw in input.split('+') {
        let part = raw.trim();
        if part.is_empty() {
            continue;
        }
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            // 不支持就说清楚。Win 键由系统独占，注册了也基本轮不到我们。
            "win" | "meta" | "super" => {
                return Err(AppError::msg("暂不支持 Win 键作为修饰键，请用 Ctrl / Alt / Shift"));
            }
            _ => {
                if vk.is_some() {
                    return Err(AppError::msg(format!("「{input}」里出现了多个主键")));
                }
                vk = Some(parse_key(part)?);
            }
        }
    }

    let Some(vk) = vk else {
        return Err(AppError::msg("热键需要一个主键（字母 / 数字 / F1~F24）"));
    };
    if mods == 0 {
        return Err(AppError::msg("热键至少需要一个修饰键（Ctrl / Alt / Shift）"));
    }
    Ok(Combo {
        // 统一成规范写法，避免 "ctrl+alt+p" 与 "Ctrl+Alt+P" 变成两个不同的值
        label: label_of(mods, vk),
        mods,
        vk,
    })
}

fn parse_key(part: &str) -> AppResult<u32> {
    let up = part.to_ascii_uppercase();
    let bytes = up.as_bytes();

    if bytes.len() == 1 {
        let c = bytes[0];
        // VK_A..VK_Z 恰好就是 'A'..'Z'，VK_0..VK_9 恰好就是 '0'..'9'
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Ok(c as u32);
        }
    }
    if let Some(rest) = up.strip_prefix('F') {
        if let Ok(n) = rest.parse::<u32>() {
            if (1..=24).contains(&n) {
                return Ok(0x70 + n - 1); // VK_F1 = 0x70
            }
        }
    }
    Err(AppError::msg(format!(
        "无法识别的按键「{part}」（支持字母 / 数字 / F1~F24）"
    )))
}

fn label_of(mods: u32, vk: u32) -> String {
    let mut s = String::new();
    if mods & MOD_CONTROL != 0 {
        s.push_str("Ctrl+");
    }
    if mods & MOD_ALT != 0 {
        s.push_str("Alt+");
    }
    if mods & MOD_SHIFT != 0 {
        s.push_str("Shift+");
    }
    s.push_str(&key_label(vk));
    s
}

fn key_label(vk: u32) -> String {
    if (0x41..=0x5A).contains(&vk) {
        return char::from_u32(vk).unwrap_or('?').to_string();
    }
    if (0x30..=0x39).contains(&vk) {
        return char::from_u32(vk).unwrap_or('?').to_string();
    }
    if (0x70..=0x87).contains(&vk) {
        return format!("F{}", vk - 0x70 + 1);
    }
    format!("VK_{vk:#X}")
}

fn register(id: i32, combo: &Combo) -> bool {
    unsafe {
        RegisterHotKey(
            std::ptr::null_mut(),
            id,
            combo.mods | MOD_NOREPEAT,
            combo.vk,
        ) != 0
    }
}

fn unregister(id: i32) {
    unsafe {
        UnregisterHotKey(std::ptr::null_mut(), id);
    }
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

/// 把 `RegisterHotKey` 的错误码翻译成人话。用户看到的不该是个数字。
fn describe_register_error(err: u32) -> String {
    if err == ERROR_HOTKEY_ALREADY_REGISTERED {
        "这个组合已经被其它程序占用了，换一个试试".to_string()
    } else {
        format!("注册失败（错误码 {err}）")
    }
}

/* ── 热键线程 ─────────────────────────────────────────────────────────── */

/// 换键请求：序号 + 新的组合串 + 回执通道
type RebindRequest = (u64, String, SyncSender<Result<String, String>>);

/// 换键请求的序号。用来区分「槽里还是我那条」与「已经被后来的请求取代」——
/// 无条件清槽会把别人正在等的请求一起抹掉。
static REBIND_SEQ: AtomicU64 = AtomicU64::new(1);

/// 进程正在退出。
///
/// 置上之后热键线程不再做任何会改变置顶状态、或者会去惊动界面的事。
/// ★ 退出流程必须靠它：那个 1Hz 的重断言定时器如果还在跑，就会把 `unpin_all`
/// 刚取消的窗口重新顶回去，而进程马上就要消失 —— 那个窗口会**永久**留在最上层，
/// 而下次启动的集合是空的、根本不知道它的存在。
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// 通知热键线程：进程要退出了，别再动置顶状态
pub fn begin_shutdown() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

fn shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

pub struct HotkeyHandle {
    thread_id: AtomicU32,
    /// 是否成功注册了某个热键
    pub active: AtomicBool,
    /// 初始注册已经尝试过了。
    /// 注意它**不代表注册成功**，只代表「现在可以换键了」——
    /// 换键请求要等这个为真再发，否则会撞上线程还没建好消息队列的窗口期。
    ready: AtomicBool,
    /// 实际生效的热键文案；未生效时是 FALLBACK_HOTKEY
    display: Mutex<String>,
    /// 待处理的换键请求。
    /// 走共享槽位而不是消息参数，是因为 `PostThreadMessageW` 只能带两个整数，
    /// 塞不下一个组合串。
    pending: Mutex<Option<RebindRequest>>,
}

impl HotkeyHandle {
    fn new() -> Self {
        Self {
            thread_id: AtomicU32::new(0),
            active: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            display: Mutex::new(FALLBACK_HOTKEY.to_string()),
            pending: Mutex::new(None),
        }
    }

    /// 是否可以开始换键了
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }

    /// 实际生效的热键文案
    pub fn display(&self) -> String {
        self.display.lock().clone()
    }

    /// 把线程里的注册结果同步成对外可见的状态。
    /// 只有热键线程会调用它 —— 别的线程写会和注册结果打架。
    fn publish(&self, current: Option<&Combo>) {
        self.active.store(current.is_some(), Ordering::SeqCst);
        *self.display.lock() = current
            .map(|c| c.label.clone())
            .unwrap_or_else(|| FALLBACK_HOTKEY.to_string());
    }

    /// 换一个全局热键，**等新键的注册结果出来再返回**。
    ///
    /// 注册必须在热键线程上做（见模块头：`WM_HOTKEY` 是线程消息），所以这里走
    /// 「投一个线程消息 + 等回执」。这样前端能直接拿到「这个组合到底行不行」，
    /// 而不是先显示成功、过一会儿发现按下去没反应。
    ///
    /// 失败时原热键保持不变 —— 见下面 `rebind` 里的恢复逻辑。
    pub fn rebind(&self, combo: String) -> AppResult<String> {
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid == 0 {
            return Err(AppError::msg("热键线程尚未就绪，稍后再试"));
        }

        let token = REBIND_SEQ.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(1);
        *self.pending.lock() = Some((token, combo, tx));

        if unsafe { PostThreadMessageW(tid, WM_APP_REKEY, 0, 0) } == 0 {
            self.recycle(token);
            return Err(AppError::msg("无法通知热键线程"));
        }

        match rx.recv_timeout(REBIND_TIMEOUT) {
            Ok(Ok(label)) => Ok(label),
            Ok(Err(e)) => Err(AppError::msg(e)),
            Err(_) => {
                // ★ 只回收**自己那一条**。
                //
                // 无条件 take 会连后到的请求一起抹掉：两条请求重叠时，先到的那条
                // 超时后把后到的那条也清空，于是两个调用方都收到「线程没有响应」，
                // 而用户真正想设的那个组合根本没生效。
                if self.recycle(token) {
                    Err(AppError::msg("热键线程没有响应"))
                } else {
                    Err(AppError::msg("换键请求被后一次请求取代，请重试"))
                }
            }
        }
    }

    /// 只有当槽里还是 `token` 这一条时才回收它，返回是否真的回收了
    fn recycle(&self, token: u64) -> bool {
        let mut slot = self.pending.lock();
        if slot.as_ref().is_some_and(|(t, _, _)| *t == token) {
            slot.take();
            true
        } else {
            false
        }
    }

    /// 让热键线程退出：往它的消息队列投一个 WM_QUIT，`GetMessageW` 随即返回 0。
    pub fn stop(&self) {
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                PostThreadMessageW(tid, WM_QUIT, 0, 0);
            }
        }
    }
}

/// 启动热键线程，立即返回句柄（注册结果稍后由 `active` / `display` 反映）
pub fn spawn(app: AppHandle) -> Arc<HotkeyHandle> {
    let handle = Arc::new(HotkeyHandle::new());
    let worker = handle.clone();
    if let Err(e) = thread::Builder::new()
        .name("topmost-pin-hotkey".to_string())
        .spawn(move || run(app, worker))
    {
        // 线程起不来不该让整个程序死掉（界面仍可正常置顶，只是没有全局热键
        // 与重断言），但**必须留下记录**：release 是 GUI 子系统，没有控制台，
        // 不写日志的话这就是一次查不出原因的「双击没反应」。
        log::write(
            "hotkey",
            format!("热键线程启动失败：{e}，全局热键与重断言将不可用"),
        );
    }
    handle
}

fn run(app: AppHandle, handle: Arc<HotkeyHandle>) {
    let tid = unsafe { GetCurrentThreadId() };
    handle.thread_id.store(tid, Ordering::SeqCst);
    log::write("hotkey", format!("线程启动 tid={tid}"));

    // 启动时的候选顺序，优先级从高到低：
    //   1. 用户明确设定过的（在设置页改过，或 --set-hotkey 指定）
    //   2. 上次实际生效的（可能是自动挑的候选）—— 用来保持稳定，
    //      否则机器上可用组合一变，每次启动都会换一个，用户记不住
    //   3. 内置候选列表
    let (explicit, last) = match app.try_state::<AppState>() {
        Some(s) => (
            s.db.get_setting(crate::db::keys::HOTKEY),
            s.db.get_setting(crate::db::keys::HOTKEY_LAST),
        ),
        None => (None, None),
    };

    let mut order: Vec<Combo> = Vec::new();
    if let Some(v) = explicit.as_deref() {
        push_candidate(&mut order, v, "优先尝试用户设定的热键");
    }
    if let Some(v) = last.as_deref() {
        push_candidate(&mut order, v, "其次尝试上次生效的热键");
    }
    for label in CANDIDATES {
        push_candidate(&mut order, label, "候选");
    }

    let mut current: Option<Combo> = None;
    for c in &order {
        if register(HOTKEY_ID, c) {
            current = Some(c.clone());
            break;
        }
        log::write(
            "hotkey",
            format!("{} 被占用（err={}），试下一个", c.label, last_error()),
        );
    }

    handle.publish(current.as_ref());
    match current.as_ref() {
        Some(c) => {
            log::write("hotkey", format!("注册成功：{}", c.label));
            // ★ 只记「实际生效的」。**不能**顺手写 keys::HOTKEY —— 那会把
            //   用户明确设过的组合，因为一次临时冲突就覆盖成自动挑的候选。
            mark_effective(&app, &c.label);
        }
        None => log::write("hotkey", "全部候选都被占用，热键不可用"),
    }
    // 从这一刻起可以接受换键请求了（注意：不代表上面一定注册成功）
    handle.ready.store(true, Ordering::SeqCst);

    // 注册结果要显示在界面上，所以无论成败都推一次事件
    events::emit_changed(&app);

    // 定时器与热键无关：界面点出来的置顶同样需要被守护，所以无条件启用
    let timer = unsafe { SetTimer(std::ptr::null_mut(), TIMER_ID, REASSERT_MS, None) };
    if timer == 0 {
        // 不能无条件写「已启动」：那样日志会显示一切正常，而实际上被别的程序
        // 抢走的置顶从此不会被补回，用户只能看到「有时候失效」。
        log::write("hotkey", format!("重断言定时器启动失败（err={}）", last_error()));
    } else {
        log::write("hotkey", "重断言定时器已启动（1s）");
    }

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    // GetMessageW: >0 有消息，0 收到 WM_QUIT，-1 出错 —— 后两者都该退出
    loop {
        let r = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if r <= 0 {
            log::write("hotkey", format!("消息循环退出 r={r}"));
            break;
        }
        match msg.message {
            WM_HOTKEY => {
                log::write("hotkey", "收到 WM_HOTKEY");
                on_hotkey(&app);
            }
            WM_TIMER => on_tick(&app),
            WM_APP_REKEY => {
                let Some((_, combo, reply)) = handle.pending.lock().take() else {
                    continue;
                };
                let result = rebind(&app, &mut current, &combo);
                handle.publish(current.as_ref());
                // 先把结果给调用方，再推事件 —— 让「设置成功」先于界面刷新
                let _ = reply.send(result);
                events::emit_changed(&app);
            }
            _ => {}
        }
    }

    unsafe {
        KillTimer(std::ptr::null_mut(), TIMER_ID);
    }
    if current.is_some() {
        unregister(HOTKEY_ID);
    }
    // ★ 必须复位：线程 ID 会被系统复用，留着旧值的话后续的 `stop()` / `rebind()`
    // 会把消息投给同进程里一个**无关**的线程（若它有消息循环，一个 WM_QUIT
    // 就能把它直接干掉）。
    handle.thread_id.store(0, Ordering::SeqCst);
}

/// 往候选顺序里加一条。解析不了就跳过并记下来 —— 尤其能暴露数据库里存了脏值。
fn push_candidate(order: &mut Vec<Combo>, label: &str, source: &str) {
    match parse(label) {
        // 用户设的恰好就是某个候选时会出现重复
        Ok(c) if order.contains(&c) => {}
        Ok(c) => {
            log::write("hotkey", format!("{source}：{label}"));
            order.push(c);
        }
        Err(e) => log::write("hotkey", format!("{source}「{label}」解析失败：{e}，跳过")),
    }
}

/// 记录「这次实际生效的热键」。每次注册成功都写。
fn mark_effective(app: &AppHandle, label: &str) {
    save(app, crate::db::keys::HOTKEY_LAST, label);
}

/// 记录「用户明确设定的热键」。**只在用户主动改键时**写。
fn mark_user_choice(app: &AppHandle, label: &str) {
    save(app, crate::db::keys::HOTKEY, label);
}

fn save(app: &AppHandle, key: &str, label: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Err(e) = state.db.set_setting(key, label) {
            log::write("hotkey", format!("保存热键设置失败：{e}"));
        }
    }
}

/// 在热键线程上换键。
///
/// ★ 失败时**把原来的热键装回去**。把用户丢在一个「没有热键」的状态里是最糟的结果：
/// 他会以为设置坏了，而不是「这个组合被占用了，换一个」。
fn rebind(app: &AppHandle, current: &mut Option<Combo>, input: &str) -> Result<String, String> {
    let next = parse(input).map_err(|e| e.to_string())?;

    match swap(current, &next, |label| mark_user_choice(app, label)) {
        Ok(()) => {
            log::write("hotkey", format!("热键已改为 {}", next.label));
            mark_effective(app, &next.label);
            Ok(next.label)
        }
        Err(err) => {
            log::write(
                "hotkey",
                format!("{} 注册失败（err={err}），已尝试恢复原热键", next.label),
            );
            Err(describe_register_error(err))
        }
    }
}

/// 换键的核心：注册新组合，失败则恢复旧的。
///
/// 单独抽出来是为了能真的测这段回滚 —— 它只需要 Win32，不需要 Tauri。
/// 返回 `Err(错误码)` 表示没换成（此时 `current` 保持原样）。
fn swap(current: &mut Option<Combo>, next: &Combo, save: impl FnOnce(&str)) -> Result<(), u32> {
    // 同一个 ID 不能同时注册两个组合，所以先摘下旧的再装新的；
    // 装不上就在下面把旧的原样装回去。
    if current.is_some() {
        unregister(HOTKEY_ID);
    }

    if register(HOTKEY_ID, next) {
        save(&next.label);
        *current = Some(next.clone());
        return Ok(());
    }

    let err = last_error();
    if let Some(old) = current.take() {
        if register(HOTKEY_ID, &old) {
            // 这行是回滚是否真的发生了的**唯一证据** ——
            // 单元测试能用探针去抢来验证，脚本只能靠日志。
            log::write("hotkey", format!("已恢复原热键 {}", old.label));
            *current = Some(old);
        } else {
            // 理论上不该发生（刚刚还占着）。真发生了就明确记下来，
            // 界面会显示「未生效」，而不是假装一切正常。
            log::write("hotkey", format!("原热键 {} 恢复失败，当前没有可用热键", old.label));
        }
    }
    Err(err)
}

/// 热键按下：切换「光标下窗口」的置顶状态
fn on_hotkey(app: &AppHandle) {
    if shutting_down() {
        return; // 正在退出，别再改置顶状态
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    // 优先光标正下方（指哪打哪），回退到最近一次的前台窗口
    // （覆盖光标恰好停在任务栏或本程序窗口上的情况）
    let under = winapi::window_under_cursor();
    let last_fg = *state.last_foreground.lock();

    log::write(
        "hotkey",
        format!(
            "光标={:?} 光标下窗口={:?} 前台={:?} 最近前台={:?}",
            winapi::cursor_pos(),
            under.map(|h| format!("0x{h:X}")),
            winapi::foreground_window().map(|h| format!("0x{h:X}")),
            last_fg.map(|h| format!("0x{h:X}")),
        ),
    );

    let target = under
        .filter(|h| winapi::is_candidate(*h))
        .or_else(|| last_fg.filter(|h| winapi::is_candidate(*h)));

    let Some(hwnd) = target else {
        log::write("hotkey", "没有解析到可置顶的目标窗口");
        events::emit_error(app, AppError::NoTarget.to_string());
        return;
    };

    let title = winapi::title_of(hwnd);
    let pid = winapi::pid_of(hwnd);
    log::write(
        "hotkey",
        format!(
            "选中 hwnd=0x{hwnd:X} alive={} pid={pid} proc={} title=「{title}」",
            winapi::is_alive(hwnd),
            winapi::process_name(pid),
        ),
    );
    match state.toggle_pin(hwnd, &title) {
        Ok(pinned) => {
            log::write(
                "pin",
                format!("热键切换 hwnd=0x{hwnd:X} -> pinned={pinned} title=「{title}」"),
            );
            events::emit_changed(app);
            crate::tray::refresh(app);
        }
        Err(e) => {
            log::write("pin", format!("热键切换失败 hwnd=0x{hwnd:X}：{e}"));
            events::emit_error(app, e.to_string());
        }
    }
}

/// 定时器：记录前台窗口 + 重断言 + 剔除已销毁的窗口
fn on_tick(app: &AppHandle) {
    // ★ 退出时先停手。否则这一轮重断言会把 unpin_all 刚取消的窗口重新顶回去，
    // 而进程马上就要消失 —— 那个窗口会永久留在最上层。
    if shutting_down() {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    if let Some(fg) = winapi::foreground_window() {
        if winapi::is_candidate(fg) {
            *state.last_foreground.lock() = Some(fg);
        }
    }

    let removed = state.pins.reassert();
    if removed > 0 {
        log::write("pin", format!("重断言剔除 {removed} 个已销毁窗口"));
        // 只有「用户可见的状态真的变了」才通知，避免每秒唤醒前端
        events::emit_changed(app);
        crate::tray::refresh(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::PeekMessageW;

    /// 每个候选都必须能被解析器接受 —— 否则那一条就是一条**静默失效**的退路，
    /// 而它本来正是「首选被占用」时的兜底。这条测试是候选写法的主要护栏：
    /// 以后改候选时打个错字会立刻变红。
    #[test]
    fn every_candidate_parses_and_is_already_canonical() {
        for label in CANDIDATES {
            let c = parse(label).unwrap_or_else(|e| panic!("候选 {label} 无法解析：{e}"));
            assert_eq!(&c.label, label, "候选应当已经是规范写法");
        }
    }

    #[test]
    fn parse_is_case_space_and_order_insensitive() {
        let a = parse("ctrl+alt+p").expect("解析");
        let b = parse("  Alt + CTRL + P  ").expect("解析");
        assert_eq!(a.label, "Ctrl+Alt+P", "输出统一成规范写法");
        assert_eq!(a, b, "写法不同但语义相同，应当解析成同一个组合");
        assert_eq!(a.vk, 0x50);
        assert_eq!(a.mods, MOD_CONTROL | MOD_ALT);
    }

    #[test]
    fn accepts_letters_digits_and_function_keys() {
        assert_eq!(parse("Ctrl+A").expect("解析").vk, 0x41);
        assert_eq!(parse("Ctrl+Alt+7").expect("解析").vk, 0x37);
        assert_eq!(parse("Ctrl+F1").expect("解析").vk, 0x70);
        assert_eq!(parse("Ctrl+F24").expect("解析").vk, 0x87);
        assert_eq!(parse("Ctrl+Shift+F12").expect("解析").label, "Ctrl+Shift+F12");
    }

    /// 不带修饰键的组合一旦注册成功，那个键就被从整个系统里吞掉了
    /// （任何程序都收不到它）。这必须在解析层拦住 —— 不能指望 RegisterHotKey 报错。
    #[test]
    fn rejects_combo_without_modifier() {
        for input in ["P", "F5", "7"] {
            let e = parse(input).expect_err("不该接受");
            assert!(
                e.to_string().contains("修饰键"),
                "错误信息要讲清原因，实际是：{e}"
            );
        }
    }

    #[test]
    fn rejects_unknown_or_unsupported_input() {
        assert!(parse("Ctrl+Win+P").is_err(), "Win 键明确不支持");
        assert!(parse("Ctrl+F25").is_err(), "F25 超出 F1~F24");
        assert!(parse("Ctrl+F0").is_err());
        assert!(parse("Ctrl+PageUp").is_err(), "不认识的键位要报错而不是忽略");
        assert!(parse("Ctrl+A+B").is_err(), "两个主键要报错");
        assert!(parse("Ctrl").is_err(), "只有修饰键、没有主键");
        assert!(parse("").is_err());
        assert!(parse("+++").is_err());
    }

    /// 用户看到的不该是个数字
    #[test]
    fn register_error_is_readable() {
        let taken = describe_register_error(ERROR_HOTKEY_ALREADY_REGISTERED);
        assert!(taken.contains("占用"), "实际：{taken}");
        assert!(!taken.contains("1409"), "错误码不该出现在给用户看的文案里");

        let other = describe_register_error(5);
        assert!(other.contains('5'), "其它错误码至少要有据可查：{other}");
    }

    /* ── 换键（真注册真抢占）─────────────────────────────────────────── */

    /// 探测用的独立 ID。不能借用 HOTKEY_ID —— 同一个 ID 再次注册会**替换**掉
    /// 已有的那条，把测试正要用的注册顶掉。
    const PROBE_ID: i32 = 0x7FFE;

    /// 测试用的冷门组合池。
    ///
    /// ★ 这几条测试会**真的**往系统里注册全局热键，所以：
    ///   · 不能写死一个组合 —— 开发机上可能正跑着本程序（它自己占着一个），
    ///     也可能有别的软件占着。挑 F13~F18 是因为本程序自己用的是
    ///     Ctrl+Alt+P 系与用户设定的组合，几乎不可能撞上；
    ///   · 每个测试用**自己独占**的一段，不能共用。cargo test 默认并行跑，
    ///     共用的话两个测试会互相把对方刚探测到的空闲组合抢走，
    ///     表现为随机的「构造不出冲突」。
    const POOL_A: &[&str] = &["Ctrl+Alt+Shift+F13", "Ctrl+Alt+Shift+F14"];
    const POOL_B: &[&str] = &["Ctrl+Alt+Shift+F15", "Ctrl+Alt+Shift+F16"];
    const POOL_C: &[&str] = &["Ctrl+Alt+Shift+F17", "Ctrl+Alt+Shift+F18"];

    fn combo(label: &str) -> Combo {
        parse(label).unwrap_or_else(|e| panic!("{label} 解析失败：{e}"))
    }

    /// 在给定的候选池里找一个当前空闲的组合（试完立刻放掉）。`used` 里的排除。
    ///
    /// 池子全被占用时给出明确原因，而不是让后面的断言以「xx 被占用」这种
    /// 看起来像产品缺陷的形式失败。
    fn free_or_skip(pool: &[&str], used: &[Combo]) -> Combo {
        for label in pool {
            let c = combo(label);
            if used.contains(&c) {
                continue;
            }
            let ok = register(PROBE_ID, &c);
            if ok {
                unregister(PROBE_ID);
                return c;
            }
        }
        panic!(
            "候选池（{}）当前全被占用，这条换键测试的前提不成立",
            pool.join(" / ")
        );
    }

    /// 在**另一个线程**上占住某个组合，模拟「这个热键已经被别的程序注册了」。
    ///
    /// 必须换线程：`RegisterHotKey` 的冲突判定是跨线程的，同一线程换个 ID
    /// 未必会失败，那样就构造不出冲突，回滚路径也就测不到。
    struct Blocker {
        /// 是否真的占住了。false 说明前提不成立，测试要立刻失败而不是假装通过。
        held: bool,
        stop: Arc<AtomicBool>,
        join: Option<thread::JoinHandle<()>>,
    }

    impl Blocker {
        fn new(combo: &Combo) -> Self {
            const BLOCK_ID: i32 = 0x7FFF;
            let c = combo.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let ready = Arc::new(AtomicBool::new(false));
            let held = Arc::new(AtomicBool::new(false));

            let (stop2, ready2, held2) = (stop.clone(), ready.clone(), held.clone());
            let join = thread::spawn(move || unsafe {
                // 先碰一下消息队列：`RegisterHotKey(NULL, ...)` 注册给调用线程，
                // 线程还没有队列时会失败。
                let mut msg: MSG = std::mem::zeroed();
                PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 0);

                let ok = RegisterHotKey(
                    std::ptr::null_mut(),
                    BLOCK_ID,
                    c.mods | MOD_NOREPEAT,
                    c.vk,
                ) != 0;
                held2.store(ok, Ordering::SeqCst);
                ready2.store(true, Ordering::SeqCst);

                while !stop2.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(10));
                }
                if ok {
                    UnregisterHotKey(std::ptr::null_mut(), BLOCK_ID);
                }
            });

            while !ready.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(5));
            }
            Self {
                held: held.load(Ordering::SeqCst),
                stop,
                join: Some(join),
            }
        }
    }

    impl Drop for Blocker {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    #[test]
    fn swap_registers_the_new_combo_and_saves_it() {
        let old = free_or_skip(POOL_A, &[]);
        let next = free_or_skip(POOL_A, std::slice::from_ref(&old));
        assert!(register(HOTKEY_ID, &old), "占住旧组合");

        let mut current = Some(old);
        let mut saved = String::new();
        swap(&mut current, &next, |label| saved.push_str(label)).expect("换键应当成功");

        assert_eq!(current.as_ref(), Some(&next), "current 应当换成新的那个");
        assert_eq!(saved, next.label, "成功时要落库");

        unregister(HOTKEY_ID);
    }

    /// ★ 换到一个**已被占用**的组合时，必须把原来的热键装回去。
    ///
    /// 这是用户最容易撞到的路径（本机 Ctrl+Alt+P 就被别的程序占着），
    /// 处理错的后果是「试一次失败，热键整个没了」。
    #[test]
    fn swap_restores_the_previous_combo_when_the_new_one_is_taken() {
        let taken = free_or_skip(POOL_B, &[]);
        let blocker = Blocker::new(&taken);
        assert!(blocker.held, "构造不出冲突，这条测试就没有意义");

        let old = free_or_skip(POOL_B, std::slice::from_ref(&taken));
        assert!(register(HOTKEY_ID, &old), "占住旧组合");

        let mut current = Some(old.clone());
        let mut saved = String::new();
        let err = swap(&mut current, &taken, |label| saved.push_str(label))
            .expect_err("被占用的组合不该注册成功");

        assert_eq!(err, ERROR_HOTKEY_ALREADY_REGISTERED);
        assert_eq!(current.as_ref(), Some(&old), "失败后 current 必须还是原来那个");
        assert!(saved.is_empty(), "失败时不该落库");

        // 「变量没变」不等于「系统里还占着」—— 换个线程去抢一下才算数
        let probe = Blocker::new(&old);
        assert!(!probe.held, "失败后原热键必须仍然注册在系统里");

        unregister(HOTKEY_ID);
        drop(blocker);
    }

    /// 首次设置（原本没有热键）失败时，不该留下任何残留注册
    #[test]
    fn swap_leaves_nothing_when_first_attempt_is_taken() {
        let taken = free_or_skip(POOL_C, &[]);
        let blocker = Blocker::new(&taken);
        assert!(blocker.held, "构造不出冲突");

        let mut current: Option<Combo> = None;
        let mut saved = false;
        let err = swap(&mut current, &taken, |_| saved = true).expect_err("应当失败");

        assert_eq!(err, ERROR_HOTKEY_ALREADY_REGISTERED);
        assert!(current.is_none(), "本来就没有热键，失败后也不该凭空多一个");
        assert!(!saved, "失败时不该落库");
        drop(blocker);
    }
}
