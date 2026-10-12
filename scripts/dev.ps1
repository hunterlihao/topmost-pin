# 开发模式：Vite 热更新 + Tauri 窗口
# 同样需要处理 cargo 不在 PATH、缓存重定向等本机约束。

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$env:CARGO_HOME = Join-Path $env:USERPROFILE ".cargo"
$env:RUSTUP_HOME = Join-Path $env:USERPROFILE ".rustup"
$cargo = Join-Path $env:CARGO_HOME "bin\cargo.exe"

$env:CARGO_NET_OFFLINE = "true"
$env:npm_config_cache = Join-Path $root ".npm-cache"
$env:PNPM_HOME = Join-Path $root ".pnpm-home"
$storeDir = Join-Path $root ".pnpm-store"

# Tauri CLI 需要能找到 cargo
$env:PATH = "$(Join-Path $env:CARGO_HOME 'bin');$env:PATH"

Write-Host "==> 启动开发模式（Ctrl+C 结束）" -ForegroundColor Cyan
pnpm tauri dev -- --offline
