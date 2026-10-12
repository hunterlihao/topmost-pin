//! SQLite 数据层。数据文件落在便携目录下的 `data.db`。
//!
//! 为什么用 SQLite 而不是 JSON：设置项之外还要记录置顶事件历史，而这类数据天然
//! 是「追加 + 按时间倒查」的形态，用 JSON 需要每次全量读写，写一半崩溃还会损坏。
//! SQLite 单文件、事务安全，且 `bundled` feature 会把 sqlite3 一起编译进来，
//! 不依赖系统库 —— 与「便携、零外部依赖」的定位一致。
//!
//! 并发：rusqlite 的 `Connection` 不是 `Sync`，这里用 `Mutex` 包一层。
//! 本程序的数据库访问都是毫秒级小操作，串行足够。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

/// 设置项键名。集中在这里，避免字符串散落各处拼错。
pub mod keys {
    /// **用户明确设定**的热键（在设置页改过，或 `--set-hotkey=` 指定的）。
    ///
    /// ★ 必须和「上次实际生效的」分开存。合成一个会有个很隐蔽的后果：
    /// 用户设的组合被别的程序**临时**占用一次，启动时就退到候选、顺手把这个键
    /// 覆盖成了候选 —— 用户的明确选择被一次临时冲突**永久**抹掉，
    /// 而那正是这个设计想避免的「肌肉记忆失效」。
    pub const HOTKEY: &str = "hotkey";

    /// 上次**实际生效**的热键（可能是自动挑的候选）。
    ///
    /// 只用来在用户没有明确设定时保持稳定：否则机器上可用组合一变，
    /// 每次启动都会换一个，用户记不住。
    pub const HOTKEY_LAST: &str = "hotkey_last";

    /// 上次尝试提权的时间戳（毫秒）。用于防抖，避免提权失败后无限循环
    pub const LAST_SWITCH_AT: &str = "last_switch_at";
}

pub const DB_FILE_NAME: &str = "data.db";

/// 当前时间戳（毫秒）
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn map_err(e: rusqlite::Error) -> AppError {
    AppError::msg(format!("数据库错误：{e}"))
}

pub struct Db {
    conn: Mutex<Connection>,
    path: std::path::PathBuf,
}

impl Db {
    /// 打开（不存在则创建）数据库文件
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path).map_err(map_err)?;

        // WAL 让读写不互相阻塞，也显著降低崩溃时损坏的概率。
        // 失败不致命（某些只读介质上会失败），继续用默认日志模式。
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");

        // ★ 必须设 busy timeout。
        //
        // WAL 允许多读单写，但写—写相撞时**没有 busy handler 就会立刻返回
        // SQLITE_BUSY**。这个库有三个执行体在访问（Tauri 主线程、热键线程、
        // 以及启动早期还没被单实例挡下的第二个进程）。撞上时
        // `set_setting(HOTKEY, …)` 会被静默丢弃 —— 表现就是
        // 「用户设好的热键下次启动莫名其妙变了」，而日志上什么都看不到。
        conn.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(map_err)?;

        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 建表 + 迁移。用 `PRAGMA user_version` 做版本号，后续加字段时追加分支即可。
    pub fn migrate(&self) -> AppResult<()> {
        let conn = self.conn.lock();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(map_err)?;

        if version < 1 {
            conn.execute_batch(
                r#"
                BEGIN;

                -- 键值设置表
                CREATE TABLE IF NOT EXISTS settings (
                    key        TEXT PRIMARY KEY,
                    value      TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                );

                -- 置顶事件历史。hwnd 只是当次运行的句柄，跨重启无意义，
                -- 因此这里保留 title / process_name 作为可读线索。
                CREATE TABLE IF NOT EXISTS pin_events (
                    id           INTEGER PRIMARY KEY AUTOINCREMENT,
                    hwnd         INTEGER NOT NULL,
                    title        TEXT    NOT NULL,
                    process_name TEXT    NOT NULL,
                    action       TEXT    NOT NULL CHECK (action IN ('pin','unpin')),
                    at           INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_pin_events_at ON pin_events(at DESC);

                PRAGMA user_version = 1;
                COMMIT;
                "#,
            )
            .map_err(map_err)?;
        }

        Ok(())
    }

    /* ── 设置 ─────────────────────────────────────────────── */

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> AppResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, now_ms()],
        )
        .map_err(map_err)?;
        Ok(())
    }

    pub fn get_i64(&self, key: &str, default: i64) -> i64 {
        self.get_setting(key)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(default)
    }

    pub fn set_i64(&self, key: &str, value: i64) -> AppResult<()> {
        self.set_setting(key, &value.to_string())
    }

    /* ── 置顶事件 ─────────────────────────────────────────── */

    /// 记录一次置顶/取消置顶。失败只写日志，不影响主流程 ——
    /// 历史记录是可观测性，不该让它把置顶本身搞失败。
    pub fn record_pin_event(&self, hwnd: isize, title: &str, process_name: &str, action: &str) {
        let conn = self.conn.lock();
        let r = conn.execute(
            "INSERT INTO pin_events (hwnd, title, process_name, action, at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![hwnd as i64, title, process_name, action, now_ms()],
        );
        if let Err(e) = r {
            crate::log::write("db", format!("写入置顶事件失败：{e}"));
        }
    }

    /// 置顶事件总数（给设置页展示，证明数据库确实在被使用）
    pub fn pin_event_count(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM pin_events", [], |r| r.get(0))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDb {
        db: Db,
        path: std::path::PathBuf,
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            // WAL 模式会额外生成这两个附属文件
            for suffix in ["-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.path.display()));
            }
        }
    }

    fn temp_db(tag: &str) -> TempDb {
        // 测试库放在 target/ 下，而不是 %TEMP%：
        // 本机文件沙箱允许写工作区，但拒绝写 %TEMP%，SQLite 会以
        // 「unable to open database file」失败 —— 那个报错完全看不出是权限问题。
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-db");
        std::fs::create_dir_all(&dir).expect("创建测试临时目录");

        let path = dir.join(format!("{tag}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Db::open(&path).expect("打开临时数据库");
        db.migrate().expect("建表");
        TempDb { db, path }
    }

    /// 迁移必须可重复执行 —— 每次启动都会调用它，不能第二次就出错或重建表。
    #[test]
    fn migrate_is_idempotent() {
        let t = temp_db("migrate");
        t.db.migrate().expect("第二次迁移");
        t.db.migrate().expect("第三次迁移");
    }

    /// 设置项读写往返，且「没设置过」与「设置成空」要能区分开。
    #[test]
    fn settings_round_trip() {
        let t = temp_db("settings");

        assert_eq!(t.db.get_setting(keys::HOTKEY), None, "从未设置过应为 None");

        t.db.set_setting(keys::HOTKEY, "Ctrl+Shift+P").expect("写入");
        assert_eq!(
            t.db.get_setting(keys::HOTKEY).as_deref(),
            Some("Ctrl+Shift+P")
        );

        // 覆盖写：同一个 key 应当更新而不是插入第二条
        t.db.set_setting(keys::HOTKEY, "Ctrl+Alt+K").expect("覆盖写");
        assert_eq!(
            t.db.get_setting(keys::HOTKEY).as_deref(),
            Some("Ctrl+Alt+K")
        );

        t.db.set_i64(keys::LAST_SWITCH_AT, 1234).expect("写整数");
        assert_eq!(t.db.get_i64(keys::LAST_SWITCH_AT, 0), 1234);
        assert_eq!(t.db.get_i64("不存在的键", 42), 42, "缺省时应取默认值");
    }

    /// 置顶事件只增不改，计数应当准确（设置页展示的就是它）
    #[test]
    fn pin_events_are_counted() {
        let t = temp_db("events");
        assert_eq!(t.db.pin_event_count(), 0);

        t.db.record_pin_event(0x1234, "记事本", "notepad.exe", "pin");
        t.db.record_pin_event(0x1234, "记事本", "notepad.exe", "unpin");
        t.db.record_pin_event(0x5678, "计算器", "calc.exe", "pin");

        assert_eq!(t.db.pin_event_count(), 3);
    }
}
