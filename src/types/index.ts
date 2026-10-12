/** 与 Rust 侧 commands.rs 的数据结构一一对应。改动时两边必须同步。 */

/** 一个可见的顶层窗口 */
export interface WindowInfo {
  /** Win32 HWND 句柄。JS 的 number 可安全承载 64 位以下的整数句柄 */
  hwnd: number;
  /** 窗口标题 */
  title: string;
  /** Win32 窗口类名，如 Chrome_WidgetWin_1 */
  className: string;
  /** 所属进程的可执行文件名，如 chrome.exe；取不到时为「未知进程」 */
  processName: string;
  /** 是否由本程序置顶（注意：不读取 WS_EX_TOPMOST，见 docs/design.md 与 pin.rs） */
  pinned: boolean;
}

/** 应用整体状态快照 */
export interface Snapshot {
  /** 当前置顶的 HWND 列表 */
  pinned: number[];
  /** 热键的展示文案，如「Ctrl+Alt+P」 */
  hotkey: string;
  /** 热键是否注册成功。false = 被其他程序占用，界面需提示 */
  hotkeyActive: boolean;
  /** 是否已设置开机自启（以注册表实际读到的为准） */
  autostart: boolean;
  /**
   * 本进程当前是否已是管理员。
   *
   * 程序固定尝试以管理员运行，但提权可能失败（UAC 被拒、账户无管理员权限），
   * 那种情况下界面要说明「为什么有些窗口置顶不了」。
   */
  isAdmin: boolean;
  /** 数据是否落在 exe 同目录（true = 真便携） */
  portable: boolean;
  /** 实际生效的数据目录 */
  dataDir: string;
  /** SQLite 数据文件路径 */
  dbPath: string;
  /** 已记录的置顶事件条数 */
  pinEventCount: number;
  /** 版本号 */
  version: string;
}

/** pin://error 事件负载 */
export interface PinErrorPayload {
  message: string;
}

/** pin://changed 事件负载 */
export interface PinChangedPayload {
  pinned: number[];
}
