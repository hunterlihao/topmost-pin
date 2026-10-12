# 把预览页逐个渲染成 PNG，供设计自检。
#
# 用 Edge 的 headless 截图（机器上已装，版本 155）。deviceScaleFactor 固定 1，
# 所以 PNG 的像素尺寸就等于 CSS 像素尺寸 —— 看间距和字号时是 1:1 的。
#
# 前置：先跑 make-preview.mjs 生成 dist/preview-*.html，再起 serve.mjs。

$ErrorActionPreference = "Stop"
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$outDir = Join-Path $root "artifacts\preview"
$edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
if (-not (Test-Path $edge)) { throw "找不到 msedge.exe" }

$port = 4199
$profile = Join-Path $outDir ".edge-profile"

# 每个状态都可以单独指定窗口宽度：窄窗断点必须真的按窄窗渲染才能检查
$jobs = @(
    @{ State = "empty";            W = 940; H = 680 },
    @{ State = "deck";             W = 940; H = 680 },
    @{ State = "many";             W = 940; H = 680 },
    @{ State = "allpinned";        W = 940; H = 680 },
    @{ State = "noadmin";          W = 940; H = 680 },
    @{ State = "nomatch";          W = 940; H = 680 },
    @{ State = "settings";         W = 940; H = 680 },
    @{ State = "settings-noadmin"; W = 940; H = 680 },
    @{ State = "deck";             W = 780; H = 620; Name = "narrow" }
)

foreach ($j in $jobs) {
    $name = if ($j.Name) { $j.Name } else { $j.State }
    $shot = Join-Path $outDir "$name.png"
    Remove-Item $shot -Force -ErrorAction SilentlyContinue

    # 注意别叫 $args —— 那是 PowerShell 的自动变量
    $edgeArgs = @(
        "--headless=new"
        "--disable-gpu"
        "--no-first-run"
        "--no-default-browser-check"
        "--disable-extensions"
        "--hide-scrollbars"
        "--force-device-scale-factor=1"
        "--user-data-dir=$profile"
        "--window-size=$($j.W),$($j.H)"
        # 虚拟时间：让 setTimeout / rAF 都跑完再截，避免截到「正在读取窗口…」
        "--virtual-time-budget=4000"
        "--screenshot=$shot"
        "http://127.0.0.1:$port/preview-$($j.State).html"
    )
    & $edge @edgeArgs 2>&1 | Out-Null

    if (Test-Path $shot) {
        $size = (Get-Item $shot).Length
        Write-Host ("  {0,-18} {1}x{2}  {3:N0} B" -f $name, $j.W, $j.H, $size)
    } else {
        Write-Host ("  {0,-18} 截图失败" -f $name) -ForegroundColor Red
    }
}

Remove-Item $profile -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "==> 截图落点：$outDir" -ForegroundColor Green
