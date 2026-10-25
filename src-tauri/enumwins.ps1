Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WinEnum {
  public delegate bool EnumWindowsProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$targets = (Get-Process glassbar -ErrorAction SilentlyContinue).Id
$cb = [WinEnum+EnumWindowsProc]{
  param($h, $l)
  $procId = 0
  [void][WinEnum]::GetWindowThreadProcessId($h, [ref]$procId)
  if ($targets -contains $procId) {
    $sb = [System.Text.StringBuilder]::new(128)
    [void][WinEnum]::GetClassName($h, $sb, 128)
    $r = New-Object WinEnum+RECT
    [void][WinEnum]::GetWindowRect($h, [ref]$r)
    $ex = [WinEnum]::GetWindowLong($h, -20)
    $vis = [WinEnum]::IsWindowVisible($h)
    Write-Host ("hwnd=0x{0:X} class='{1}' rect=({2},{3})-({4},{5}) visible={6} exstyle=0x{7:X} transparent={8}" -f $h.ToInt64(), $sb.ToString(), $r.L, $r.T, $r.R, $r.B, $vis, $ex, (($ex -band 0x20) -ne 0))
  }
  return $true
}
[void][WinEnum]::EnumWindows($cb, [IntPtr]::Zero)