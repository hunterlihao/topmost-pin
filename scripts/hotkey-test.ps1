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
$script:skip = 0

function Check($what, $ok, $detail = '') {
    if ($ok) {
        Write-Host ("  [PASS] {0}  {1}" -f $what, $detail)
    } else {
        Write-Host ("  [FAIL] {0}  {1}" -f $what, $detail) -ForegroundColor Red
        $script:fail++
    }
}

# 环境不满足某条断言的前提时用它 —— 不算产品缺陷，但必须显式说出来，
# 不能让「跳过」悄悄混进「全部通过」。
function Skip($what, $why) {
    Write-Host ("  [SKIP] {0}  {1}" -f $what, $why) -ForegroundColor Yellow
    $script:skip++
}

function Start-App([string[]]$extra) {
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
    Remove-Item $log -Force -ErrorAction SilentlyContinue
    # ★ 删完必须确认真的删掉了。应用写日志用的是追加模式、启动时不清空，
    #   而删除失败（句柄没释放、杀软占用）会被 SilentlyContinue 吞掉 ——
    #   那样后面所有子串断言都会命中**上一轮的日志**，整轮假通过。
    if (Test-Path $log) {
        throw "上一轮的日志没能删掉，本脚本的断言不可信：$log"
    }
    # 变量名不要用 $args —— 那是 PowerShell 的自动变量
    $argList = @('--no-elevate') + $extra
    return Start-Process -FilePath $exe -ArgumentList $argList -PassThru
}

# 读日志。文件不存在时返回空串而不是抛终止错误 ——
# 否则「应用没起来」会把逐项结论与 exit $script:fail 一起带走。
function Read-Log {
    if (-not (Test-Path $log)) { return '' }
    return (Get-Content $log -Raw)
}

function Wait-Log([string]$pattern, [int]$seconds = 12) {
    $until = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $until) {
        Start-Sleep -Milliseconds 200
        if ((Test-Path $log) -and ((Get-Content $log -Raw) -match $pattern)) { return $true }
    }
    return $false
}

$custom = 'Ctrl+Alt+Shift+F23'

try {
    Write-Host '=== 准备：全新安装（清掉数据库里的热键设置）==='
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    Get-ChildItem (Join-Path $dataDir 'data.db*') -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue

    # ── 1. 第一次启动：应落到候选列表里的某个组合 ──────────────────────
    Write-Host '=== 1) 首次启动，自动挑一个可用组合 ==='
    $null = Start-App @()
    $ok = Wait-Log '注册成功：'
    Check '首次启动注册了某个候选热键' $ok
    $firstLog = Read-Log
    if ($ok) {
        $auto = [regex]::Match($firstLog, '注册成功：(\S+)').Groups[1].Value
        Write-Host ("    实际：{0}" -f $auto)
    }
    # ★ 只有在**这一步**才能知道 Ctrl+Alt+P 是否被占用：此时数据库里没有
    #   用户设定的热键，应用会把整个候选列表试一遍、逐个记下谁被占用。
    #   到了第 3 步之后，用户设定的组合已经生效，候选列表根本不会被尝试 ——
    #   那时再去日志里找占用证据是永远找不到的（踩过）。
    $altPTaken = $firstLog -match 'Ctrl\+Alt\+P 被占用'
    Write-Host ("    Ctrl+Alt+P 在本机是否已被占用：{0}" -f $(if ($altPTaken) { '是' } else { '否' }))

    # ── 2. 通过命令行改成一个自定义组合 ────────────────────────────────
    Write-Host '=== 2) --set-hotkey 换成自定义组合（走的就是界面那条链路）==='
    $null = Start-App @("--set-hotkey=$custom")
    $ok = Wait-Log '--set-hotkey 已生效'
    Check '换键请求经由热键线程成功生效' $ok
    if ($ok) {
        $m = [regex]::Match((Read-Log), '--set-hotkey 已生效：(\S+)')
        Check '生效的就是请求的那个组合' ($m.Groups[1].Value -eq $custom) "实际：$($m.Groups[1].Value)"
    }

    # ── 3. 重启后应当记住它（落库 + 优先复用）─────────────────────────
    Write-Host '=== 3) 重启，应当记住自定义组合 ==='
    $null = Start-App @()
    $ok = Wait-Log ('注册成功：' + [regex]::Escape($custom))
    Check '重启后仍在用自定义的那一个' $ok
    $text = Read-Log
    Check '启动时优先尝试了它' ($text -match ('优先尝试用户设定的热键：' + [regex]::Escape($custom)))

    # ── 4. 换到一个被占用的组合：必须报错，且原热键还在 ────────────────
    Write-Host '=== 4) 换到一个已被占用的组合，应当失败且不丢原热键 ==='
    # ★ 前提证据来自第 1 步（见那里的说明），不是当前这段日志。
    #   不校验就断言的话：换一台没被占用的机器上这一步会**换键成功**，
    #   随后两条断言全部 FAIL —— 指向一个并不存在的产品缺陷。
    if (-not $altPTaken) {
        Skip '换到被占用组合时被拒绝' '本机 Ctrl+Alt+P 未被占用，构造不出该前提'
        Skip '失败后把原热键装了回来' '同上'
    } else {
        $null = Start-App @('--set-hotkey=Ctrl+Alt+P')
        $ok = Wait-Log '--set-hotkey 失败'
        Check '被占用的组合被拒绝' $ok
        if ($ok) {
            $m = [regex]::Match((Read-Log), '--set-hotkey 失败：(.+)')
            Write-Host ("    实际提示：" + $m.Groups[1].Value.Trim())
        }
        # 失败之后原热键必须仍然可用。用日志里的「已恢复原热键」作为证据 ——
        # 只看「曾经注册成功」是不够的，那行在失败之前就已经写下了。
        $after = Read-Log
        Check '失败后把原热键装了回来' ($after -match ('已恢复原热键 ' + [regex]::Escape($custom)))
    }
} finally {
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
}

Write-Host ''
if ($script:fail -eq 0) {
    Write-Host ("热键修改：全部通过。{0}" -f $(if ($script:skip) { "（跳过 $script:skip 项，见上）" } else { '' }))
} else {
    Write-Host ("热键修改：{0} 项失败，{1} 项跳过。" -f $script:fail, $script:skip) -ForegroundColor Red
}
exit $script:fail
