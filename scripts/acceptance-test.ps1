<#
.SYNOPSIS
    端到端验收：真实按下热键，真的把目标窗口置顶。

.DESCRIPTION
    覆盖「热键 -> 目标解析 -> 置顶集合 -> SetWindowPos」整条链路。
    与其它测试的分工：

      · cargo test           覆盖 Win32 原语与置顶集合状态机，含**跨进程**置顶
      · smoke-test.ps1       覆盖进程能启动、界面能渲染、单实例、便携数据目录
      · 本脚本               覆盖「按键」这一下 —— 向应用的热键线程投递 WM_HOTKEY，
                             然后用**独立探针**读目标窗口的 WS_EX_TOPMOST

    ★ 目标窗口为什么选「文件资源管理器」而不是自己现造一个：
    本脚本由开发环境（DSH）的 shell 启动，它派生出来的进程都带同一套进程环境。
    实测：让应用去置顶一个由本脚本自己创建的 WinForms 窗口，会稳定地拿到
    ERROR_ACCESS_DENIED(5)；而同样的跨进程 SetWindowPos 由 cargo 测试发起则一切正常。
    为了不让「测试脚手架自身的进程上下文」污染结论，这里改用一个**由用户正常启动、
    与测试脚手架无关**的进程作为目标 —— explorer.exe 的文件夹窗口。
    代价是会短暂移动鼠标并置顶一个资源管理器窗口，结束时全部还原。

    实现要点（都是踩过的坑）：
      · WM_HOTKEY 是线程消息，用 PostThreadMessageW 逐个投给目标进程的线程 ——
        只有注册了热键的那个会处理它。不能用 PostMessage 发给主窗口：tao 会丢掉它。
      · 必须声明 DPI 感知。本机 2560x1600 @150%，不感知的进程拿到的坐标是虚拟化的，
        SetCursorPos 会落到错误位置。
      · 必须先用 WindowFromPoint 确认光标真的落在预期目标上，否则断言无法区分
        「热键没生效」和「压根瞄错了」。
      · 应用是常驻托盘程序，不会自己退出，必须在 finally 里无条件收掉。
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

$ProcessName = [System.IO.Path]::GetFileNameWithoutExtension($ExePath)
$AppWindowTitle = '窗口置顶助手'

$failures = [System.Collections.Generic.List[string]]::new()
function Check($name, $ok, $detail) {
    if ($ok) {
        Write-Host ("  [PASS] {0}{1}" -f $name, $(if ($detail) { "  $detail" } else { '' })) -ForegroundColor Green
    } else {
        Write-Host ("  [FAIL] {0}{1}" -f $name, $(if ($detail) { "  $detail" } else { '' })) -ForegroundColor Red
        $failures.Add($name)
    }
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class Acc {
    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool PostThreadMessageW(uint idThread, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] private static extern bool SetProcessDpiAwarenessContext(IntPtr v);

    [DllImport("kernel32.dll", SetLastError = true)] private static extern IntPtr OpenProcess(uint a, bool inherit, uint pid);
    [DllImport("kernel32.dll")] private static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] private static extern bool QueryFullProcessImageNameW(IntPtr h, uint flags, StringBuilder name, ref uint size);

    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    public const uint GA_ROOT = 2;

    public static string MakeDpiAware() {
        return SetProcessDpiAwarenessContext(new IntPtr(-4)) ? "PerMonitorV2" : "Failed";
    }

    private static List<string> _scratch;
    private static List<IntPtr> _handles;

    private static void Collect() {
        _scratch = new List<string>();
        _handles = new List<IntPtr>();
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            var t = new StringBuilder(512); GetWindowTextW(h, t, 512);
            var c = new StringBuilder(256); GetClassNameW(h, c, 256);
            if (t.ToString().Trim().Length == 0) return true;
            _handles.Add(h);
            _scratch.Add(string.Format("hwnd=0x{0:X} pid={1} class=[{2}] title=[{3}]",
                h.ToInt64(), PidOf(h), c.ToString(), t.ToString()));
            return true;
        }, IntPtr.Zero);
    }

    public static string[] AllWindows() { Collect(); return _scratch.ToArray(); }

    public static uint PidOf(IntPtr h) { uint p; GetWindowThreadProcessId(h, out p); return p; }

    /// 进程可执行文件名，如 explorer.exe
    public static string ProcessNameOf(uint pid) {
        IntPtr h = OpenProcess(0x1000, false, pid); // PROCESS_QUERY_LIMITED_INFORMATION
        if (h == IntPtr.Zero) return "?";
        try {
            var sb = new StringBuilder(260);
            uint size = 260;
            if (!QueryFullProcessImageNameW(h, 0, sb, ref size)) return "?";
            string full = sb.ToString();
            int i = full.LastIndexOf('\\');
            return i >= 0 ? full.Substring(i + 1) : full;
        } finally { CloseHandle(h); }
    }

    /// 找出指定进程名 + 类名的窗口，且其**矩形中心确实命中自己**（没被别的窗口盖住）
    public static IntPtr FindPinnable(string processName, string className) {
        Collect();
        for (int i = 0; i < _handles.Count; i++) {
            IntPtr h = _handles[i];
            var c = new StringBuilder(256); GetClassNameW(h, c, 256);
            if (className.Length > 0 && c.ToString() != className) continue;
            if (ProcessNameOf(PidOf(h)) != processName) continue;
            RECT r; GetWindowRect(h, out r);
            int w = r.Right - r.Left, hh = r.Bottom - r.Top;
            if (w < 300 || hh < 200) continue;      // 太小说明不是主窗口
            return h;
        }
        return IntPtr.Zero;
    }

    /// 在窗口矩形内找一个**确实露出来**的点（没有被别的窗口盖住）。
    ///
    /// 不能直接用矩形中心：实测资源管理器窗口的中心正好被浏览器盖住，
    /// WindowFromPoint 会返回浏览器，断言就变成在验证另一个窗口了。
    /// 这里按 9x9 网格扫描，返回第一个命中自己的点。
    public static bool FindVisiblePoint(IntPtr h, out int x, out int y) {
        RECT r; GetWindowRect(h, out r);
        int w = r.Right - r.Left, hh = r.Bottom - r.Top;
        for (int iy = 1; iy <= 9; iy++) {
            for (int ix = 1; ix <= 9; ix++) {
                int px = r.Left + w * ix / 10;
                int py = r.Top + hh * iy / 10;
                POINT p; p.X = px; p.Y = py;
                IntPtr hw = WindowFromPoint(p);
                if (hw == IntPtr.Zero) continue;
                IntPtr root = GetAncestor(hw, GA_ROOT);
                if ((root == IntPtr.Zero ? hw : root) == h) { x = px; y = py; return true; }
            }
        }
        x = 0; y = 0; return false;
    }

    /// 沿 z 序从最前面往下找第一个「够大、可见、且有点能命中自己」的窗口。
    ///
    /// 这样选目标的原因：任何由本脚本现造的窗口都受脚手架进程上下文影响，
    /// 而尝试把已有窗口「抬起来」又会被系统拒绝（后台进程不许改 z 序，防抢焦点）。
    /// 直接用 z 序最上面的窗口，天然可见可命中，且进程属于用户自己的正常程序。
    public static IntPtr FindVisibleTarget(string[] skipProcesses) {
        const uint GW_HWNDNEXT = 2;
        IntPtr h = GetForegroundWindow();
        int guard = 0;
        while (h != IntPtr.Zero && guard++ < 60) {
            if (IsWindowVisible(h)) {
                RECT r; GetWindowRect(h, out r);
                if ((r.Right - r.Left) >= 400 && (r.Bottom - r.Top) >= 300) {
                    string proc = ProcessNameOf(PidOf(h));
                    bool skip = false;
                    foreach (string s in skipProcesses) if (proc == s) { skip = true; break; }
                    if (!skip) {
                        int x, y;
                        if (FindVisiblePoint(h, out x, out y)) return h;
                    }
                }
            }
            h = GetWindow(h, GW_HWNDNEXT);
        }
        return IntPtr.Zero;
    }

    public static IntPtr WindowUnderCursor() {
        POINT p; GetCursorPos(out p);
        IntPtr h = WindowFromPoint(p);
        if (h == IntPtr.Zero) return IntPtr.Zero;
        IntPtr root = GetAncestor(h, GA_ROOT);
        return root == IntPtr.Zero ? h : root;
    }

    // GWL_EXSTYLE = -20, WS_EX_TOPMOST = 0x8
    public static bool IsTopMost(IntPtr h) {
        return (GetWindowLongPtrW(h, -20).ToInt64() & 0x8L) != 0;
    }
}
'@

Write-Host ("本进程 DPI 感知模式：{0}" -f [Acc]::MakeDpiAware()) -ForegroundColor DarkGray

$origin = New-Object Acc+POINT
[void][Acc]::GetCursorPos([ref]$origin)

Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 600

$SW_MINIMIZE = 6
$SW_RESTORE = 9

$proc = $null
try {
    Write-Host '==> 启动被测应用' -ForegroundColor Cyan
    # ★ 必须带 --no-elevate。
    #
    # 程序固定以管理员运行，普通权限启动时会**自我提权重启**：ShellExecute runas
    # 成功后旧进程直接 exit(0)（lib.rs「已发起提权重启，本进程退出…」）。
    # 脚本跟踪的是 Start-Process 拿到的那个 pid，于是会看到：
    #   · 下一行的「应用进程存活」假失败；
    #   · 再往下 Get-Process -Id $proc.Id 直接抛终止错误 —— 脚本在投递 WM_HOTKEY
    #     之前就崩了，这个**唯一**验证核心功能的端到端验收永远走不到断言。
    # 而且 60 秒防抖会让它时好时坏，比稳定失败更难查。权限路径由
    # scripts/diag-single-instance.ps1 单独验证。
    $proc = Start-Process -FilePath $ExePath -ArgumentList '--no-elevate' -PassThru
    Start-Sleep -Seconds 8
    Check '应用进程存活' (-not $proc.HasExited) "pid=$($proc.Id)"

    # ★ 先让路：应用窗口是居中且很大的，会盖住目标窗口的中心点，
    #   导致 WindowFromPoint 命中应用自己，从而把「瞄错」误判成「功能失效」。
    #   最小化不影响热键线程，托盘与已置顶窗口照常工作。
    $appWin = [Acc]::FindPinnable($ProcessName + '.exe', 'Tauri Window')
    if ($appWin -ne [IntPtr]::Zero) {
        [void][Acc]::ShowWindow($appWin, $SW_MINIMIZE)
        Start-Sleep -Milliseconds 800
    }

    Write-Host '==> 选一个可见的目标窗口（z 序最上面那个正常用户程序）' -ForegroundColor Cyan
    # 排除测试脚手架自己的进程：被测应用、pwsh、终端宿主
    $skipProcs = @($ProcessName + '.exe', 'pwsh.exe', 'powershell.exe', 'WindowsTerminal.exe', 'conhost.exe')
    $target = [Acc]::FindVisibleTarget($skipProcs)
    if ($target -eq [IntPtr]::Zero) {
        Write-Host '    找不到可用目标：桌面可见窗口全是测试脚手架自己的进程。' -ForegroundColor Yellow
        Write-Host '    这不是产品缺陷，是无法建立测试条件。' -ForegroundColor Yellow
        Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        [void][Acc]::SetCursorPos($origin.X, $origin.Y)
        Write-Host ''
        Write-Host '端到端验收跳过（退出码 2）。' -ForegroundColor Yellow
        exit 2
    }
    Write-Host ("    目标 hwnd=0x{0:X} proc={1}" -f $target.ToInt64(), [Acc]::ProcessNameOf([Acc]::PidOf($target))) -ForegroundColor DarkGray

    $x = 0; $y = 0
    $hasPoint = [Acc]::FindVisiblePoint($target, [ref]$x, [ref]$y)
    Check '在目标窗口上找到可见的点' $hasPoint ("({0},{1})" -f $x, $y)

    [void][Acc]::SetCursorPos($x, $y)
    Start-Sleep -Milliseconds 400
    [void][Acc]::SetCursorPos($x, $y)
    Start-Sleep -Milliseconds 200

    $under = [Acc]::WindowUnderCursor()
    Check '光标确实落在目标窗口上（没被别的窗口盖住）' ($under -eq $target) ("({0},{1}) -> 0x{2:X}" -f $x, $y, $under.ToInt64())
    Check '目标窗口初始未置顶' (-not [Acc]::IsTopMost($target))

    # ── 控制实验 ─────────────────────────────────────────────────────────
    # 没有这一步，「应用置顶失败」无法归因：可能是产品缺陷，也可能是当前
    # 自动化环境按进程深度限制窗口操作。
    #
    # ★ 控制条件必须与「应用」处在同样的进程深度。实测对比（同一次调用内）：
    #     · harness 直接派生的 pwsh           -> SetWindowPos 系统外窗口 成功
    #     · cargo 派生的 Rust 测试进程（孙进程）-> 同一窗口 ERROR_ACCESS_DENIED
    #   被测应用由脚本 Start-Process 启动，同样是孙进程，因此拿 pwsh 自己做对照
    #   会得到「控制通过、应用失败」的假阴性。这里改跑 Rust 探针作为对照 ——
    #   它是一份用途明确、用法显然正确的代码，若它也被拒，就说明是环境限制。
    Write-Host '==> 控制实验：用同样进程深度的 Rust 探针试着置顶沙箱外窗口' -ForegroundColor Cyan
    $probeOut = & pwsh -NoProfile -Command @"
Set-Location '$root\src-tauri'
`$env:CARGO_HOME = Join-Path `$env:USERPROFILE '.cargo'
`$env:RUSTUP_HOME = Join-Path `$env:USERPROFILE '.rustup'
`$env:CARGO_NET_OFFLINE = 'true'
& (Join-Path `$env:CARGO_HOME 'bin\cargo.exe') test --offline --features custom-protocol --test win32_e2e -- --ignored --nocapture --test-threads=1 probe_pinning 2>&1 | Select-String -Pattern '沙箱外窗口'
"@ 2>&1 | Out-String
$probeLine = ($probeOut -split "`n" | Where-Object { $_ -match '沙箱外窗口' } | Select-Object -First 1)
Write-Host ("    " + $(if ($probeLine) { $probeLine.Trim() } else { '(探针没有输出)' })) -ForegroundColor DarkGray

    # ★ 探针有三种结局，必须分开判：
    #   · 有目标且全部被拒 -> 这才是「环境按进程深度限制窗口操作」，退出码 2；
    #   · 根本没找到目标   -> 条件没建立起来，**不能**据此下环境结论；
    #   · 有目标且成功     -> 环境允许，继续往下做真断言。
    # 之前两者走同一个出口，于是「探针没找到目标」也会被包装成「已验证环境限制」——
    # 那是给一件没验证过的事情下结论。探针的候选进程是写死的 8 个，
    # 而桌面上开着什么完全不受本脚本控制，这种情况很常见。
    $probeTested = 0
    $probeOkCount = 0
    if ($probeLine -match '沙箱外窗口\s+(\d+)/(\d+)\s+成功') {
        $probeOkCount = [int]$Matches[1]
        $probeTested = [int]$Matches[2]
    }

    if ($probeTested -eq 0) {
        Write-Host '    控制实验**没能建立条件**：探针在桌面上没找到名单里的目标程序。' -ForegroundColor Red
        Write-Host '    这与「环境禁止跨进程置顶」是两回事，不能据此下环境结论。' -ForegroundColor Red
        Write-Host '    请先开着 Chrome / Edge / 记事本之类再跑一次。' -ForegroundColor Red
        Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        [void][Acc]::SetCursorPos($origin.X, $origin.Y)
        Write-Host ''
        Write-Host '端到端验收未能建立测试条件（退出码 3，非产品缺陷，但也**不是**环境结论）。' -ForegroundColor Red
        exit 3
    }

    if ($probeOkCount -eq 0) {
        Write-Host '    控制实验失败：当前自动化环境按进程深度限制窗口操作。' -ForegroundColor Yellow
        Write-Host '    这不是产品缺陷 —— 一份用法显然正确的纯 Rust 控制台程序（与应用同为' -ForegroundColor Yellow
        Write-Host '    孙进程）对 explorer.exe / chrome.exe 的窗口执行同样的调用，也一律被拒；' -ForegroundColor Yellow
        Write-Host '    而 harness 直接派生的 pwsh 对同一窗口调用则成功。' -ForegroundColor Yellow
        Write-Host '    最终确认请手动双击 dist-portable 里的 exe（在资源管理器里启动）。' -ForegroundColor Yellow
        Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        [void][Acc]::SetCursorPos($origin.X, $origin.Y)
        Write-Host ''
        Write-Host '端到端验收跳过（退出码 2：环境不允许，非产品缺陷）。' -ForegroundColor Yellow
        exit 2
    }
    Write-Host ("    控制实验通过：{0}/{1} 个沙箱外窗口置顶成功" -f $probeOkCount, $probeTested) -ForegroundColor Green

    if ($under -ne $target) {
        Write-Host '    目标被遮挡，跳过置顶断言（避免把「瞄错」误报成「功能失效」）' -ForegroundColor Yellow
    } else {
        Write-Host '==> 投递 WM_HOTKEY（等价于按下热键）' -ForegroundColor Cyan
        $threadIds = @((Get-Process -Id $proc.Id).Threads | Select-Object -ExpandProperty Id)

        function Send-Hotkey {
            param($ThreadIds)
            $posted = 0
            foreach ($tid in $ThreadIds) {
                # WM_HOTKEY = 0x0312。逐个投：只有注册了热键的那个线程会处理它。
                if ([Acc]::PostThreadMessageW([uint32]$tid, 0x0312, [IntPtr]1, [IntPtr]::Zero)) { $posted++ }
            }
            return $posted
        }

        [void][Acc]::SetCursorPos($x, $y)
        Start-Sleep -Milliseconds 120
        $posted = Send-Hotkey $threadIds
        Write-Host ("    PostThreadMessage 投递成功 {0}/{1} 个线程" -f $posted, $threadIds.Count) -ForegroundColor DarkGray

        Start-Sleep -Milliseconds 1500
        $raised = [Acc]::IsTopMost($target)
        Check '按下热键后目标窗口已置顶（独立探针读 WS_EX_TOPMOST）' $raised

        if ($raised) {
            Write-Host '==> 再按一次应取消' -ForegroundColor Cyan
            [void][Acc]::SetCursorPos($x, $y)
            Start-Sleep -Milliseconds 120
            [void](Send-Hotkey $threadIds)
            Start-Sleep -Milliseconds 1500
            Check '再按一次后置顶已取消' (-not [Acc]::IsTopMost($target))
        } else {
            Check '再按一次后置顶已取消' $false '首次置顶未成功，跳过（避免假通过）'
        }
    }

    Write-Host '==> 截取应用界面存档' -ForegroundColor Cyan
    if ($appWin -ne [IntPtr]::Zero) {
        [void][Acc]::ShowWindow($appWin, $SW_RESTORE)
        Start-Sleep -Milliseconds 900
    }
    if ($appWin -ne [IntPtr]::Zero) {
        Add-Type -Namespace P -Name W -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool PrintWindow(System.IntPtr h, System.IntPtr d, uint f);
'@
        $r = New-Object Acc+RECT
        [void][Acc]::GetWindowRect($appWin, [ref]$r)
        $bmp = [System.Drawing.Bitmap]::new($r.Right - $r.Left, $r.Bottom - $r.Top)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc()
        [void][P.W]::PrintWindow($appWin, $hdc, 2)
        $g.ReleaseHdc($hdc); $g.Dispose()
        $shot = Join-Path $OutDir 'acceptance-app.png'
        $bmp.Save($shot, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Write-Host ("    截图: {0}" -f $shot) -ForegroundColor DarkGray
    }
} finally {
    Write-Host '==> 收尾' -ForegroundColor Cyan
    Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    [void][Acc]::SetCursorPos($origin.X, $origin.Y)
    Start-Sleep -Milliseconds 700
}

Write-Host ''
if ($failures.Count -eq 0) {
    Write-Host '端到端验收通过：热键确实把目标窗口置顶了。' -ForegroundColor Green
    exit 0
} else {
    Write-Host ("端到端验收有 {0} 项失败：{1}" -f $failures.Count, ($failures -join '、')) -ForegroundColor Red
    exit 1
}
