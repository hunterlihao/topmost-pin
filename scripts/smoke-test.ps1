<#
.SYNOPSIS
    便携版启动冒烟测试：验证构建产物真的能跑起来。

.DESCRIPTION
    覆盖这些只有启动后才知道的结论：
      - exe 的 PE 子系统是 GUI（不是 Console）—— 即运行时不会弹出黑窗
      - 进程能稳定存活，主窗口能被创建
      - 便携数据目录落在 exe 同目录，WebView2 缓存已初始化
      - 单实例：第二次启动不产生第二个进程
      - 界面真的渲染了（截图像素采样，而不是只看进程活着）

    实现注意：
      · 枚举窗口一律走编译好的 C# 辅助方法。不要用 PowerShell 脚本块当
        委托回调 —— 在本机实测会卡死。
      · 应用是常驻托盘程序，不会自己退出，因此必须在 finally 里无条件收掉，
        否则调用方会一直等下去。

.PARAMETER ExePath
    便携版 exe 路径。默认取 dist-portable\窗口置顶助手\窗口置顶助手.exe

.PARAMETER OutDir
    截图与报告的落点。默认 artifacts\
#>
[CmdletBinding()]
param(
    [string]$ExePath,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if (-not $ExePath) {
    $ExePath = Join-Path $root 'dist-portable\窗口置顶助手\窗口置顶助手.exe'
}
if (-not $OutDir) { $OutDir = Join-Path $root 'artifacts' }
if (-not (Test-Path $ExePath)) { throw "找不到 exe：$ExePath" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$AppDir = Split-Path $ExePath -Parent
$ProcessName = [System.IO.Path]::GetFileNameWithoutExtension($ExePath)
$WindowTitle = '窗口置顶助手'

$failures = [System.Collections.Generic.List[string]]::new()
function Check($name, $ok, $detail) {
    if ($ok) {
        Write-Host ("  [PASS] {0}{1}" -f $name, $(if ($detail) { "  $detail" } else { '' })) -ForegroundColor Green
    } else {
        Write-Host ("  [FAIL] {0}{1}" -f $name, $(if ($detail) { "  $detail" } else { '' })) -ForegroundColor Red
        $failures.Add($name)
    }
}

# ── PE 子系统 ────────────────────────────────────────────────────────────
# Windows 的 GUI 子系统 = 2，Console = 3。gui 构建不会附加控制台窗口。
function Get-PeSubsystem([string]$Path) {
    $fs = [System.IO.File]::OpenRead($Path)
    try {
        $br = [System.IO.BinaryReader]::new($fs)
        $fs.Position = 0x3C
        $peOffset = $br.ReadInt32()
        $fs.Position = $peOffset + 0x5C
        return $br.ReadInt16()
    } finally { $fs.Dispose() }
}

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class WinCap {
    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    // PW_RENDERFULLCONTENT = 0x2，WebView2 这类合成窗口必须带这个标志
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);

    [DllImport("user32.dll")] private static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] private static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hWnd);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    /// 让本进程 DPI 感知。
    ///
    /// ★ 必须做，且必须在取任何窗口矩形之前做。
    /// 本机显示器是 2560x1600 @150%，DPI 不感知的进程拿到的 GetWindowRect 是
    /// 被系统虚拟化过的坐标（1410 物理像素会被报成 940），据此建立的位图只有
    /// 窗口的左侧 2/3 —— 截出来的图会被误判成「界面渲染错位」。
    public static string MakeDpiAware() {
        // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
        if (SetProcessDpiAwarenessContext(new IntPtr(-4))) return "PerMonitorV2";
        if (SetProcessDPIAware()) return "System";
        return "Failed";
    }

    /// 找一个小尺寸的可见窗口（启动动画窗），按类名与进程双重限定，
    /// 免得误抓到 IME / 消息窗口。
    public static IntPtr FindSmallWindow(int pid, string className, int maxW, int maxH) {
        IntPtr result = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p != (uint)pid || !IsWindowVisible(h)) return true;
            var c = new StringBuilder(256); GetClassNameW(h, c, 256);
            if (c.ToString() != className) return true;
            RECT r; GetWindowRect(h, out r);
            int w = r.Right - r.Left, hh = r.Bottom - r.Top;
            if (w > 120 && w <= maxW && hh > 120 && hh <= maxH) { result = h; return false; }
            return true;
        }, IntPtr.Zero);
        return result;
    }

    /// 按标题找**面积最大**的可见窗口。
    ///
    /// 不能只取第一个匹配：启动动画窗与主窗口标题相同，过渡瞬间两个都可能可见，
    /// 取到动画窗的话会得到一个小矩形甚至 0x0，截图判据就失去意义了。
    public static IntPtr FindLargestByTitle(string needle) {
        IntPtr best = IntPtr.Zero;
        long bestArea = 0;
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            var t = new StringBuilder(512); GetWindowTextW(h, t, 512);
            if (!t.ToString().Contains(needle)) return true;
            RECT r; GetWindowRect(h, out r);
            long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
            if (area > bestArea) { bestArea = area; best = h; }
            return true;
        }, IntPtr.Zero);
        return best;
    }

    /// 该进程是否创建了控制台窗口。release 构建声明了 windows_subsystem="windows"，
    /// 不应该有；debug 构建会保留控制台用于看 panic 与日志。
    public static bool HasConsoleWindow(int pid) {
        bool found = false;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p != (uint)pid) return true;
            var c = new StringBuilder(256); GetClassNameW(h, c, 256);
            if (c.ToString() == "ConsoleWindowClass") { found = true; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    private static List<string> _scratch;

    /// 列出某个进程的所有顶层窗口，返回可读描述
    public static string[] WindowsOfProcess(int pid) {
        _scratch = new List<string>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == (uint)pid) {
                var t = new StringBuilder(512); GetWindowTextW(h, t, 512);
                var c = new StringBuilder(256); GetClassNameW(h, c, 256);
                _scratch.Add(string.Format("hwnd=0x{0:X} visible={1} class={2} title=[{3}]",
                    h.ToInt64(), IsWindowVisible(h), c.ToString(), t.ToString()));
            }
            return true;
        }, IntPtr.Zero);
        return _scratch.ToArray();
    }

    /// 按标题子串查找窗口；返回第一个匹配的可见窗口
    public static IntPtr FindByTitle(string needle) {
        IntPtr result = IntPtr.Zero;
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            var t = new StringBuilder(512); GetWindowTextW(h, t, 512);
            if (t.ToString().Contains(needle)) { result = h; return false; }
            return true;
        }, IntPtr.Zero);
        return result;
    }
}
'@

$dpiMode = [WinCap]::MakeDpiAware()

# 位图部分放在 PowerShell 里做：Add-Type 编译 C# 时默认不引用
# System.Drawing.Common，在 C# 里写 System.Drawing.Bitmap 会编译失败（CS1069）。
function Save-WindowPng {
    param([IntPtr]$Hwnd, [string]$Path)
    $r = New-Object WinCap+RECT
    [void][WinCap]::GetWindowRect($Hwnd, [ref]$r)
    $w = $r.Right - $r.Left
    $h = $r.Bottom - $r.Top
    if ($w -le 0 -or $h -le 0) { return $false }

    $bmp = [System.Drawing.Bitmap]::new($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [void][WinCap]::PrintWindow($Hwnd, $hdc, 2) # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return $true
}

# ── 开始 ─────────────────────────────────────────────────────────────────

Write-Host '==> 清理已有实例与上次的测试数据' -ForegroundColor Cyan
Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 600

# ★ 数据库必须清掉。上次运行可能把「以管理员身份运行」设成了开，
#   那样应用一启动就会**自我提权重启**（ShellExecute runas -> 本进程退出 -> 新实例），
#   而本脚本跟踪的是最初那个 pid，于是会得到一串看起来像产品故障的假失败：
#   「进程存活」失败、「第二次启动不产生第二个进程」变成 1->2、窗口矩形成 0x0。
#   实测就是这么被坑过一次，排查花了不少时间。
$dataDirEarly = Join-Path $AppDir 'data'
if (Test-Path $dataDirEarly) {
    Get-ChildItem (Join-Path $dataDirEarly 'data.db*') -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $dataDirEarly 'topmost-pin.log') -Force -ErrorAction SilentlyContinue
}

Write-Host '==> PE 子系统' -ForegroundColor Cyan
$sub = Get-PeSubsystem $ExePath
$subName = switch ($sub) { 2 { 'Windows GUI' } 3 { 'Console' } default { "Unknown($sub)" } }
Check 'exe 是 GUI 子系统（运行时无控制台黑窗）' ($sub -eq 2) "subsystem=$sub ($subName)"
Write-Host ("    本次截图进程 DPI 感知模式：{0}" -f $dpiMode)

$proc = $null
try {
    Write-Host '==> 启动' -ForegroundColor Cyan
    $proc = Start-Process -FilePath $ExePath -PassThru

    # ── 抓启动动画 ────────────────────────────────────────────────────────
    # 动画窗只存在约 1.5 秒，必须在它活着的时候截下来。
    # 注意「窗口出现」不等于「内容画好」：WebView2 首帧要晚几百毫秒，
    # 所以这里持续重拍到窗口消失，**保留最后一帧** —— 那正是动画的完整状态。
    $splashShot = $null
    $sw = [IntPtr]::Zero
    $deadline = (Get-Date).AddSeconds(8)
    while ((Get-Date) -lt $deadline -and $sw -eq [IntPtr]::Zero) {
        $sw = [WinCap]::FindSmallWindow($proc.Id, 'Tauri Window', 900, 480)
        if ($sw -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 70 }
    }

    if ($sw -ne [IntPtr]::Zero) {
        $candidate = Join-Path $OutDir 'splash.png'
        $until = (Get-Date).AddSeconds(5)
        while ((Get-Date) -lt $until) {
            Start-Sleep -Milliseconds 140
            try {
                if (-not (Save-WindowPng -Hwnd $sw -Path $candidate)) { break }
                $splashShot = $candidate
            } catch {
                break # 窗口已销毁
            }
        }
    }
    Check '启动动画窗口出现过并被截下' ($null -ne $splashShot) $(if ($splashShot) { $splashShot } else { '8 秒内没抓到' })

    Start-Sleep -Seconds 8

    Check '进程存活' (-not $proc.HasExited) "pid=$($proc.Id)"

    $wins = [WinCap]::WindowsOfProcess($proc.Id)
    Write-Host ("    该进程的顶层窗口 {0} 个：" -f $wins.Count)
    foreach ($w in $wins) { Write-Host "      $w" }

    # 启动动画窗与主窗口同名，取面积最大的那个才是主窗口
    $hwnd = [WinCap]::FindLargestByTitle($WindowTitle)
    Check '主窗口已创建' ($hwnd -ne [IntPtr]::Zero) ("hwnd=0x{0:X}" -f $hwnd.ToInt64())
    # 直接查有没有 ConsoleWindowClass，比只看 PE 子系统更贴近「用户会不会看到黑窗」
    Check '不存在控制台窗口' (-not [WinCap]::HasConsoleWindow($proc.Id))

    $dataDir = Join-Path $AppDir 'data'
    Check '便携数据目录落在 exe 同目录' (Test-Path $dataDir) $dataDir

    $webviewDir = Join-Path $dataDir 'webview'
    Check 'WebView2 缓存已初始化' (Test-Path $webviewDir) $webviewDir

    # ── SQLite 数据文件 ──────────────────────────────────────────────────
    $dbPath = Join-Path $dataDir 'data.db'
    Check 'SQLite 数据文件已创建' (Test-Path $dbPath) $dbPath
    if (Test-Path $dbPath) {
        # 应用还活着（WAL 模式会持有句柄），因此必须带 FileShare.ReadWrite 才能读
        $head = New-Object byte[] 16
        $fs = [System.IO.File]::Open(
            $dbPath,
            [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::Read,
            [System.IO.FileShare]::ReadWrite)
        try { [void]$fs.Read($head, 0, 16) } finally { $fs.Dispose() }

        # SQLite 文件头 16 字节固定是 "SQLite format 3\0"
        $magic = -join ($head | ForEach-Object {
                if ($_ -ge 32 -and $_ -lt 127) { [char]$_ } else { '.' }
            })
        Check '数据文件是合法 SQLite' ($magic -like 'SQLite format 3*') $magic
    }

    # ── 启动动画与权限状态（读应用自己的日志，比猜时序可靠）─────────────
    $logPath = Join-Path $dataDir 'topmost-pin.log'
    if (Test-Path $logPath) {
        $logText = Get-Content $logPath -Raw
        Check '启动动画已收尾并放出主窗口' ($logText -match '启动动画结束') 
        Check '权限状态已判定并记录' ($logText -match '权限：设置=') 
    } else {
        Check '应用日志已生成' $false $logPath
    }

    Write-Host '==> 单实例' -ForegroundColor Cyan
    $before = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue).Count
    $second = Start-Process -FilePath $ExePath -PassThru
    Start-Sleep -Seconds 5
    $after = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
    Check '第二次启动不产生第二个进程' ($after.Count -eq $before) "启动前=$before 启动后=$($after.Count)"

    Write-Host '==> 截图验证界面真的渲染了' -ForegroundColor Cyan
    # ★ 每轮都**重新查找**窗口，而不是复用早先拿到的句柄。
    # 启动动画窗与主窗口标题相同，早先拿到的可能是动画窗；它随后被销毁，
    # 句柄失效后 GetWindowRect 返回 0x0 —— 这个假失败已经出现过两次。
    $r = New-Object WinCap+RECT
    $w = 0
    $h = 0
    for ($i = 0; $i -lt 30; $i++) {
        $cand = [WinCap]::FindLargestByTitle($WindowTitle)
        if ($cand -ne [IntPtr]::Zero) {
            [void][WinCap]::GetWindowRect($cand, [ref]$r)
            $w = $r.Right - $r.Left
            $h = $r.Bottom - $r.Top
            if ($w -gt 100 -and $h -gt 100) { $hwnd = $cand; break }
        }
        Start-Sleep -Milliseconds 250
    }

    if ($w -gt 100 -and $h -gt 100) {
            $bmp = [System.Drawing.Bitmap]::new($w, $h)
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            $hdc = $g.GetHdc()
            [void][WinCap]::PrintWindow($hwnd, $hdc, 2)
            $g.ReleaseHdc($hdc)
            $g.Dispose()

            $shot = Join-Path $OutDir 'main-window.png'
            $bmp.Save($shot, [System.Drawing.Imaging.ImageFormat]::Png)

            # 判据：界面不是纯白/纯黑一片。全白通常是 WebView 白屏（内嵌资源没加载上）。
            $colors = @{}
            for ($y = 0; $y -lt $h; $y += 7) {
                for ($x = 0; $x -lt $w; $x += 7) {
                    $c = $bmp.GetPixel($x, $y)
                    $colors['{0},{1},{2}' -f $c.R, $c.G, $c.B] = 1
                }
            }
            $bmp.Dispose()

            Check '窗口尺寸合理' ($true) ("{0}x{1}" -f $w, $h)
            Check '界面已渲染（采样到多种颜色，非白屏）' ($colors.Count -gt 8) "采样到 $($colors.Count) 种颜色"
            Write-Host ("    截图: {0}" -f $shot)
    } else {
        # 失败时把标题匹配的窗口全打出来，便于区分「句柄失效」与「窗口真的没出现」
        Write-Host '    标题匹配的可见窗口：' -ForegroundColor DarkGray
        foreach ($line in [WinCap]::AllWindows()) {
            if ($line -match $WindowTitle) { Write-Host "      $line" -ForegroundColor DarkGray }
        }
        Check '窗口尺寸合理' $false ("{0}x{1}" -f $w, $h)
    }
} finally {
    Write-Host '==> 收尾' -ForegroundColor Cyan
    Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 900
    Check '进程已全部退出' (@(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue).Count -eq 0)
}

Write-Host ''
if ($failures.Count -eq 0) {
    Write-Host '冒烟测试全部通过。' -ForegroundColor Green
    exit 0
} else {
    Write-Host ("冒烟测试有 {0} 项失败：{1}" -f $failures.Count, ($failures -join '、')) -ForegroundColor Red
    exit 1
}
