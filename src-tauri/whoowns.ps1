Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class WinPt {
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int c);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int c);
}
"@
$p = New-Object WinPt+POINT
$p.X = 2450; $p.Y = 1398
$h = [WinPt]::WindowFromPoint($p)
$procId = 0
[void][WinPt]::GetWindowThreadProcessId($h, [ref]$procId)
$sb = [System.Text.StringBuilder]::new(128)
[void][WinPt]::GetClassName($h, $sb, 128)
$t = [System.Text.StringBuilder]::new(128)
[void][WinPt]::GetWindowText($h, $t, 128)
$owner = (Get-Process -Id $procId -ErrorAction SilentlyContinue).ProcessName
Write-Host "window at point: class='$($sb.ToString())' title='$($t.ToString())' owner=$owner pid=$procId"
$p2 = New-Object WinPt+POINT
$p2.X = 2790; $p2.Y = 1398
$h2 = [WinPt]::WindowFromPoint($p2)
$procId2 = 0
[void][WinPt]::GetWindowThreadProcessId($h2, [ref]$procId2)
$sb2 = [System.Text.StringBuilder]::new(128)
[void][WinPt]::GetClassName($h2, $sb2, 128)
$owner2 = (Get-Process -Id $procId2 -ErrorAction SilentlyContinue).ProcessName
Write-Host "window at cog: class='$($sb2.ToString())' owner=$owner2 pid=$procId2"