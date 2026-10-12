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

use std::sync::mpsc;
use std::thread;

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
