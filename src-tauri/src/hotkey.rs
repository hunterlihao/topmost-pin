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

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

use tauri::{AppHandle, Manager};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageW, KillTimer, PostThreadMessageW, SetTimer, MSG, WM_HOTKEY, WM_QUIT, WM_TIMER,
};

use crate::error::AppError;
use crate::events;
use crate::state::AppState;
use crate::winapi;

/// 展示给用户的热键文案。必须与下面 RegisterHotKey 的实参一致。
pub const HOTKEY_DISPLAY: &str = "Ctrl+Alt+P";

const HOTKEY_ID: i32 = 1;
const TIMER_ID: usize = 1;

/// 重断言间隔。1s 足够追上绝大多数「抢回置顶」的行为，又不会太频繁地惊动目标进程。
const REASSERT_MS: u32 = 1000;

/// 虚拟键码 'P'
const VK_P: u32 = 0x50;

pub struct HotkeyHandle {
    thread_id: AtomicU32,
    pub active: AtomicBool,
}

impl HotkeyHandle {
    fn new() -> Self {
        Self {
            thread_id: AtomicU32::new(0),
            active: AtomicBool::new(false),
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

/// 启动热键线程，立即返回句柄（注册结果稍后由 `active` 反映）
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
    unsafe {
        handle
            .thread_id
            .store(GetCurrentThreadId(), Ordering::SeqCst);

        // hwnd = NULL：注册到本线程
        let ok = RegisterHotKey(
            std::ptr::null_mut(),
            HOTKEY_ID,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            VK_P,
        );
        handle.active.store(ok != 0, Ordering::SeqCst);

        // 注册结果要在界面上显示，所以无论成败都推一次事件
        events::emit_changed(&app);

        // 定时器与热键无关：界面点出来的置顶同样需要被守护，所以无条件启用
        SetTimer(std::ptr::null_mut(), TIMER_ID, REASSERT_MS, None);

        let mut msg: MSG = std::mem::zeroed();
        // GetMessageW: >0 有消息，0 收到 WM_QUIT，-1 出错 —— 后两者都该退出
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            match msg.message {
                WM_HOTKEY => on_hotkey(&app),
                WM_TIMER => on_tick(&app),
                _ => {}
            }
        }

        KillTimer(std::ptr::null_mut(), TIMER_ID);
        if ok != 0 {
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
    let target = winapi::window_under_cursor()
        .filter(|h| winapi::is_candidate(*h))
        .or_else(|| (*state.last_foreground.lock()).filter(|h| winapi::is_candidate(*h)));

    let Some(hwnd) = target else {
        events::emit_error(app, AppError::NoTarget.to_string());
        return;
    };

    let title = winapi::title_of(hwnd);
    match state.pins.toggle(hwnd, &title) {
        Ok(_) => {
            events::emit_changed(app);
            crate::tray::refresh(app);
        }
        Err(e) => events::emit_error(app, e.to_string()),
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
        // 只有「用户可见的状态真的变了」才通知，避免每秒唤醒前端
        events::emit_changed(app);
        crate::tray::refresh(app);
    }
}
