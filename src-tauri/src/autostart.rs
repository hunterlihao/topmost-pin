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

/// 当前是否已设置开机自启
pub fn is_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>(VALUE_NAME))
        .is_ok()
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
