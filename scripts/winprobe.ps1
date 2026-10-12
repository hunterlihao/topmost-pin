<#
.SYNOPSIS
    窗口置顶状态断言工具。

.DESCRIPTION
    直接读窗口的 WS_EX_TOPMOST 位，用来给验收提供可脚本化的判据 ——
    不依赖肉眼观察，也不依赖本程序自己的上报。

    GWL_EXSTYLE = -20，WS_EX_TOPMOST = 0x00000008。

.PARAMETER List
    列出所有可见的、有标题的顶层窗口。

.PARAMETER Title
    标题子串，大小写不敏感。可匹配到多个窗口时，要求**全部**满足预期。

.PARAMETER Expect
    TopMost / NotTopMost。断言失败时抛错并返回退出码 1。

.EXAMPLE
    ./winprobe.ps1 -List
.EXAMPLE
    ./winprobe.ps1 -Title "命令提示符" -Expect TopMost
#>
[CmdletBinding(DefaultParameterSetName = 'List')]
param(
    [Parameter(ParameterSetName = 'List')]
    [switch]$List,

    [Parameter(Mandatory = $true, ParameterSetName = 'Assert')]
    [string]$Title,

    [Parameter(Mandatory = $true, ParameterSetName = 'Assert')]
    [ValidateSet('TopMost', 'NotTopMost')]
    [string]$Expect
)

$ErrorActionPreference = 'Stop'

Add-Type -Namespace Win32 -Name Probe -MemberDefinition @'
public delegate bool EnumWindowsProc(System.IntPtr hWnd, System.IntPtr lParam);

[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, System.IntPtr lParam);

[System.Runtime.InteropServices.DllImport("user32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern int GetWindowTextW(System.IntPtr hWnd, System.Text.StringBuilder lpString, int nMaxCount);

[System.Runtime.InteropServices.DllImport("user32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern int GetClassNameW(System.IntPtr hWnd, System.Text.StringBuilder lpClassName, int nMaxCount);

[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern bool IsWindowVisible(System.IntPtr hWnd);

// 本工具只跑在 x64 上，直接用 64 位入口点
[System.Runtime.InteropServices.DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
public static extern System.IntPtr GetWindowLongPtrW(System.IntPtr hWnd, int nIndex);
'@

$GWL_EXSTYLE = -20
$WS_EX_TOPMOST = 0x00000008

function Get-TopMostState {
    param([IntPtr]$Hwnd)
    $ex = [Win32.Probe]::GetWindowLongPtrW($Hwnd, $GWL_EXSTYLE).ToInt64()
    return (($ex -band $WS_EX_TOPMOST) -ne 0)
}

function Get-VisibleTopLevelWindows {
    $script:probeResults = [System.Collections.Generic.List[object]]::new()

    $callback = [Win32.Probe+EnumWindowsProc] {
        param([IntPtr]$hWnd, [IntPtr]$lParam)

        if ([Win32.Probe]::IsWindowVisible($hWnd)) {
            $titleBuf = [System.Text.StringBuilder]::new(512)
            [void][Win32.Probe]::GetWindowTextW($hWnd, $titleBuf, 512)
            $title = $titleBuf.ToString()

            if ($title.Trim().Length -gt 0) {
                $classBuf = [System.Text.StringBuilder]::new(256)
                [void][Win32.Probe]::GetClassNameW($hWnd, $classBuf, 256)

                $script:probeResults.Add([pscustomobject]@{
                        Hwnd    = $hWnd
                        Handle  = ('0x{0:X}' -f $hWnd.ToInt64())
                        Title   = $title
                        Class   = $classBuf.ToString()
                        TopMost = (Get-TopMostState -Hwnd $hWnd)
                    })
            }
        }
        return $true
    }

    # 委托必须保持强引用，否则可能在枚举过程中被 GC 回收
    [void][Win32.Probe]::EnumWindows($callback, [IntPtr]::Zero)
    return $script:probeResults
}

$windows = Get-VisibleTopLevelWindows

if ($PSCmdlet.ParameterSetName -eq 'List') {
    if ($windows.Count -eq 0) {
        Write-Host '没有找到可见的顶层窗口。'
        exit 0
    }
    $windows |
        Sort-Object -Property @{ Expression = 'TopMost'; Descending = $true }, Title |
        Format-Table Handle, TopMost, Class, Title -AutoSize |
        Out-String -Width 400 |
        Write-Host
    Write-Host ("共 {0} 个窗口，其中已置顶 {1} 个。" -f $windows.Count, ($windows | Where-Object TopMost).Count)
    exit 0
}

# 标题里可能含 [ ] * ? 这些通配符（例如「[WIP] 记事本」），
# 不转义的话 -like 会把它们当字符集，结果是匹配到别的窗口、或者一个都匹配不到。
$pattern = '*' + [System.Management.Automation.WildcardPattern]::Escape($Title) + '*'
$matched = @($windows | Where-Object { $_.Title -like $pattern })

if ($matched.Count -eq 0) {
    # ★ 必须 -ErrorAction Continue：脚本开头设了 $ErrorActionPreference='Stop'，
    # 裸 Write-Error 在那是**终止性**错误，下面的 exit 1 根本执行不到，
    # 调用方（& ./winprobe.ps1）拿到的是终止性异常而不是退出码。
    Write-Error "没有找到标题包含「$Title」的可见窗口。" -ErrorAction Continue
    exit 1
}

$want = $Expect -eq 'TopMost'
$wrong = @($matched | Where-Object { $_.TopMost -ne $want })

foreach ($w in $matched) {
    $mark = if ($w.TopMost -eq $want) { 'OK  ' } else { 'FAIL' }
    Write-Host ("{0} {1}  {2}" -f $mark, $w.Handle, $w.Title)
}

if ($wrong.Count -gt 0) {
    Write-Error ("断言失败：期望 {0}，但有 {1}/{2} 个匹配窗口不满足。" -f $Expect, $wrong.Count, $matched.Count) -ErrorAction Continue
    exit 1
}

Write-Host ("断言通过：{0} 个匹配窗口均为 {1}。" -f $matched.Count, $Expect)
exit 0
