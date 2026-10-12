/**
 * 用真实的构建产物渲染界面的各个状态，供截图自检。
 *
 * 为什么不用真程序截：真程序只能停在「刚启动、一个窗口都没置顶」这一种状态，
 * 而设计要检查的是**所有**状态 —— 台面上有 3 行、有 9 行（要滚动）、全部置顶、
 * 搜索无结果、无管理员权限、设置页、窄窗。靠鼠标脚本去驱动真程序既不稳也不快。
 *
 * 做法：在应用模块加载**之前**把 Tauri 的 IPC 换成桩，喂进固定的窗口数据。
 * 界面代码一行都不用改，也不需要给应用塞任何调试开关。
 *
 * 产物写到 dist/（已被 .gitignore 排除），本目录也整个被排除。
 */
import { readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const dist = join(root, "dist");

const js = readdirSync(join(dist, "assets")).find(
  (f) => f.startsWith("index-") && f.endsWith(".js"),
);
if (!js) throw new Error("dist/assets 里找不到入口 JS，先跑一次 vite build");

// ★ 样式在构建时被抽成独立文件，不会由 JS 注入 —— 少了这一行，
//   预览页会渲染成完全没有样式的 HTML（第一次跑就是这么被骗过去的）。
const css = readdirSync(join(dist, "assets")).find(
  (f) => f.startsWith("index-") && f.endsWith(".css"),
);
if (!css) throw new Error("dist/assets 里找不到入口 CSS");

/** 一个可见窗口。hwnd 用真实量级的句柄值（低位，< 2^31）。 */
const win = (hwnd, title, processName, className, pinned = false) => ({
  hwnd,
  title,
  processName,
  className,
  pinned,
});

/* 一份典型的开发者桌面：故意包含两组**标题完全相同**的窗口
   （两个「新建标签页」、两个「项目 — Visual Studio Code」），
   用来检查 HWND 那一列到底有没有存在的必要。 */
const WINDOWS = [
  win(0x00110a2c, "API 设计文档 - Chrome", "chrome.exe", "Chrome_WidgetWin_1"),
  win(0x00120b3d, "构建日志", "WindowsTerminal.exe", "CASCADIA_HOSTING_WINDOW_CLASS"),
  win(0x00130c4e, "任务管理器", "Taskmgr.exe", "TaskManagerWindow"),
  win(0x00140d5f, "微信", "WeChat.exe", "WeChatMainWndForPC"),
  win(0x00150e60, "计算器", "ApplicationFrameHost.exe", "ApplicationFrameWindow"),
  win(0x00160f71, "设置", "SystemSettings.exe", "ApplicationFrameWindow"),
  win(0x00171082, "项目 - Visual Studio Code", "Code.exe", "Chrome_WidgetWin_1"),
  win(0x00181193, "项目 - Visual Studio Code", "Code.exe", "Chrome_WidgetWin_1"),
  win(0x001912a4, "新建标签页", "chrome.exe", "Chrome_WidgetWin_1"),
  win(0x001a13b5, "新建标签页", "chrome.exe", "Chrome_WidgetWin_1"),
  win(0x001b14c6, "文件资源管理器", "explorer.exe", "CabinetWClass"),
  win(0x001c15d7, "下载", "explorer.exe", "CabinetWClass"),
  win(0x001d16e8, "Redis 监控", "WindowsTerminal.exe", "CASCADIA_HOSTING_WINDOW_CLASS"),
  win(0x001e17f9, "未命名 - 记事本", "Notepad.exe", "Notepad"),
  win(0x001f180a, "Docker Desktop", "Docker Desktop.exe", "Chrome_WidgetWin_1"),
  win(0x0020191b, "Postman", "Postman.exe", "Chrome_WidgetWin_1"),
  win(0x00211a2c, "数据同步服务", "java.exe", "SunAwtFrame"),
  win(0x00221b3d, "支付网关 - 本地", "java.exe", "SunAwtFrame"),
  win(0x00231c4e, "Figma", "Figma.exe", "Chrome_WidgetWin_1"),
  win(0x00241d5f, "网易云音乐", "cloudmusic.exe", "OrpheusBrowserHost"),
  win(0x00251e60, "Windows PowerShell", "powershell.exe", "ConsoleWindowClass"),
  win(0x00261f71, "NVIDIA GeForce Experience", "NVIDIA Overlay.exe", "CEF-OSC-WIDGET"),
  win(0x00272082, "QQ", "QQ.exe", "TXGuiFoundation"),
  win(0x00282193, "截图工具", "SnippingTool.exe", "Microsoft.UI.Content.DesktopChildSiteBridge"),
];

/** 预设状态：把前 n 个窗口标成已置顶 */
function pinnedFirst(n) {
  return WINDOWS.map((w, i) => ({ ...w, pinned: i < n }));
}

const STATES = {
  empty: { windows: pinnedFirst(0), admin: true },
  deck: { windows: pinnedFirst(3), admin: true },
  many: { windows: pinnedFirst(9), admin: true },
  allpinned: { windows: pinnedFirst(WINDOWS.length), admin: true },
  noadmin: { windows: pinnedFirst(2), admin: false },
  nomatch: { windows: pinnedFirst(2), admin: true, drive: "nomatch" },
  settings: { windows: pinnedFirst(3), admin: true, drive: "settings" },
  "settings-noadmin": { windows: pinnedFirst(2), admin: false, drive: "settings" },
};

const html = (state) => {
  const preset = STATES[state];
  if (!preset) throw new Error(`未知状态：${state}（可选：${Object.keys(STATES).join(", ")}）`);

  const snapshot = {
    pinned: preset.windows.filter((w) => w.pinned).map((w) => w.hwnd),
    hotkey: "Ctrl+Shift+P",
    hotkeyActive: true,
    autostart: true,
    isAdmin: preset.admin,
    portable: true,
    dataDir: "D:\\tools\\窗口置顶助手\\data",
    dbPath: "D:\\tools\\窗口置顶助手\\data\\data.db",
    pinEventCount: 128,
    version: "1.0.0",
  };

  // 热键不可用的那一档单独造一个，用来检查报头不再是空白键帽
  if (state === "nohotkey") snapshot.hotkeyActive = false;

  return `<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <title>preview:${state}</title>
    <link rel="stylesheet" href="./assets/${css}" />
  </head>
  <body>
    <div id="app"></div>
    <script>
      // ── Tauri IPC 桩 ────────────────────────────────────────────────
      // 必须在应用模块加载之前装好：应用一 mount 就会调 list_windows / get_snapshot。
      window.__TAURI_INTERNALS__ = {
        transformCallback: function () { return 1; },
        invoke: function (cmd) {
          if (cmd === "list_windows") return Promise.resolve(${JSON.stringify(preset.windows)});
          if (cmd === "get_snapshot") return Promise.resolve(${JSON.stringify(snapshot)});
          if (cmd === "plugin:event|listen") return Promise.resolve(1);
          if (cmd === "plugin:event|unlisten") return Promise.resolve();
          return Promise.resolve(null);
        },
      };
    </script>
    <script type="module" src="./assets/${js}"></script>
    <script>
      // 有些状态要「进页面之后点一下」才到得了（设置页、搜索无结果）。
      // 界面本身没有调试开关，所以这里直接操作真实 DOM。
      var drive = ${JSON.stringify(preset.drive || null)};
      if (drive === "settings") {
        setTimeout(function () {
          var btns = document.querySelectorAll(".seg button");
          for (var i = 0; i < btns.length; i++) {
            if (btns[i].textContent.trim() === "设置") btns[i].click();
          }
        }, 120);
      }
      if (drive === "nomatch") {
        setTimeout(function () {
          var box = document.querySelector('input[type="search"]');
          box.value = "zzzz-没有这个窗口";
          box.dispatchEvent(new Event("input", { bubbles: true }));
        }, 120);
      }
    </script>
  </body>
</html>
`;
};

const states = process.argv.slice(2);
const list = states.length ? states : Object.keys(STATES);
for (const s of list) {
  writeFileSync(join(dist, `preview-${s}.html`), html(s), "utf8");
}
console.log(`已生成 ${list.length} 个预览页：${list.join(", ")}`);
