# 全局热键「可修改」的端到端验证。
#
# 必须以子进程方式运行（& pwsh -NoProfile -File ...）：
# harness 自己直接派生 GUI 进程时，工具调用会一直等下去。
#
# 验的是**界面「修改」按钮走的那条同一条路径**
# （命令 -> AppState::rebind_hotkey -> HotkeyHandle::rebind
#   -> PostThreadMessageW -> 热键线程 -> swap -> 落库），
# 因为点不了界面，就用 --set-hotkey= 这个等价入口驱动它。
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root 'dist-portable\窗口置顶助手\窗口置顶助手.exe'
$dataDir = Join-Path $root 'dist-portable\窗口置顶助手\data'
$log = Join-Path $dataDir 'topmost-pin.log'
$name = '窗口置顶助手'

$script:fail = 0

function Check($what, $ok, $detail = '') {
    if ($ok) {
        Write-Host ("  [PASS] {0}  {1}" -f $what, $detail)
    } else {
        Write-Host ("  [FAIL] {0}  {1}" -f $what, $detail) -ForegroundColor Red
        $script:fail++
    }
}

function Start-App([string[]]$extra) {
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 500
    Remove-Item $log -Force -ErrorAction SilentlyContinue
    $args = @('--no-elevate') + $extra
    return Start-Process -FilePath $exe -ArgumentList $args -PassThru
}

function Wait-Log([string]$pattern, [int]$seconds = 12) {
    $until = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $until) {
        Start-Sleep -Milliseconds 200
        if ((Test-Path $log) -and ((Get-Content $log -Raw) -match $pattern)) { return $true }
    }
    return $false
}

try {
    Write-Host '=== 准备：全新安装（清掉数据库里的热键设置）==='
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 1
    Get-ChildItem (Join-Path $dataDir 'data.db*') -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue

    # ── 1. 第一次启动：应落到候选列表里的某个组合 ──────────────────────
    Write-Host '=== 1) 首次启动，自动挑一个可用组合 ==='
    $p = Start-App @()
    Start-Sleep -Seconds 8
    $text = Get-Content $log -Raw
    $auto = [regex]::Match($text, '注册成功：(\S+)').Groups[1].Value
    Check '首次启动注册了某个候选热键' ($auto -ne '') "实际：$auto"

    # ── 2. 通过命令行改成一个自定义组合 ────────────────────────────────
    Write-Host '=== 2) --set-hotkey 换成自定义组合（走的就是界面那条链路）==='
    $custom = 'Ctrl+Alt+Shift+F23'
    $p = Start-App @("--set-hotkey=$custom")
    $ok = Wait-Log '--set-hotkey 已生效'
    Check '换键请求经由热键线程成功生效' $ok
    if ($ok) {
        $text = Get-Content $log -Raw
        $m = [regex]::Match($text, '--set-hotkey 已生效：(\S+)')
        Check '生效的就是请求的那个组合' ($m.Groups[1].Value -eq $custom) "实际：$($m.Groups[1].Value)"
    }

    # ── 3. 重启后应当记住它（落库 + 优先复用）─────────────────────────
    Write-Host '=== 3) 重启，应当记住自定义组合 ==='
    $p = Start-App @()
    $ok = Wait-Log '注册成功：Ctrl\+Alt\+Shift\+F23'
    Check '重启后仍在用自定义的那一个' $ok
    $text = Get-Content $log -Raw
    Check '启动时优先尝试了它' ($text -match '优先尝试 Ctrl\+Alt\+Shift\+F23')

    # ── 4. 换到一个被占用的组合：必须报错，且原热键还在 ────────────────
    Write-Host '=== 4) 换到本机已被占用的 Ctrl+Alt+P，应当失败且不丢原热键 ==='
    # 先确认这个组合在本机确实被占（本机实测被别的程序注册了）
    $p = Start-App @('--set-hotkey=Ctrl+Alt+P')
    $ok = Wait-Log '--set-hotkey 失败'
    Check '被占用的组合被拒绝' $ok
    if ($ok) {
        $text = Get-Content $log -Raw
        $m = [regex]::Match($text, '--set-hotkey 失败：(.+)')
        Write-Host ("    实际提示：" + $m.Groups[1].Value.Trim())
    }
    # 失败之后原热键必须仍然可用。用日志里的「已恢复原热键」作为证据 ——
    # 只看「曾经注册成功」是不够的，那行在失败之前就已经写下了。
    $text = Get-Content $log -Raw
    Check '失败后把原热键装了回来' ($text -match '已恢复原热键 Ctrl\+Alt\+Shift\+F23')
} finally {
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 500
}

Write-Host ''
if ($script:fail -eq 0) { Write-Host '热键修改：全部通过。' }
else { Write-Host ("热键修改：{0} 项失败。" -f $script:fail) -ForegroundColor Red }
exit $script:fail
