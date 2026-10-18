# 宣传片素材抓图：真实运行中的「窗口置顶助手」窗口。
#
# 为什么不用 PrintWindow：对 WebView2 实测返回纯黑（mean=0.0），不可用。
# 这里走 SetForegroundWindow -> GetWindowRect -> Graphics.CopyFromScreen：
# 拿到的是屏幕合成后的像素，代价是窗口必须完整可见、且不能被别的置顶窗口压住。
#
# 用法：
#   pwsh -NoProfile -File scripts\shoot-window.ps1 -Name main -Tab window
#   pwsh -NoProfile -File scripts\shoot-window.ps1 -Name settings -Tab settings
#   -Click "x,y" 可追加相对窗口左上角的点击（150% 缩放下的物理像素）
param(
  [Parameter(Mandatory = $true)][string]$Name,
  [ValidateSet("none", "window", "settings")][string]$Tab = "none",
  [string[]]$Click = @(),
  [int]$SettleMs = 900
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinShot {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][WinShot]::SetProcessDPIAware()

$proc = Get-Process | Where-Object { $_.MainWindowTitle -eq "窗口置顶助手" } | Select-Object -First 1
if (-not $proc) { throw "找不到「窗口置顶助手」窗口" }
$hwnd = $proc.MainWindowHandle

function Get-Rect {
  $r = New-Object WinShot+RECT
  [void][WinShot]::GetWindowRect($hwnd, [ref]$r)
  return $r
}

[void][WinShot]::ShowWindow($hwnd, 9)      # SW_RESTORE
[void][WinShot]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds 500

function Invoke-Click([int]$x, [int]$y) {
  $r = Get-Rect
  [void][WinShot]::SetCursorPos($r.Left + $x, $r.Top + $y)
  Start-Sleep -Milliseconds 150
  [void][WinShot]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)   # LEFTDOWN
  Start-Sleep -Milliseconds 70
  [void][WinShot]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)   # LEFTUP
  Start-Sleep -Milliseconds 450
}

# 页签在窗口内的相对坐标（150% 缩放下的物理像素，由首帧截图量得）
if ($Tab -eq "settings") { Invoke-Click 356 84 }
if ($Tab -eq "window")   { Invoke-Click 285 84 }
foreach ($c in $Click) {
  $xy = $c -split ','
  Invoke-Click ([int]$xy[0]) ([int]$xy[1])
}

[void][WinShot]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds $SettleMs

$r = Get-Rect
$w = $r.Right - $r.Left
$h = $r.Bottom - $r.Top
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
$out = Join-Path $PSScriptRoot "..\docs\promo\$Name.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"$Name -> $out  ${w}x${h}  rect=$($r.Left),$($r.Top)"
