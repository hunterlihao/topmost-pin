//! 自实现的单实例控制 + 「唤醒已有实例」。
//!
//! ★ 为什么不用 `tauri-plugin-single-instance`：
//!
//! 「是否以管理员运行」需要在运行时重启进程（见 elevation.rs）。重启时旧实例还没
//! 退出，而插件的互斥体是在 `Builder` 阶段创建的 —— 新实例一启动就会被判定为
//! 重复启动，回调唤醒旧窗口后**自己退出**。用户看到的现象是：切换开关后程序一闪即没。
//!
//! 自己实现就能让「正在重启」的实例**等待旧实例释放互斥体**再继续。
//! 顺带还甩掉了插件依赖链上 Linux 专用的 zbus / async-broadcast。
//!
//! 唤醒机制用命名事件而不是窗口消息：不需要给 Tauri 的窗口过程挂钩子，
//! 已有实例起一个后台线程等事件即可。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    INFINITE,
};

use crate::winapi::to_wide;

/// `Local\` 前缀 = 当前登录会话内唯一。多用户同时登录时各自可以跑一份。
const MUTEX_NAME: &str = "Local\\TopMostPin.SingleInstance";
const EVENT_NAME: &str = "Local\\TopMostPin.Activate";

/// 等待旧实例释放互斥体的上限。UAC 弹窗期间旧实例早就退了，
/// 8 秒只是给「用户点了 UAC 但系统慢」留足余量。
const WAIT_PREVIOUS_TIMEOUT: Duration = Duration::from_secs(8);
const POLL_INTERVAL: Duration = Duration::from_millis(120);

/// 持有它即代表「本进程是唯一实例」。句柄在进程存活期间一直持有。
pub struct InstanceGuard {
    mutex: HANDLE,
}

// 句柄只是内核对象引用，跨线程传递是安全的
unsafe impl Send for InstanceGuard {}
unsafe impl Sync for InstanceGuard {}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.mutex.is_null() {
                CloseHandle(self.mutex);
            }
        }
    }
}

/// `acquire` 的结果。
///
/// ★ 必须把「已经有实例在跑」和「等旧实例退出等超时了」分开 —— 两者的处理方式
/// 完全相反。都当成「重复启动」去唤醒旧实例的话，超时那一支会变成
/// **两个进程都没了**（旧实例正在退出、没人接收唤醒），用户看到的是
/// 「双击之后程序凭空消失」，而且日志上看不出任何异常。
pub enum Acquire {
    /// 拿到了：本进程是唯一实例
    Owned(InstanceGuard),
    /// 已经有实例在运行
    Taken,
    /// 重启路径下等待旧实例释放互斥体，等超时了
    Timeout,
}

/// 尝试成为唯一实例。
///
/// `wait_for_previous = true` 时，若旧实例还在，会轮询等待它退出 —— 这是权限切换
/// 重启路径专用的；普通重复启动必须立刻返回 `Taken`，好让调用方去唤醒旧窗口。
pub fn acquire(wait_for_previous: bool) -> Acquire {
    let name = to_wide(MUTEX_NAME);
    let deadline = Instant::now() + WAIT_PREVIOUS_TIMEOUT;

    loop {
        unsafe {
            let mutex = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if mutex.is_null() {
                // 创建失败（极少见）。保守起见当作「拿不到」，避免真的跑出两个实例。
                return Acquire::Taken;
            }
            // ★ GetLastError 必须紧接着读，中间不能插入其它调用
            if GetLastError() != ERROR_ALREADY_EXISTS {
                return Acquire::Owned(InstanceGuard { mutex });
            }
            CloseHandle(mutex);
        }

        if !wait_for_previous {
            return Acquire::Taken;
        }
        if Instant::now() >= deadline {
            return Acquire::Timeout;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 唤醒已有实例（把主窗口拉到前台）。返回是否成功送达。
pub fn signal_existing() -> bool {
    let name = to_wide(EVENT_NAME);
    unsafe {
        let event = OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr());
        if event.is_null() {
            return false;
        }
        let ok = SetEvent(event) != 0;
        CloseHandle(event);
        ok
    }
}

static LISTENING: AtomicBool = AtomicBool::new(false);

/// 起一个后台线程等待「被唤醒」事件，收到就调用 `on_activate`。
///
/// 只允许启动一次。
pub fn listen(on_activate: impl Fn() + Send + 'static) {
    if LISTENING.swap(true, Ordering::SeqCst) {
        return;
    }
    let name = to_wide(EVENT_NAME);

    std::thread::Builder::new()
        .name("topmost-pin-activate".to_string())
        .spawn(move || unsafe {
            // 自动重置事件：每次 SetEvent 只唤醒一个等待者，正好对应一次启动请求
            let event = CreateEventW(std::ptr::null(), 0, 0, name.as_ptr());
            if event.is_null() {
                crate::log::write("instance", "无法创建唤醒事件，重复启动将无法唤起窗口");
                return;
            }
            loop {
                if WaitForSingleObject(event, INFINITE) == 0 {
                    on_activate();
                }
            }
        })
        .expect("无法启动单实例监听线程");
}
