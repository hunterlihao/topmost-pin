//! 全局状态。由 Tauri 托管（`app.manage`），命令与热键线程共享同一份。

use std::sync::Arc;

use parking_lot::Mutex;

use crate::hotkey::HotkeyHandle;
use crate::pin::PinManager;
use crate::portable::PortablePaths;
use crate::winapi::HwndRaw;

pub struct AppState {
    pub pins: PinManager,
    pub paths: PortablePaths,
    /// 最近一次「不属于本程序」的前台窗口。托盘菜单用它作为操作对象 ——
    /// 菜单一打开，前台窗口就变成任务栏了，所以必须提前记住。
    pub last_foreground: Mutex<Option<HwndRaw>>,
    hotkey: Mutex<Option<Arc<HotkeyHandle>>>,
}

impl AppState {
    pub fn new(paths: PortablePaths) -> Self {
        Self {
            pins: PinManager::new(),
            paths,
            last_foreground: Mutex::new(None),
            hotkey: Mutex::new(None),
        }
    }

    /// WebView2 用户数据目录。放进 data\ 是为了便携版做到「零残留」。
    pub fn webview_cache_dir(&self) -> std::path::PathBuf {
        self.paths.data_dir.join("webview")
    }

    pub fn set_hotkey(&self, handle: Arc<HotkeyHandle>) {
        *self.hotkey.lock() = Some(handle);
    }

    /// 热键是否注册成功。被别的程序占用时为 false，界面据此给出提示。
    pub fn hotkey_active(&self) -> bool {
        self.hotkey
            .lock()
            .as_ref()
            .map(|h| h.active.load(std::sync::atomic::Ordering::SeqCst))
            .unwrap_or(false)
    }

    pub fn stop_hotkey(&self) {
        if let Some(h) = self.hotkey.lock().as_ref() {
            h.stop();
        }
    }
}
