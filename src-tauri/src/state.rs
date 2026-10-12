//! 全局状态。由 Tauri 托管（`app.manage`），命令、热键线程、托盘共享同一份。

use std::sync::Arc;

use parking_lot::Mutex;

use crate::db::Db;
use crate::error::AppResult;
use crate::hotkey::HotkeyHandle;
use crate::pin::PinManager;
use crate::portable::PortablePaths;
use crate::winapi::{self, HwndRaw};

pub struct AppState {
    pub pins: PinManager,
    pub paths: PortablePaths,
    pub db: Db,
    /// 本进程启动时是否为管理员。进程令牌在运行中不会变，所以启动时取一次即可。
    pub is_admin: bool,
    /// 最近一次「不属于本程序」的前台窗口。托盘菜单用它作为操作对象 ——
    /// 菜单一打开，前台窗口就变成任务栏了，所以必须提前记住。
    pub last_foreground: Mutex<Option<HwndRaw>>,
    hotkey: Mutex<Option<Arc<HotkeyHandle>>>,
}

impl AppState {
    pub fn new(paths: PortablePaths, db: Db, is_admin: bool) -> Self {
        Self {
            pins: PinManager::new(),
            paths,
            db,
            is_admin,
            last_foreground: Mutex::new(None),
            hotkey: Mutex::new(None),
        }
    }

    /// WebView2 用户数据目录。放进 data\ 是为了便携版做到「零残留」。
    pub fn webview_cache_dir(&self) -> std::path::PathBuf {
        self.paths.data_dir.join("webview")
    }

    /* ── 置顶操作（唯一入口，顺带落库）─────────────────────
     *
     * 数据库记录刻意放在这里而不是 PinManager 里：PinManager 只关心「集合 + Win32」，
     * 让持久化从它内部穿过去会把两者绑死，也没法在不碰窗口的情况下单测状态机。
     */

    /// 切换置顶并记录事件，返回切换后的状态
    pub fn toggle_pin(&self, hwnd: HwndRaw, title: &str) -> AppResult<bool> {
        let pinned = self.pins.toggle(hwnd, title)?;
        self.record(hwnd, title, pinned);
        Ok(pinned)
    }

    /// 取消单个窗口的置顶并记录事件
    pub fn unpin_one(&self, hwnd: HwndRaw) -> AppResult<()> {
        let title = self
            .pins
            .list()
            .into_iter()
            .find(|(h, _)| *h == hwnd)
            .map(|(_, t)| t)
            .unwrap_or_default();
        self.pins.unpin(hwnd)?;
        self.record(hwnd, &title, false);
        Ok(())
    }

    /// 取消全部置顶，返回取消的数量
    pub fn unpin_all(&self) -> usize {
        let before = self.pins.list();
        let n = self.pins.unpin_all();
        for (hwnd, title) in before {
            self.record(hwnd, &title, false);
        }
        n
    }

    fn record(&self, hwnd: HwndRaw, title: &str, pinned: bool) {
        let process = winapi::process_name(winapi::pid_of(hwnd));
        self.db.record_pin_event(
            hwnd,
            title,
            &process,
            if pinned { "pin" } else { "unpin" },
        );
    }

    /* ── 热键 ─────────────────────────────────────────────── */

    pub fn set_hotkey(&self, handle: Arc<HotkeyHandle>) {
        *self.hotkey.lock() = Some(handle);
    }

    /// 热键是否注册成功。全部候选都被占用时为 false，界面据此给出提示。
    pub fn hotkey_active(&self) -> bool {
        self.hotkey
            .lock()
            .as_ref()
            .map(|h| h.active.load(std::sync::atomic::Ordering::SeqCst))
            .unwrap_or(false)
    }

    /// **实际生效的**热键文案。
    ///
    /// 不是写死的常量 —— 候选热键是逐个尝试的，界面必须显示真正注册成功的那个，
    /// 否则用户按提示去按键会发现没用（本机 Ctrl+Alt+P 就被占用了）。
    pub fn hotkey_display(&self) -> String {
        self.hotkey
            .lock()
            .as_ref()
            .map(|h| h.display())
            .unwrap_or_else(|| crate::hotkey::FALLBACK_HOTKEY.to_string())
    }

    pub fn stop_hotkey(&self) {
        if let Some(h) = self.hotkey.lock().as_ref() {
            h.stop();
        }
    }

    /* ── 设置 ─────────────────────────────────────────────── */
}
