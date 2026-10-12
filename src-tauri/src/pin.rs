//! 置顶集合管理。
//!
//! ★ 核心设计决策：**切换依据是本程序自己维护的集合，不是读 WS_EX_TOPMOST。**
//!
//! 理由：提示框、输入法候选窗、部分浮层天然就带 topmost 位。如果按 EXStyle 判断
//! 「是否已置顶」，光标指向这类窗口时第一次点击会变成「取消置顶」—— 用户看到的是
//! 什么都没发生，或者明明想置顶却被取消。用自己的集合就没有这个歧义：
//! 只有本程序置顶过的窗口才在集合里。
//!
//! Win32 调用通过 `Win32Ops` 注入，因此本模块的单元测试完全不碰真实窗口。

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::error::AppResult;
use crate::winapi::{self, HwndRaw};

/// PinManager 依赖的 Win32 能力。抽成 trait 只为可测性。
pub trait Win32Ops: Send + Sync {
    fn is_alive(&self, hwnd: HwndRaw) -> bool;
    fn has_topmost_style(&self, hwnd: HwndRaw) -> bool;
    fn set_topmost(&self, hwnd: HwndRaw, on: bool) -> AppResult<()>;
}

/// 真实实现
pub struct RealWin32;

impl Win32Ops for RealWin32 {
    fn is_alive(&self, hwnd: HwndRaw) -> bool {
        winapi::is_alive(hwnd)
    }
    fn has_topmost_style(&self, hwnd: HwndRaw) -> bool {
        winapi::has_topmost_style(hwnd)
    }
    fn set_topmost(&self, hwnd: HwndRaw, on: bool) -> AppResult<()> {
        winapi::set_topmost(hwnd, on)
    }
}

pub struct PinManager {
    ops: Box<dyn Win32Ops>,
    /// hwnd -> 标题快照（标题仅供托盘菜单展示）
    inner: Mutex<BTreeMap<HwndRaw, String>>,
}

impl PinManager {
    pub fn new() -> Self {
        Self::with_ops(Box::new(RealWin32))
    }

    pub fn with_ops(ops: Box<dyn Win32Ops>) -> Self {
        Self {
            ops,
            inner: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn is_pinned(&self, hwnd: HwndRaw) -> bool {
        self.inner.lock().contains_key(&hwnd)
    }

    pub fn ids(&self) -> Vec<HwndRaw> {
        self.inner.lock().keys().copied().collect()
    }

    pub fn list(&self) -> Vec<(HwndRaw, String)> {
        self.inner
            .lock()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// 切换，返回切换后的状态（true = 已置顶）
    pub fn toggle(&self, hwnd: HwndRaw, title: &str) -> AppResult<bool> {
        if self.is_pinned(hwnd) {
            self.unpin(hwnd)?;
            Ok(false)
        } else {
            self.pin(hwnd, title)?;
            Ok(true)
        }
    }

    /// 置顶。已在集合中时是幂等的空操作。
    pub fn pin(&self, hwnd: HwndRaw, title: &str) -> AppResult<()> {
        if self.is_pinned(hwnd) {
            return Ok(());
        }
        self.ops.set_topmost(hwnd, true)?;
        self.inner.lock().insert(hwnd, title.to_string());
        Ok(())
    }

    /// 取消置顶。窗口已经不在了不算失败 —— 结果和用户想要的一致。
    pub fn unpin(&self, hwnd: HwndRaw) -> AppResult<()> {
        let existed = self.inner.lock().remove(&hwnd).is_some();
        if existed && self.ops.is_alive(hwnd) {
            self.ops.set_topmost(hwnd, false)?;
        }
        Ok(())
    }

    /// 取消全部置顶，返回取消的数量
    pub fn unpin_all(&self) -> usize {
        let all = self.ids();
        let mut n = 0;
        for h in all {
            if self.unpin(h).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// 定时重断言：把被别的程序抢回去的窗口重新置顶，并剔除已销毁的窗口。
    ///
    /// 返回**被剔除的数量**。刻意不把「补了 topmost 位」算作变化 —— 那不是用户
    /// 可见的状态改变，若把它当变化上报，前端会每秒被事件唤醒一次。
    pub fn reassert(&self) -> usize {
        let entries = self.list();
        let mut dead = Vec::new();

        for (hwnd, _) in entries {
            if !self.ops.is_alive(hwnd) {
                dead.push(hwnd);
                continue;
            }
            if !self.ops.has_topmost_style(hwnd) {
                // 失败就下次再试，不在这里报错刷屏
                let _ = self.ops.set_topmost(hwnd, true);
            }
        }

        if dead.is_empty() {
            return 0;
        }
        let mut g = self.inner.lock();
        let mut removed = 0;
        for h in dead {
            if g.remove(&h).is_some() {
                removed += 1;
            }
        }
        removed
    }
}

impl Default for PinManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    /// 假的 Win32：记录调用，不碰真实窗口
    struct FakeWin32 {
        alive: Arc<AtomicBool>,
        topmost: Arc<AtomicBool>,
        /// 置为 true 时 set_topmost 一律返回 AccessDenied（模拟管理员窗口）
        deny: Arc<AtomicBool>,
        raise_calls: Arc<AtomicUsize>,
        lower_calls: Arc<AtomicUsize>,
    }

    impl FakeWin32 {
        fn new() -> (Self, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
            let alive = Arc::new(AtomicBool::new(true));
            let topmost = Arc::new(AtomicBool::new(false));
            let raise = Arc::new(AtomicUsize::new(0));
            let lower = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    alive: alive.clone(),
                    topmost: topmost.clone(),
                    deny: Arc::new(AtomicBool::new(false)),
                    raise_calls: raise.clone(),
                    lower_calls: lower.clone(),
                },
                alive,
                topmost,
                raise,
                lower,
            )
        }

        fn denier() -> Self {
            let (mut f, ..) = Self::new();
            f.deny = Arc::new(AtomicBool::new(true));
            f
        }
    }

    impl Win32Ops for FakeWin32 {
        fn is_alive(&self, _hwnd: HwndRaw) -> bool {
            self.alive.load(Ordering::SeqCst)
        }
        fn has_topmost_style(&self, _hwnd: HwndRaw) -> bool {
            self.topmost.load(Ordering::SeqCst)
        }
        fn set_topmost(&self, _hwnd: HwndRaw, on: bool) -> AppResult<()> {
            if self.deny.load(Ordering::SeqCst) {
                return Err(AppError::AccessDenied);
            }
            if on {
                self.raise_calls.fetch_add(1, Ordering::SeqCst);
            } else {
                self.lower_calls.fetch_add(1, Ordering::SeqCst);
            }
            self.topmost.store(on, Ordering::SeqCst);
            Ok(())
        }
    }

    fn manager() -> (PinManager, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let (fake, alive, topmost, raise, lower) = FakeWin32::new();
        (
            PinManager::with_ops(Box::new(fake)),
            alive,
            topmost,
            raise,
            lower,
        )
    }

    #[test]
    fn toggle_round_trip() {
        let (m, _alive, topmost, raise, lower) = manager();
        assert!(!m.is_pinned(0x1000));
        assert!(m.toggle(0x1000, "记事本").unwrap());
        assert!(m.is_pinned(0x1000));
        assert!(topmost.load(Ordering::SeqCst));
        assert!(!m.toggle(0x1000, "记事本").unwrap());
        assert!(!m.is_pinned(0x1000));
        assert!(!topmost.load(Ordering::SeqCst));
        assert_eq!(raise.load(Ordering::SeqCst), 1);
        assert_eq!(lower.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pin_is_idempotent() {
        let (m, _alive, _topmost, raise, _lower) = manager();
        m.pin(0x2000, "a").unwrap();
        m.pin(0x2000, "a").unwrap();
        m.pin(0x2000, "a").unwrap();
        assert_eq!(raise.load(Ordering::SeqCst), 1, "重复置顶只应真正调用一次");
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn unpin_unknown_hwnd_is_ok() {
        let (m, _alive, _topmost, _raise, lower) = manager();
        assert!(m.unpin(0xDEAD).is_ok());
        assert_eq!(lower.load(Ordering::SeqCst), 0, "没置顶过的窗口不该被降级");
    }

    #[test]
    fn access_denied_does_not_enter_set() {
        let (fake, ..) = FakeWin32::new();
        let m = PinManager::with_ops(Box::new(fake));
        // 先成功一次，证明通路正常
        assert!(m.pin(0x3000, "x").is_ok());
        assert!(m.is_pinned(0x3000));

        let m2 = PinManager::with_ops(Box::new(FakeWin32::denier()));
        let err = m2.pin(0x4000, "taskmgr").unwrap_err();
        assert!(matches!(err, AppError::AccessDenied));
        assert!(!m2.is_pinned(0x4000), "失败的目标不应留在集合里");
    }

    #[test]
    fn reassert_drops_dead_windows() {
        let (m, alive, _topmost, _raise, _lower) = manager();
        m.pin(0x5000, "a").unwrap();
        m.pin(0x6000, "b").unwrap();
        assert_eq!(m.len(), 2);

        alive.store(false, Ordering::SeqCst);
        assert_eq!(m.reassert(), 2);
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn reassert_restores_stolen_topmost() {
        let (m, _alive, topmost, raise, _lower) = manager();
        m.pin(0x7000, "a").unwrap();
        assert_eq!(raise.load(Ordering::SeqCst), 1);

        // 装作被别的程序抢走了 topmost 位
        topmost.store(false, Ordering::SeqCst);
        assert_eq!(m.reassert(), 0, "补位不算用户可见变化");
        assert!(topmost.load(Ordering::SeqCst), "应被重新置顶");
        assert_eq!(raise.load(Ordering::SeqCst), 2);
        assert_eq!(m.len(), 1, "窗口还活着，不该被剔除");
    }

    #[test]
    fn reassert_is_noop_when_healthy() {
        let (m, _alive, _topmost, raise, _lower) = manager();
        m.pin(0x8000, "a").unwrap();
        assert_eq!(m.reassert(), 0);
        assert_eq!(m.reassert(), 0);
        assert_eq!(raise.load(Ordering::SeqCst), 1, "健康状态下不应重复调用");
    }

    #[test]
    fn unpin_all_counts() {
        let (m, ..) = manager();
        m.pin(0x1, "a").unwrap();
        m.pin(0x2, "b").unwrap();
        m.pin(0x3, "c").unwrap();
        assert_eq!(m.unpin_all(), 3);
        assert_eq!(m.len(), 0);
        assert_eq!(m.unpin_all(), 0);
    }

    #[test]
    fn concurrent_toggle_stays_consistent() {
        use std::thread;

        let (fake, ..) = FakeWin32::new();
        let m = Arc::new(PinManager::with_ops(Box::new(fake)));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let m = m.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..200 {
                    let _ = m.toggle(0x9000, "contended");
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // 集合本身始终自洽：要么在要么不在
        let _ = m.is_pinned(0x9000);
        assert!(m.len() <= 1);
    }
}
