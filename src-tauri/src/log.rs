//! 极简日志。
//!
//! 发布版是 GUI 子系统程序，没有控制台，`println!` 与 panic 信息都无处可去 ——
//! 出问题时排查只能是盲猜。所以把关键事件追加写进 `data\topmost-pin.log`。
//!
//! 设计上刻意保持最小：同步追加、不缓冲、不引入任何依赖。写入失败一律忽略 ——
//! 日志永远不该成为程序失败的原因。
//!
//! 只在「发生了值得记录的事」时写（启动、热键注册结果、每次按键、置顶状态变化、
//! 剔除失效窗口、错误），**不记录每秒的定时器心跳**，否则日志会被刷爆。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

/// 指定日志落点。必须在任何 `write` 之前调用。
pub fn init(data_dir: &std::path::Path) {
    let _ = LOG_PATH.set(data_dir.join("topmost-pin.log"));
    let _ = START.set(Instant::now());
}

fn since_ms() -> u128 {
    START.get_or_init(Instant::now).elapsed().as_millis()
}

/// 写一行日志。`tag` 用来快速过滤，例如 `hotkey` / `pin` / `tray`。
pub fn write(tag: &str, msg: impl AsRef<str>) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(f, "[{:>8}ms] {:<7} {}", since_ms(), tag, msg.as_ref());
}

/// 日志文件路径（供界面「打开数据目录」旁展示）
pub fn path() -> Option<&'static PathBuf> {
    LOG_PATH.get()
}
