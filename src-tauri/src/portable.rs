//! 便携版路径解析。
//!
//! 规则：优先把数据放在 **exe 同目录的 `data\`**（真便携，整个文件夹拷走即可）；
//! 若该位置不可写（只读 U 盘、Program Files、写保护盘），则回退到
//! `%LOCALAPPDATA%\窗口置顶助手\data\`，并把 `portable = false` 上报给界面显著提示。

use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PortablePaths {
    /// 实际生效的数据目录
    pub data_dir: PathBuf,
    /// true = 数据在 exe 同目录（真便携）；false = 已回退到系统目录
    pub portable: bool,
}

/// 构造一个可装箱的错误
pub fn err(msg: impl Into<String>) -> Box<dyn std::error::Error> {
    Box::new(io::Error::new(io::ErrorKind::NotFound, msg.into()))
}

/// 解析数据目录
pub fn resolve() -> Result<PortablePaths, Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let exe_dir = exe
        .parent()
        .ok_or_else(|| err("无法定位可执行文件所在目录"))?;

    let candidate = exe_dir.join("data");
    if is_writable(&candidate) {
        return Ok(PortablePaths {
            data_dir: candidate,
            portable: true,
        });
    }

    let base = dirs::data_local_dir().ok_or_else(|| err("无法定位 %LOCALAPPDATA%"))?;
    let fallback = base.join("窗口置顶助手").join("data");
    std::fs::create_dir_all(&fallback)?;
    if !is_writable(&fallback) {
        return Err(err(format!("数据目录不可写：{}", fallback.display())));
    }

    Ok(PortablePaths {
        data_dir: fallback,
        portable: false,
    })
}

/// 探测目录能否创建且可写（写一个探针文件再删掉）
fn is_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-probe");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}
