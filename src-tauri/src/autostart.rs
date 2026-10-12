//! 开机自启：直接写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`。
//!
//! 刻意不用 `tauri-plugin-autostart`：它的依赖链上有 `auto-launch -> async-broadcast`，
//! 而 async-broadcast 不在本机离线缓存中，`cargo metadata --offline` 会直接失败
//! （详见 Cargo.toml 的注释）。自己写注册表只有十几行，且零新增依赖。
//!
//! 只写 HKCU，不碰 HKLM —— 不需要管理员权限，也不构成「安装」，
//! 与便携版的定位一致：删掉文件夹、清掉这个值，系统里不留东西。

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "TopMostPin";

/// 当前是否已设置开机自启，**且那条记录指向的就是这个 exe**。
///
/// ★ 只判断「值存在」是不够的：便携版被挪到别的文件夹之后，注册表里那条仍然
/// 指着旧路径，界面却会显示「已开启」—— 而实际上登录时什么都不会启动。
/// 便携正是这个程序的定位，挪文件夹是常规操作，所以必须比对路径。
///
/// 顺带一提：这种情况不需要用户去手工清理注册表 —— 无论他接下来点「开启」
/// 还是「关闭」，都会把那条陈旧记录覆盖或删掉。
pub fn is_enabled() -> bool {
    let Ok(current) = std::env::current_exe() else {
        return false;
    };
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>(VALUE_NAME))
        .map(|stored| points_at(&stored, &current))
        .unwrap_or(false)
}

/// 比对注册表里存的命令行与当前 exe 路径。
///
/// 存进去的形式是 `"C:\...\x.exe"`（带引号，因为路径通常含空格），所以要先剥引号。
/// 不直接做字符串相等：大小写、短路径（8.3）、相对段都可能不同，
/// 能规范化就按规范化结果比，规范化失败（文件已被删/挪走）再退化成大小写无关比较。
fn points_at(stored: &str, current: &std::path::Path) -> bool {
    let raw = stored.trim().trim_matches('"').trim();
    if raw.is_empty() {
        return false;
    }
    match (std::path::Path::new(raw).canonicalize(), current.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => raw.eq_ignore_ascii_case(&current.to_string_lossy()),
    }
}

/// 设置 / 取消开机自启
pub fn set(enabled: bool) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY)?;

    if enabled {
        let exe = std::env::current_exe()?;
        // exe 路径通常含空格，必须带引号，否则系统会按空格切分命令行
        key.set_value(VALUE_NAME, &format!("\"{}\"", exe.display()))?;
    } else {
        match key.delete_value(VALUE_NAME) {
            Ok(()) => {}
            // 本来就没有，等价于已取消
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
