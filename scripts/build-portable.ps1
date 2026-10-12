# 一键产出便携版：dist-portable\窗口置顶助手\
#
# 本机环境的三个硬约束，脚本已全部处理：
#   1. cargo 无法联网（schannel 凭证问题）-> 全程 CARGO_NET_OFFLINE=true，只用本地缓存
#   2. Rust 不在 PATH                -> 显式使用 %USERPROFILE%\.cargo\bin\cargo.exe
#   3. npm/pnpm 默认缓存在项目外      -> 全部重定向到项目内，避免写入被拒
#
# 另：Cargo.toml 里 windows-sys 必须写成内联表。若写成 [dependencies.windows-sys]
# 子表，TOML 会把后续所有依赖项吞进该子表，serde/dirs 会静默失效。

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

Write-Host "==> 准备环境变量" -ForegroundColor Cyan
$env:CARGO_HOME = Join-Path $env:USERPROFILE ".cargo"
$env:RUSTUP_HOME = Join-Path $env:USERPROFILE ".rustup"
$cargo = Join-Path $env:CARGO_HOME "bin\cargo.exe"
if (-not (Test-Path $cargo)) { throw "找不到 cargo：$cargo" }

$env:CARGO_NET_OFFLINE = "true"
$env:CARGO_INCREMENTAL = "0"
$env:npm_config_cache = Join-Path $root ".npm-cache"
$env:PNPM_HOME = Join-Path $root ".pnpm-home"
$storeDir = Join-Path $root ".pnpm-store"

Write-Host "==> 安装前端依赖" -ForegroundColor Cyan
pnpm install --store-dir $storeDir
if ($LASTEXITCODE -ne 0) { throw "pnpm install 失败" }

Write-Host "==> 前端类型检查（vue-tsc 严格模式）" -ForegroundColor Cyan
pnpm typecheck
if ($LASTEXITCODE -ne 0) { throw "类型检查失败" }

Write-Host "==> 构建前端（Vite）" -ForegroundColor Cyan
pnpm build
if ($LASTEXITCODE -ne 0) { throw "前端构建失败" }

Write-Host "==> 单元测试" -ForegroundColor Cyan
& $cargo test --offline --features custom-protocol --manifest-path (Join-Path $root "src-tauri\Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "单元测试失败" }

# ★ 必须带 --features custom-protocol。
# tauri/build.rs 里 `let dev = !custom_protocol;`，缺了它 Tauri 会判定为开发模式，
# 窗口会去加载 devUrl（http://localhost:1420）而不是内嵌资源，界面直接白屏报
# ERR_CONNECTION_REFUSED。`tauri build` CLI 会自动加上，直接调 cargo 时必须手写。
Write-Host "==> 构建 Rust（release, 离线, custom-protocol）" -ForegroundColor Cyan
& $cargo build --release --offline --features custom-protocol --manifest-path (Join-Path $root "src-tauri\Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Rust 构建失败" }

Write-Host "==> 组装便携目录" -ForegroundColor Cyan
$outDir = Join-Path $root "dist-portable\窗口置顶助手"

# ★ 绝不删除已有的 data\ —— 那是用户的实际运行数据（WebView2 缓存、配置）。
# 只替换 exe 与说明文件，数据目录原样保留。
$dataDir = Join-Path $outDir "data"
$preserved = $null
if (Test-Path $dataDir) {
    $preserved = Join-Path $env:TEMP ("tmp-data-backup-" + [guid]::NewGuid().ToString("N"))
    Write-Host "    检测到已有数据，先临时移出以保护：$dataDir" -ForegroundColor Yellow
    Move-Item $dataDir $preserved -Force
}

if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

if ($preserved) {
    Move-Item $preserved $dataDir -Force
    Write-Host "    已还原原有数据目录（未改动）" -ForegroundColor Green
}

# cargo 的产物名取自 Cargo.toml 的 package name，不是 tauri.conf.json 的 productName
$srcExe = Join-Path $root "src-tauri\target\release\topmost-pin.exe"
if (-not (Test-Path $srcExe)) { throw "找不到构建产物：$srcExe" }
$dstExe = Join-Path $outDir "窗口置顶助手.exe"
Copy-Item $srcExe $dstExe -Force

# 使用说明
$readme = @"
窗口置顶助手 —— 便携版
=====================================

【怎么用】
双击「窗口置顶助手.exe」即可运行，无需安装。

两种置顶方式，随便用哪种：
  1. 把鼠标指向目标窗口，按 Ctrl+Alt+P。再按一次取消。
  2. 在界面列表里找到窗口，点它左边的图钉。

已置顶的窗口会整行反相显示，排在列表最上方。

【关掉窗口去哪了】
关掉主窗口只是收进系统托盘，程序仍在后台守着已置顶的窗口。
双击托盘图标可以重新打开界面。
要真正退出：托盘右键 -> 退出，或界面「设置」页 -> 退出。

【数据在哪】
全部数据都在本目录下的 data\ 文件夹里：
    data\webview\    WebView2 运行时缓存

换电脑 / 换 U 盘：把整个「窗口置顶助手」文件夹拷走即可。
程序不写系统目录，删除文件夹即彻底卸载。
（唯一例外：如果勾选了「开机自启」，会在当前用户的注册表 Run 项里留一条记录，
  在那个开关上取消勾选即会清除。）

【系统要求】
Windows 10 / 11 64 位。
需要 WebView2 运行时（Win10 1803+ 与 Win11 已预装；若缺失请安装
Microsoft Edge WebView2 Runtime）。

【已知限制】
· 以管理员身份运行的程序（如任务管理器）无法被置顶 —— 这是 Windows 的
  完整性级别限制（UIPI）。本程序以普通权限运行，遇到这种情况会在界面上
  明确提示，不会静默失败。
· Ctrl+Alt+P 若被其他软件占用，界面会提示「热键未生效」，
  此时仍可用界面列表正常置顶。
· 退出程序会自动取消全部置顶。若程序异常结束留下残留，
  重新运行后点一次「全部取消」即可清除。
"@
Set-Content -Path (Join-Path $outDir "使用说明.txt") -Value $readme -Encoding UTF8

Write-Host "==> 完成" -ForegroundColor Green
$exeInfo = Get-Item $dstExe
$total = (Get-ChildItem $outDir -Recurse -File | Measure-Object -Property Length -Sum).Sum
$hash = (Get-FileHash $dstExe -Algorithm SHA256).Hash

Write-Host ("  目录 : {0}" -f $outDir)
Write-Host ("  exe  : {0}  ({1:N2} MB)" -f $exeInfo.Name, ($exeInfo.Length / 1MB))
Write-Host ("  合计 : {0:N2} MB" -f ($total / 1MB))
Write-Host ("  SHA256: {0}" -f $hash)
