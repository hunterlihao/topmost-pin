# 单实例行为的干净环境诊断。
# 必须以子进程方式运行（& pwsh -NoProfile -File ...）：
# harness 自己直接派生 GUI 进程时，工具调用会一直等下去。
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root 'dist-portable\窗口置顶助手\窗口置顶助手.exe'
$log = Join-Path $root 'dist-portable\窗口置顶助手\data\topmost-pin.log'
$name = '窗口置顶助手'

function Count-App { @(Get-Process -Name $name -ErrorAction SilentlyContinue).Count }

Write-Host '=== 彻底清理 ==='
Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2
Write-Host ("启动前进程数 = " + (Count-App))
Remove-Item $log -Force -ErrorAction SilentlyContinue

try {
    Write-Host '=== 第一次启动 ==='
    $p1 = Start-Process -FilePath $exe -PassThru
    Start-Sleep -Seconds 8
    Write-Host ("进程数 = {0}   p1.HasExited = {1}   p1.Id = {2}" -f (Count-App), $p1.HasExited, $p1.Id)

    Write-Host '=== 第二次启动（应被单实例挡下并立刻退出）==='
    $p2 = Start-Process -FilePath $exe -PassThru
    Start-Sleep -Seconds 5
    $alive = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    Write-Host ("进程数 = {0}" -f $alive.Count)
    Write-Host ("p2.HasExited = {0}   存活 pid = {1}" -f $p2.HasExited, (($alive | ForEach-Object { $_.Id }) -join ','))

    Write-Host '=== 应用日志 ==='
    if (Test-Path $log) { Get-Content $log } else { Write-Host '(无日志)' }
} finally {
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 1
    Write-Host ("收尾后进程数 = " + (Count-App))
}
