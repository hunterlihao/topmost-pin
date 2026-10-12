//! 启动动画窗口。
//!
//! 做法：主窗口以 `visible(false)` 创建，同时弹出一个无边框小窗跑动画；
//! 动画页跑完后调用 `finish_splash` 命令关闭自己并把主窗口放出来。
//!
//! 为什么不由 Rust 定时关：动画时长归前端管（要尊重 `prefers-reduced-motion`），
//! Rust 只负责兜底 —— 如果前端脚本因为任何原因没跑起来，也必须把主窗口放出来，
//! 否则用户看到的是「程序启动了但什么都没有」。

use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "splash";
pub const MAIN_LABEL: &str = "main";
pub const WIDTH: f64 = 320.0;
pub const HEIGHT: f64 = 196.0;

/// 兜底超时。超过这个时间无论前端如何，都强制放出主窗口。
const FORCE_FINISH_MS: u64 = 5000;

/// 创建启动动画窗口。
///
/// ★ `data_directory` 必须和主窗口用**同一个**：WebView2 在一个进程里只能有
/// 一个用户数据目录，第二个窗口若用默认目录会直接失败（表现为「拒绝访问」）。
/// 这个坑很隐蔽 —— 报错信息完全看不出和缓存目录有关。
pub fn create(app: &AppHandle, webview_data_dir: &std::path::Path) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("splash.html".into()))
        .title("窗口置顶助手")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .visible(true)
        .data_directory(webview_data_dir.to_path_buf())
        .build()?;
    Ok(())
}

/// 前端跑完动画后调用：关掉动画窗、放出主窗口。
/// 会切回主线程 —— 窗口操作是 UI 资源。
pub fn finish(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || finish_now(&app));
}

fn finish_now(app: &AppHandle) {
    // 已经结束过了就别重复做（前端与兜底计时器可能都触发）
    if app.get_webview_window(LABEL).is_none() {
        return;
    }
    if let Some(s) = app.get_webview_window(LABEL) {
        let _ = s.close();
    }
    if let Some(m) = app.get_webview_window(MAIN_LABEL) {
        let _ = m.show();
        let _ = m.unminimize();
        let _ = m.set_focus();
    }
    crate::log::write("splash", "启动动画结束，主窗口已显示");
}

/// 兜底：即使前端脚本没跑起来，也必须把主窗口放出来
pub fn spawn_fallback(app: AppHandle) {
    std::thread::Builder::new()
        .name("topmost-pin-splash-timeout".to_string())
        .spawn(move || {
            std::thread::sleep(Duration::from_millis(FORCE_FINISH_MS));
            if app.get_webview_window(LABEL).is_some() {
                crate::log::write("splash", "启动动画超时未收尾，强制放出主窗口");
                finish(&app);
            }
        })
        .ok();
}
