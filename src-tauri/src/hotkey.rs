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

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageW, KillTimer, PostThreadMessageW, SetTimer, MSG, WM_HOTKEY, WM_QUIT, WM_TIMER,
};

use crate::error::AppError;
use crate::events;
use crate::log;
use crate::state::AppState;
use crate::winapi;

/// 全部候选都注册失败时界面上显示的占位文案
pub const FALLBACK_HOTKEY: &str = "未生效";

/// 候选热键：(展示文案, 修饰键, 虚拟键码)。按优先级从上到下尝试。
///
/// 选键原则：Ctrl+Alt 系优先（不与系统快捷键冲突），P 优先（Pin 的联想），
/// 其余作为退路。同一次运行只会注册成功一个。
const CANDIDATES: &[(&str, u32, u32)] = &[
    ("Ctrl+Alt+P", MOD_CONTROL | MOD_ALT, 0x50), // P
    ("Ctrl+Shift+P", MOD_CONTROL | MOD_SHIFT, 0x50),
    ("Ctrl+Alt+T", MOD_CONTROL | MOD_ALT, 0x54), // T
    ("Ctrl+Alt+A", MOD_CONTROL | MOD_ALT, 0x41), // A
    ("Ctrl+Alt+W", MOD_CONTROL | MOD_ALT, 0x57), // W
    ("Ctrl+Alt+K", MOD_CONTROL | MOD_ALT, 0x4B), // K
];

const HOTKEY_ID: i32 = 1;
const TIMER_ID: usize = 1;

/// 重断言间隔。1s 足够追上绝大多数「抢回置顶」的行为，又不会太频繁地惊动目标进程。
const REASSERT_MS: u32 = 1000;

pub struct HotkeyHandle {
    thread_id: AtomicU32,
    /// 是否成功注册了某个热键
    pub active: AtomicBool,
    /// 实际生效的热键文案；未生效时是 FALLBACK_HOTKEY
    display: Mutex<String>,
}

impl HotkeyHandle {
    fn new() -> Self {
        Self {
            thread_id: AtomicU32::new(0),
            active: AtomicBool::new(false),
            display: Mutex::new(FALLBACK_HOTKEY.to_string()),
        }
    }

    /// 实际生效的热键文案
    pub fn display(&self) -> String {
        self.display.lock().clone()
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
    thread::Builder::new()
        .name("topmost-pin-hotkey".to_string())
        .spawn(move || run(app, worker))
        .expect("无法启动热键线程");
    handle
}

fn run(app: AppHandle, handle: Arc<HotkeyHandle>) {
    let tid = unsafe { GetCurrentThreadId() };
    handle.thread_id.store(tid, Ordering::SeqCst);
    log::write("hotkey", format!("线程启动 tid={tid}"));

    unsafe {
        // hwnd = NULL：注册到本线程。
        //
        // 候选顺序会被「上次实际生效的那个」顶到最前 —— 否则机器上装了别的软件时，
        // 可用组合一变，热键就会莫名其妙换掉，用户的肌肉记忆直接失效。
        let preferred = app
            .try_state::<AppState>()
            .and_then(|s| s.db.get_setting(crate::db::keys::HOTKEY));
        let mut order: Vec<&(&str, u32, u32)> = CANDIDATES.iter().collect();
        if let Some(p) = preferred.as_deref() {
            order.sort_by_key(|(label, _, _)| usize::from(*label != p));
        }
        if let Some(p) = preferred.as_deref() {
            log::write("hotkey", format!("上次生效的热键是 {p}，优先尝试它"));
        }

        let mut chosen: Option<&str> = None;
        for (label, mods, vk) in order {
            let ok = RegisterHotKey(
                std::ptr::null_mut(),
                HOTKEY_ID,
                mods | MOD_NOREPEAT,
                *vk,
            );
            if ok != 0 {
                chosen = Some(label);
                break;
            }
            log::write(
                "hotkey",
                format!("候选 {label} 被占用（err={}），试下一个", windows_sys::Win32::Foundation::GetLastError()),
            );
        }

        handle.active.store(chosen.is_some(), Ordering::SeqCst);
        *handle.display.lock() = chosen.unwrap_or(FALLBACK_HOTKEY).to_string();
        match chosen {
            Some(label) => {
                log::write("hotkey", format!("注册成功：{label}"));
                // 落库，下次启动优先复用
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = state.db.set_setting(crate::db::keys::HOTKEY, label);
                }
            }
            None => log::write("hotkey", "全部候选都被占用，热键不可用"),
        }

        // 注册结果要显示在界面上，所以无论成败都推一次事件
        events::emit_changed(&app);

        // 定时器与热键无关：界面点出来的置顶同样需要被守护，所以无条件启用
        SetTimer(std::ptr::null_mut(), TIMER_ID, REASSERT_MS, None);
        log::write("hotkey", "重断言定时器已启动（1s）");

        let mut msg: MSG = std::mem::zeroed();
        // GetMessageW: >0 有消息，0 收到 WM_QUIT，-1 出错 —— 后两者都该退出
        loop {
            let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
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
                _ => {}
            }
        }

        KillTimer(std::ptr::null_mut(), TIMER_ID);
        if chosen.is_some() {
            UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID);
        }
    }
}

/// 热键按下：切换「光标下窗口」的置顶状态
fn on_hotkey(app: &AppHandle) {
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
