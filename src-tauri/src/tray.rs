//! 系统托盘：图标、动态菜单、tooltip。
//!
//! 菜单必须在这里用 Rust 构建 —— `TrayIconConfig`（tauri.conf.json）没有 menu 字段，
//! 只支持图标与 tooltip。
//!
//! 菜单是**动态**的：每次置顶集合变化都用 `set_menu` 重建一次，
//! 让「已置顶窗口」直接成为可点击的取消入口。

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};

use crate::error::AppError;
use crate::events;
use crate::state::AppState;
use crate::winapi;

pub const TRAY_ID: &str = "main";

/// 动态菜单项 id 前缀：`unpin:<hwnd>`
const UNPIN_PREFIX: &str = "unpin:";

/// 菜单标题最大字符数。太长会把菜单撑得极宽。
const TITLE_MAX_CHARS: usize = 42;

const FALLBACK_TARGET: &str = "置顶 / 取消：最近的前台窗口";

/// 按字符（而非字节）截断 —— 中文标题按字节截会切出乱码
fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    let total = s.chars().count();
    if total <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

fn target_label(app: &AppHandle) -> String {
    let Some(state) = app.try_state::<AppState>() else {
        return FALLBACK_TARGET.to_string();
    };
    // 先取出值再 match：直接把 MutexGuard 放进 match  scrutinee 会让临时量
    // 活到整个 match 结束，从而借用比 state 更久。
    let fg: Option<isize> = *state.last_foreground.lock();
    match fg {
        Some(h) if winapi::is_alive(h) => {
            let t = winapi::title_of(h);
            if t.trim().is_empty() {
                FALLBACK_TARGET.to_string()
            } else {
                format!("置顶 / 取消：{}", truncate(&t, 30))
            }
        }
        _ => FALLBACK_TARGET.to_string(),
    }
}

fn tooltip_text(app: &AppHandle) -> String {
    let n = app.try_state::<AppState>().map(|s| s.pins.len()).unwrap_or(0);
    if n == 0 {
        "窗口置顶助手".to_string()
    } else {
        format!("窗口置顶助手 —— 已置顶 {n} 个窗口")
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let pinned = app
        .try_state::<AppState>()
        .map(|s| s.pins.list())
        .unwrap_or_default();

    let show = MenuItemBuilder::with_id("show", "显示主窗口").build(app)?;
    let pin_target = MenuItemBuilder::with_id("pin_target", target_label(app)).build(app)?;
    let unpin_all = MenuItemBuilder::with_id(
        "unpin_all",
        format!("全部取消置顶（{}）", pinned.len()),
    )
    .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    let mut builder = MenuBuilder::new(app)
        .item(&show)
        .separator()
        .item(&pin_target);

    if !pinned.is_empty() {
        builder = builder.separator();
        for (hwnd, title) in &pinned {
            let item = MenuItemBuilder::with_id(
                format!("{UNPIN_PREFIX}{hwnd}"),
                format!("✕ {}", truncate(title, TITLE_MAX_CHARS)),
            )
            .build(app)?;
            builder = builder.item(&item);
        }
        builder = builder.item(&unpin_all);
    }

    builder.separator().item(&quit).build()
}

pub fn setup(app: &App) -> tauri::Result<()> {
    let handle = app.handle();
    let menu = build_menu(handle)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(tooltip_text(handle))
        .show_menu_on_left_click(false);

    // 图标取自内嵌的窗口图标，无需运行时解码 PNG
    // （image-png / image-ico feature 依赖的 image crate 不在离线缓存中）
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn handle_menu(app: &AppHandle, id: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    match id {
        "show" => show_main(app),
        "quit" => app.exit(0),
        "unpin_all" => {
            crate::log::write("tray", "unpin_all（来自托盘菜单）");
            state.unpin_all();
            events::emit_changed(app);
            refresh(app);
        }
        "pin_target" => {
            let target = (*state.last_foreground.lock()).filter(|h| winapi::is_candidate(*h));
            crate::log::write(
                "tray",
                format!("pin_target（来自托盘菜单）目标={:?}", target.map(|h| format!("0x{h:X}"))),
            );
            match target {
                Some(hwnd) => {
                    let title = winapi::title_of(hwnd);
                    match state.toggle_pin(hwnd, &title) {
                        Ok(_) => {
                            events::emit_changed(app);
                            refresh(app);
                        }
                        Err(e) => events::emit_error(app, e.to_string()),
                    }
                }
                None => events::emit_error(app, AppError::NoTarget.to_string()),
            }
        }
        other => {
            if let Some(rest) = other.strip_prefix(UNPIN_PREFIX) {
                let Ok(hwnd) = rest.parse::<isize>() else {
                    return;
                };
                crate::log::write("tray", format!("取消单个置顶（来自托盘菜单）hwnd=0x{hwnd:X}"));
                if let Err(e) = state.unpin_one(hwnd) {
                    events::emit_error(app, e.to_string());
                }
                events::emit_changed(app);
                refresh(app);
            }
        }
    }
}

/// 重建托盘菜单与 tooltip。
///
/// ★ 会从热键线程调用，所以必须切回主线程 —— 托盘是 UI 资源，
/// 跨线程直接操作行为未定义。
pub fn refresh(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || refresh_now(&app));
}

fn refresh_now(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tooltip_text(app)));
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
