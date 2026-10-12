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

# ── 开始 ─────────────────────────────────────────────────────────────────

Write-Host '==> 清理已有实例' -ForegroundColor Cyan
Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 600

Write-Host '==> PE 子系统' -ForegroundColor Cyan
$sub = Get-PeSubsystem $ExePath
$subName = switch ($sub) { 2 { 'Windows GUI' } 3 { 'Console' } default { "Unknown($sub)" } }
Check 'exe 是 GUI 子系统（运行时无控制台黑窗）' ($sub -eq 2) "subsystem=$sub ($subName)"
Write-Host ("    本次截图进程 DPI 感知模式：{0}" -f $dpiMode)

$proc = $null
try {
    Write-Host '==> 启动' -ForegroundColor Cyan
    $proc = Start-Process -FilePath $ExePath -PassThru
    Start-Sleep -Seconds 8

    Check '进程存活' (-not $proc.HasExited) "pid=$($proc.Id)"

    $wins = [WinCap]::WindowsOfProcess($proc.Id)
    Write-Host ("    该进程的顶层窗口 {0} 个：" -f $wins.Count)
    foreach ($w in $wins) { Write-Host "      $w" }

    $hwnd = [WinCap]::FindByTitle($WindowTitle)
    if ($hwnd -eq [IntPtr]::Zero -and $wins.Count -gt 0) {
        # 兜底：标题可能被本地化或尚未设置完，退而用第一个可见窗口
        $hwnd = [WinCap]::FindByTitle('Tauri')
    }
    Check '主窗口已创建' ($hwnd -ne [IntPtr]::Zero) ("hwnd=0x{0:X}" -f $hwnd.ToInt64())

    $dataDir = Join-Path $AppDir 'data'
    Check '便携数据目录落在 exe 同目录' (Test-Path $dataDir) $dataDir

    $webviewDir = Join-Path $dataDir 'webview'
    Check 'WebView2 缓存已初始化' (Test-Path $webviewDir) $webviewDir

    Write-Host '==> 单实例' -ForegroundColor Cyan
    $before = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue).Count
    $second = Start-Process -FilePath $ExePath -PassThru
    Start-Sleep -Seconds 5
    $after = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
    Check '第二次启动不产生第二个进程' ($after.Count -eq $before) "启动前=$before 启动后=$($after.Count)"

    Write-Host '==> 截图验证界面真的渲染了' -ForegroundColor Cyan
    if ($hwnd -ne [IntPtr]::Zero) {
        $r = New-Object WinCap+RECT
        [void][WinCap]::GetWindowRect($hwnd, [ref]$r)
        $w = $r.Right - $r.Left
        $h = $r.Bottom - $r.Top

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
            Check '窗口尺寸合理' $false ("{0}x{1}" -f $w, $h)
        }
    } else {
        Check '界面已渲染' $false '窗口不存在，无法截图'
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
