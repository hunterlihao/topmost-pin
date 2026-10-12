//! Win32 层的集成测试 —— 对**真实窗口**验证置顶与取消置顶。
//!
//! 单元测试（pin.rs）用注入的假 Win32 验证状态机；这里验证真正落地的那一层：
//! `SetWindowPos` 是否真的改动了目标窗口的 `WS_EX_TOPMOST` 位。
//!
//! ★ 为什么自己建窗口，而不是启动一个 cmd.exe：
//! Windows 11 默认终端是 Windows Terminal，`cmd.exe` 会被托管成已有窗口的
//! 一个标签页，**不会**产生标题各异的独立顶层窗口（这个坑已经踩过一次：
//! 原版测试等 20 秒也等不到窗口）。自建窗口是确定性的，还能顺带验证
//! 「本进程自己的窗口必须被过滤掉」这条契约。
//!
//! 需要真实桌面环境，因此标了 #[ignore]：
//!     cargo test --features custom-protocol -- --ignored --nocapture

use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use topmost_pin_lib::winapi;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    PostMessageW, PostQuitMessage, RegisterClassExW, TranslateMessage, UnregisterClassW,
    MSG, WM_CLOSE, WM_DESTROY, WS_OVERLAPPEDWINDOW,
};

const CLASS_NAME: &str = "TopMostPinE2EClass\0";
const WINDOW_TITLE: &str = "TopMostPinE2E";

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

struct TestWindow {
    hwnd: isize,
    thread: Option<thread::JoinHandle<()>>,
}

impl TestWindow {
    /// 在独立线程里注册类、建窗口并跑消息循环
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<isize>();
        let class: Vec<u16> = CLASS_NAME.encode_utf16().collect();
        let title: Vec<u16> = WINDOW_TITLE.encode_utf16().chain(std::iter::once(0)).collect();

        let join = thread::spawn(move || unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());

            let mut wc: windows_sys::Win32::UI::WindowsAndMessaging::WNDCLASSEXW =
                std::mem::zeroed();
            wc.cbSize = std::mem::size_of_val(&wc) as u32;
            wc.lpfnWndProc = Some(wnd_proc);
            wc.hInstance = hinstance;
            wc.lpszClassName = class.as_ptr();
            RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                420,
                260,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null_mut(),
            );
            if hwnd.is_null() {
                let _ = tx.send(0);
                return;
            }
            let _ = tx.send(hwnd as isize);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            UnregisterClassW(class.as_ptr(), hinstance);
        });

        let hwnd = rx.recv().expect("窗口线程没有回报句柄");
        assert!(hwnd != 0, "CreateWindowExW 失败");
        Self {
            hwnd,
            thread: Some(join),
        }
    }

    fn close(&mut self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.close();
        }
    }
}

/// 轮询等待标题匹配的可见顶层窗口出现
fn wait_for_window(title: &str, timeout: Duration) -> Option<isize> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(w) = winapi::enum_top_level_windows()
            .into_iter()
            .find(|w| w.title.contains(title))
        {
            return Some(w.hwnd);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    None
}

/// 枚举结果必须满足过滤契约：非空标题、不含本进程自己的窗口
#[test]
fn enumeration_respects_filter_contract() {
    let me = std::process::id();
    for w in winapi::enum_top_level_windows() {
        assert!(
            !w.title.trim().is_empty(),
            "枚举结果里不应出现空标题窗口（hwnd={}）",
            w.hwnd
        );
        assert_ne!(w.pid, me, "枚举结果里不应出现本进程自己的窗口：{}", w.title);
        assert!(w.hwnd != 0, "hwnd 不应为 0");
    }
}

/// 能取到自身进程的可执行文件名
#[test]
fn can_resolve_own_process_name() {
    let name = winapi::process_name(std::process::id());
    assert!(
        name.to_lowercase().ends_with(".exe"),
        "应取到自身进程名，实际得到「{name}」"
    );
}

/// 本进程自己的窗口必须被 is_candidate 拒绝（否则界面会把自己的窗口列出来）
#[test]
#[ignore = "需要真实桌面环境，用 --ignored 显式运行"]
fn own_window_is_not_a_candidate() {
    let w = TestWindow::spawn();
    assert!(
        winapi::is_alive(w.hwnd),
        "自建窗口应当是存活的，句柄 0x{:X}",
        w.hwnd
    );
    assert!(
        winapi::is_own_window(w.hwnd),
        "自建窗口应当被识别为本进程所有"
    );
    assert!(
        !winapi::is_candidate(w.hwnd),
        "本进程自己的窗口不应成为置顶候选"
    );
    assert_eq!(winapi::title_of(w.hwnd), WINDOW_TITLE, "标题应能正确读出");
}

/// 真实窗口的置顶 / 取消置顶往返 —— 这是整个程序的核心承诺
#[test]
#[ignore = "需要真实桌面环境，用 --ignored 显式运行"]
fn pin_and_unpin_a_real_window() {
    let w = TestWindow::spawn();

    assert!(
        !winapi::has_topmost_style(w.hwnd),
        "新建窗口初始不应带 WS_EX_TOPMOST"
    );

    winapi::set_topmost(w.hwnd, true).expect("置顶调用失败");
    assert!(
        winapi::has_topmost_style(w.hwnd),
        "置顶后 WS_EX_TOPMOST 应当置位"
    );

    // 幂等：重复置顶不应报错，也不应把状态改坏
    winapi::set_topmost(w.hwnd, true).expect("重复置顶不应失败");
    assert!(winapi::has_topmost_style(w.hwnd));

    winapi::set_topmost(w.hwnd, false).expect("取消置顶调用失败");
    assert!(
        !winapi::has_topmost_style(w.hwnd),
        "取消置顶后 WS_EX_TOPMOST 应当清除"
    );

    // 再来一轮，确认状态可反复切换
    winapi::set_topmost(w.hwnd, true).expect("二次置顶失败");
    assert!(winapi::has_topmost_style(w.hwnd));
    winapi::set_topmost(w.hwnd, false).expect("二次取消失败");
    assert!(!winapi::has_topmost_style(w.hwnd));
}

/// PinManager 通过真实 Win32 走一遍完整生命周期
#[test]
#[ignore = "需要真实桌面环境，用 --ignored 显式运行"]
fn pin_manager_round_trip_on_real_window() {
    use topmost_pin_lib::pin::PinManager;

    let w = TestWindow::spawn();
    let m = PinManager::new();

    assert!(!m.is_pinned(w.hwnd));
    assert!(m.toggle(w.hwnd, WINDOW_TITLE).unwrap(), "首次切换应为置顶");
    assert!(m.is_pinned(w.hwnd));
    assert!(
        winapi::has_topmost_style(w.hwnd),
        "PinManager 置顶后窗口应真的带上 WS_EX_TOPMOST"
    );

    // 重断言在健康状态下不应产生变化
    assert_eq!(m.reassert(), 0);
    assert!(winapi::has_topmost_style(w.hwnd));

    assert!(!m.toggle(w.hwnd, WINDOW_TITLE).unwrap(), "二次切换应为取消");
    assert!(!m.is_pinned(w.hwnd));
    assert!(
        !winapi::has_topmost_style(w.hwnd),
        "取消后 WS_EX_TOPMOST 应清除"
    );
}

/// 窗口销毁后，重断言应当把它从集合里剔除（对应「关掉的窗口自动消失」）
#[test]
#[ignore = "需要真实桌面环境，用 --ignored 显式运行"]
fn reassert_drops_destroyed_window() {
    use topmost_pin_lib::pin::PinManager;

    let mut w = TestWindow::spawn();
    let m = PinManager::new();
    m.pin(w.hwnd, WINDOW_TITLE).unwrap();
    assert_eq!(m.len(), 1);

    w.close();

    assert_eq!(m.reassert(), 1, "已销毁的窗口应被剔除");
    assert_eq!(m.len(), 0);
}

/* ────────────────────────────────────────────────────────────────────────
 * 跨进程置顶
 *
 * 上面几个测试里，被置顶的窗口都属于**本测试进程** —— 而产品真正要做的是
 * 操作**别的进程**的窗口。这两件事在 Windows 上并不等价（完整性级别、输入队列、
 * DPI 感知都可能让跨进程的 SetWindowPos 被拒）。
 *
 * 所以这里必须真的开一个外部进程，去置顶它的窗口。
 * ──────────────────────────────────────────────────────────────────────── */

const CROSS_TITLE: &str = "TopMostPinCrossProcTarget";

/// 起一个外部进程，显示一个 WinForms 窗口并存活 30 秒
fn spawn_external_window() -> std::process::Child {
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         $f = New-Object System.Windows.Forms.Form; \
         $f.Text = '{CROSS_TITLE}'; \
         $f.Width = 420; $f.Height = 220; \
         $f.Show(); \
         $sw = [Diagnostics.Stopwatch]::StartNew(); \
         while ($sw.Elapsed.TotalSeconds -lt 30) {{ \
           [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 100 }}"
    );
    Command::new("pwsh")
        .args(["-NoProfile", "-Command", &script])
        .spawn()
        .expect("无法启动外部窗口进程")
}

#[test]
#[ignore = "需要真实桌面环境，用 --ignored 显式运行"]
fn can_pin_a_window_owned_by_another_process() {
    let mut child = spawn_external_window();

    let result = std::panic::catch_unwind(|| {
        let hwnd = wait_for_window(CROSS_TITLE, Duration::from_secs(25))
            .expect("25 秒内没有等到外部进程的窗口");

        assert!(
            !winapi::is_own_window(hwnd),
            "目标窗口应当属于外部进程"
        );
        assert!(winapi::is_candidate(hwnd), "外部窗口应当是候选");
        assert!(!winapi::has_topmost_style(hwnd), "初始不应置顶");

        winapi::set_topmost(hwnd, true).expect("跨进程置顶失败");
        assert!(
            winapi::has_topmost_style(hwnd),
            "跨进程置顶后 WS_EX_TOPMOST 应置位"
        );

        winapi::set_topmost(hwnd, false).expect("跨进程取消置顶失败");
        assert!(!winapi::has_topmost_style(hwnd), "跨进程取消后应清除");

        // ── 同一个窗口，改从**子线程**操作 ───────────────────────────────
        // 应用实际就是在自己的热键线程上调用 SetWindowPos 的，而不是主线程。
        // 这一步用来排除「在线程上调用」这个变量。
        let t = std::thread::spawn(move || {
            winapi::set_topmost(hwnd, true)
                .expect("子线程跨进程置顶失败");
            let on = winapi::has_topmost_style(hwnd);
            winapi::set_topmost(hwnd, false).expect("子线程取消置顶失败");
            on
        });
        assert!(
            t.join().expect("子线程 panic"),
            "子线程跨进程置顶后 WS_EX_TOPMOST 应置位"
        );
        assert!(!winapi::has_topmost_style(hwnd), "子线程取消后应清除");
    });

    let _ = child.kill();
    let _ = child.wait();

    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

/// 试着置顶一个**完全在沙箱之外**的窗口（用户自己启动的程序）。
///
/// 这个测试的意义是**区分「产品有缺陷」和「测试环境不允许」**：
/// 自动化环境（DSH）会把它的进程树放进一个 Job Object，并且进程可能运行在
/// 受限的窗口站上。若连这个测试都拿不到权限，那么「置顶用户程序失败」就是
/// 环境限制而不是代码问题。
#[test]
#[ignore = "需要真实桌面环境，用 --ignored 显式运行"]
fn probe_pinning_a_window_outside_the_sandbox() {
    // ★ 关键变量：先把自己声明成 PerMonitorV2。
    //
    // 实测对照：同一个目标窗口，DPI 感知的调用方（PowerShell 脚本，已声明
    // PerMonitorV2）置顶成功；DPI 不感知的调用方（本测试进程，控制台程序
    // 默认 Unaware）拿到 ERROR_ACCESS_DENIED。目标窗口属于 PerMonitorV2 进程。
    // 这一步把「DPI 感知上下文不匹配」这个变量单独测出来。
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    }
    // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
    let dpi_ok = unsafe { SetProcessDpiAwarenessContext(-4) } != 0;
    eprintln!("声明 PerMonitorV2 DPI 感知：{}", if dpi_ok { "成功" } else { "失败" });

    // 这些都是用户正常启动、不可能在自动化进程树里的程序
    const OUTSIDE: &[&str] = &[
        "QQ.exe",
        "WeChat.exe",
        "chrome.exe",
        "msedge.exe",
        "explorer.exe",
        "Typora.exe",
        "Code.exe",
        "idea64.exe",
    ];

    let mut tested = 0;
    let mut ok_count = 0;
    for w in winapi::enum_top_level_windows() {
        let name = winapi::process_name(w.pid);
        if !OUTSIDE.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            continue;
        }
        if tested >= 8 {
            break;
        }
        tested += 1;

        let before = winapi::has_topmost_style(w.hwnd);
        let r = winapi::set_topmost(w.hwnd, true);
        let after = winapi::has_topmost_style(w.hwnd);
        match &r {
            Ok(()) => {
                ok_count += 1;
                eprintln!("[OK  ] {name:<16} hwnd=0x{:X} topmost {before}->{after}", w.hwnd);
                let _ = winapi::set_topmost(w.hwnd, false);
            }
            Err(e) => {
                eprintln!("[FAIL] {name:<16} hwnd=0x{:X} before={before} after={after} -> {e}", w.hwnd);
            }
        }
    }
    if tested == 0 {
        eprintln!("跳过：桌面上没有找到沙箱外的目标程序");
    } else {
        eprintln!("沙箱外窗口 {ok_count}/{tested} 成功（本测试是环境探针，不 panic）");
    }
}
