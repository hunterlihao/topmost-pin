/** Tauri 命令与事件的唯一出入口。组件不直接 import @tauri-apps/api。 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  PinChangedPayload,
  PinErrorPayload,
  Snapshot,
  WindowInfo,
} from "../types";

/* ── 命令 ─────────────────────────────────────────────── */

/** 枚举当前所有可见的顶层窗口 */
export const listWindows = () => invoke<WindowInfo[]>("list_windows");

/** 切换置顶状态，返回切换后的状态（true = 已置顶） */
export const togglePin = (hwnd: number) =>
  invoke<boolean>("toggle_pin", { hwnd });

/** 取消全部置顶，返回取消的数量 */
export const unpinAll = () => invoke<number>("unpin_all");

/** 取应用整体状态 */
export const getSnapshot = () => invoke<Snapshot>("get_snapshot");

/** 设置开机自启，返回生效后的状态 */
export const setAutostart = (enabled: boolean) =>
  invoke<boolean>("set_autostart", { enabled });

/**
 * 设置「是否以管理员运行」。
 *
 * 进程令牌无法在运行时更改，所以权限与当前不一致时这个命令会**重启应用**，
 * 调用后当前进程随即退出 —— 前端不必等待它的返回。
 */
export const setRunAsAdmin = (enabled: boolean) =>
  invoke<boolean>("set_run_as_admin", { enabled });

/** 启动动画播完后调用，关闭动画窗并放出主窗口 */
export const finishSplash = () => invoke<void>("finish_splash");

export const openDataDir = () => invoke<void>("open_data_dir");
export const hideToTray = () => invoke<void>("hide_to_tray");
export const quitApp = () => invoke<void>("quit_app");

/* ── 事件 ─────────────────────────────────────────────── */

export const onPinChanged = (fn: (p: PinChangedPayload) => void) =>
  listen<PinChangedPayload>("pin://changed", (e) => fn(e.payload));

export const onPinError = (fn: (p: PinErrorPayload) => void) =>
  listen<PinErrorPayload>("pin://error", (e) => fn(e.payload));

export type { UnlistenFn };
