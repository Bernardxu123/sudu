param(
    [string]$Exe = "target\release\sudu.exe",
    [string]$Doc = "",
    [int]$Runs = 5
)

$exePath = (Resolve-Path -LiteralPath $Exe).Path
$rows = @()
for ($i = 1; $i -le $Runs; $i++) {
    $probe = Join-Path $env:TEMP ("sudu-probe-" + $i + ".txt")
    Remove-Item -LiteralPath $probe -ErrorAction SilentlyContinue
    $env:SUDU_PROBE = $probe
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    if ($Doc -ne "") {
        $p = Start-Process -FilePath $exePath -ArgumentList @($Doc) -PassThru
    } else {
        $p = Start-Process -FilePath $exePath -PassThru
    }
    while ($sw.ElapsedMilliseconds -lt 10000) {
        $p.Refresh()
        if ($p.MainWindowHandle -ne 0) { break }
        Start-Sleep -Milliseconds 20
    }
    $windowMs = $sw.ElapsedMilliseconds
    Start-Sleep -Seconds 2
    $p.Refresh()
    $rssMb = [math]::Round($p.WorkingSet64 / 1MB, 1)
    $probeMs = ""
    if (Test-Path -LiteralPath $probe) {
        $probeMs = (Get-Content -LiteralPath $probe -Raw).Trim()
    }
    $rows += [pscustomobject]@{
        run = $i
        window_ms = $windowMs
        probe_ms = $probeMs
        rss_mb = $rssMb
    }
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $probe -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
}
$rows | Format-Table -AutoSize
$median = ($rows.window_ms | Sort-Object)[[int]([math]::Floor($rows.Count / 2))]
$medianProbe = ($rows.probe_ms | Where-Object { $_ -ne "" } | Sort-Object)
if ($medianProbe.Count -gt 0) {
    $medianProbe = $medianProbe[[int]([math]::Floor($medianProbe.Count / 2))]
}
$rss = ($rows.rss_mb | Sort-Object)[[int]([math]::Floor($rows.Count / 2))]
Write-Output ("median_window_ms=" + $median + " median_probe_ms=" + $medianProbe + " median_rss_mb=" + $rss)
