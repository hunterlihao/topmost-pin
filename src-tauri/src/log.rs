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

/// 单个日志文件的体积上限。超过就在启动时截掉前半段。
///
/// 热键每按一次要写好几行，长年累月下来日志只增不减。对一个以「便携、零残留」
/// 为定位的工具来说，不能让它在用户的 U 盘上默默长到几百 MB。
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// 指定日志落点。必须在任何 `write` 之前调用。
pub fn init(data_dir: &std::path::Path) {
    let path = data_dir.join("topmost-pin.log");
    trim_if_oversized(&path);
    let _ = LOG_PATH.set(path);
    let _ = START.set(Instant::now());
}

/// 超过上限就**保留末尾一半**，并在开头留一行说明。
///
/// 不直接清空：排查恰恰只需要最近那段现场，清空等于把线索一起扔掉。
fn trim_if_oversized(path: &std::path::Path) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= MAX_BYTES {
        return;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };

    let mut tail = &bytes[bytes.len() / 2..];
    // 从第一个换行之后开始，避免留下半行
    if let Some(pos) = tail.iter().position(|b| *b == b'\n') {
        tail = &tail[pos + 1..];
    }

    let mut out = Vec::with_capacity(tail.len() + 96);
    out.extend_from_slice(
        format!(
            "[  log  ] 日志超过 {} KB，已截断，仅保留最近部分\n",
            MAX_BYTES / 1024
        )
        .as_bytes(),
    );
    out.extend_from_slice(tail);
    // 本进程的写入用的是追加模式（FILE_APPEND_DATA），总是写在文件末尾，
    // 所以这次重写不会给别的句柄留下空洞。
    let _ = std::fs::write(path, out);
}

fn since_ms() -> u128 {
    START.get_or_init(Instant::now).elapsed().as_millis()
}

/// 写一行日志。`tag` 用来快速过滤，例如 `hotkey` / `pin` / `tray`。
pub fn write(tag: &str, msg: impl AsRef<str>) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };

    // ★ 先拼成完整一行，再一次性写出去。
    //
    // 用 `writeln!(f, "[{}] {} {}", ...)` 会走 `write_fmt` 的默认实现 ——
    // 它按格式片段分多次 WriteFile。追加模式下，这些片段可能与**另一个线程**
    // 或**另一个进程**（启动早期的第二实例也会写同一个文件）的片段交错，
    // 产出半行拼半行的日志，恰好破坏「出问题时唯一线索」的价值。
    let line = format!("[{:>8}ms] {:<7} {}\n", since_ms(), tag, msg.as_ref());

    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = f.write_all(line.as_bytes());
}

/// 日志文件路径（供界面「打开数据目录」旁展示）
pub fn path() -> Option<&'static PathBuf> {
    LOG_PATH.get()
}
