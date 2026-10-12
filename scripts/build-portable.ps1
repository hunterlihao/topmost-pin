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

# 目标 exe 正在运行时覆盖会失败（文件被占用），而报错会是一句
# "being used by another process"，看不出该怎么办。先挡住并说清楚。
$running = @(Get-Process -Name '窗口置顶助手' -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    throw ("「窗口置顶助手」正在运行（pid={0}），无法替换 exe。请先从托盘退出，再重新构建。" -f (($running | ForEach-Object { $_.Id }) -join ','))
}

New-Item -ItemType Directory -Force -Path $outDir | Out-Null

# ★ 只替换本脚本产出的那两个文件，**不重建整个目录**。
#
# 早先的写法是把 data\ 先挪到 %TEMP% 再挪回来，然后 Remove-Item -Recurse 清空目录。
# 那有一段真实的丢数据窗口：脚本若在两步之间被打断，用户的运行数据就留在临时目录里，
# 而目录已经被清空了。既然只有两个文件需要更新，就别去碰别的。
Remove-Item (Join-Path $outDir "窗口置顶助手.exe") -Force -ErrorAction SilentlyContinue

# cargo 的产物名取自 Cargo.toml 的 package name，不是 tauri.conf.json 的 productName
$srcExe = Join-Path $root "src-tauri\target\release\topmost-pin.exe"
if (-not (Test-Path $srcExe)) { throw "找不到构建产物：$srcExe" }
$dstExe = Join-Path $outDir "窗口置顶助手.exe"
Copy-Item $srcExe $dstExe -Force

# 使用说明来自 packaging\ 下的正式文件，而不是写死在脚本里。
# 写死在脚本里已经出过一次事：程序改成「固定管理员运行」之后，
# 说明里的「本程序以普通权限运行」还留着，成了一份自相矛盾的交付文档。
$manualSrc = Join-Path $root "packaging\使用说明.txt"
if (-not (Test-Path $manualSrc)) { throw "找不到使用说明：$manualSrc" }
Copy-Item $manualSrc (Join-Path $outDir "使用说明.txt") -Force

Write-Host "==> 完成" -ForegroundColor Green
$exeInfo = Get-Item $dstExe
$total = (Get-ChildItem $outDir -Recurse -File | Measure-Object -Property Length -Sum).Sum
$hash = (Get-FileHash $dstExe -Algorithm SHA256).Hash

Write-Host ("  目录 : {0}" -f $outDir)
Write-Host ("  exe  : {0}  ({1:N2} MB)" -f $exeInfo.Name, ($exeInfo.Length / 1MB))
Write-Host ("  合计 : {0:N2} MB" -f ($total / 1MB))
Write-Host ("  SHA256: {0}" -f $hash)
