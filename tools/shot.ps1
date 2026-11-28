param(
    [string]$Out = "shot.png",
    [string]$ProcessName = "sudu"
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class SuduWin {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int cmd);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

    public static IntPtr FindLargestProcessWindow(int targetPid) {
        IntPtr best = IntPtr.Zero;
        long bestArea = 0;
        EnumWindows((hWnd, lParam) => {
            uint pid = 0;
            GetWindowThreadProcessId(hWnd, out pid);
            if ((int)pid != targetPid) return true;
            if (!IsWindowVisible(hWnd)) return true;
            RECT r;
            if (!GetWindowRect(hWnd, out r)) return true;
            long w = r.Right - r.Left;
            long h = r.Bottom - r.Top;
            if (w <= 80 || h <= 80) return true;
            long area = w * h;
            if (area > bestArea) {
                bestArea = area;
                best = hWnd;
            }
            return true;
        }, IntPtr.Zero);
        return best;
    }
}
"@

[SuduWin]::SetProcessDPIAware() | Out-Null
$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $procs) {
    throw "process not found: $ProcessName"
}

$hwnd = [IntPtr]::Zero
foreach ($p in $procs) {
    $cand = [SuduWin]::FindLargestProcessWindow($p.Id)
    if ($cand -ne [IntPtr]::Zero) {
        $hwnd = $cand
        break
    }
}
if ($hwnd -eq [IntPtr]::Zero) {
    $fallbackProc = $procs | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if ($fallbackProc) { $hwnd = $fallbackProc.MainWindowHandle }
}
if ($hwnd -eq [IntPtr]::Zero) {
    throw "visible window not found for: $ProcessName"
}

[SuduWin]::ShowWindow($hwnd, 9) | Out-Null
[SuduWin]::SetForegroundWindow($hwnd) | Out-Null
[SuduWin]::SetWindowPos($hwnd, [IntPtr](-1), 0, 0, 0, 0, 0x0001 -bor 0x0002) | Out-Null
Start-Sleep -Milliseconds 600

$rect = New-Object SuduWin+RECT
[SuduWin]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$width = [Math]::Max(1, $rect.Right - $rect.Left)
$height = [Math]::Max(1, $rect.Bottom - $rect.Top)

$bmp = New-Object System.Drawing.Bitmap($width, $height)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$copied = $false
try {
    $gfx.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bmp.Size)
    $copied = $true
} catch {
    $hdc = $gfx.GetHdc()
    try {
        [SuduWin]::PrintWindow($hwnd, $hdc, 2) | Out-Null
    } finally {
        $gfx.ReleaseHdc($hdc)
    }
}
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$gfx.Dispose()
$bmp.Dispose()
[SuduWin]::SetWindowPos($hwnd, [IntPtr](-2), 0, 0, 0, 0, 0x0001 -bor 0x0002) | Out-Null
Write-Output ("saved " + $Out + " " + $width + "x" + $height + " method=" + $(if ($copied) { "CopyFromScreen" } else { "PrintWindow" }))
