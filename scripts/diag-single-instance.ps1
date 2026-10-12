# 干净环境下的启动行为诊断：权限自我切换 + 单实例。
#
# 必须以子进程方式运行（& pwsh -NoProfile -File ...）：
# harness 自己直接派生 GUI 进程时，工具调用会一直等下去。
#
# 注意这里**不加** --no-elevate：要验的就是真实的默认路径
# （应用出厂默认以管理员运行，首次启动会自我提权重启一次）。
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root 'dist-portable\窗口置顶助手\窗口置顶助手.exe'
$dataDir = Join-Path $root 'dist-portable\窗口置顶助手\data'
$log = Join-Path $dataDir 'topmost-pin.log'
$name = '窗口置顶助手'

function Count-App { @(Get-Process -Name $name -ErrorAction SilentlyContinue).Count }

Write-Host '=== 彻底清理（含数据库，模拟全新安装）==='
Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2
Write-Host ("启动前进程数 = " + (Count-App))
Remove-Item $log -Force -ErrorAction SilentlyContinue
Get-ChildItem (Join-Path $dataDir 'data.db*') -ErrorAction SilentlyContinue |
    Remove-Item -Force -ErrorAction SilentlyContinue

try {
    Write-Host '=== 第一次启动（全新安装：数据库里没有设置，应取默认值=管理员）==='
    $p1 = Start-Process -FilePath $exe -PassThru
    Start-Sleep -Seconds 10
    $alive = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    Write-Host ("进程数 = {0}   最初那个进程 HasExited = {1}" -f $alive.Count, $p1.HasExited)

    Write-Host '=== 第二次启动（应被单实例挡下并立刻退出）==='
    $p2 = Start-Process -FilePath $exe -PassThru
    Start-Sleep -Seconds 5
    $alive = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    Write-Host ("进程数 = {0}   第二次的进程 HasExited = {1}" -f $alive.Count, $p2.HasExited)
    Write-Host ("存活 pid = " + (($alive | ForEach-Object { $_.Id }) -join ','))

    Write-Host '=== 应用日志 ==='
    if (Test-Path $log) { Get-Content $log } else { Write-Host '(无日志)' }
} finally {
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 1
    Write-Host ("收尾后进程数 = " + (Count-App))
}
