Add-Type -AssemblyName System.Drawing
$out = "$env:TEMP\opencode"
$sig = @"
using System;
using System.Runtime.InteropServices;
public class Win32Cap {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr l);
  public delegate bool EnumWindowsProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
Add-Type -TypeDefinition $sig
$found = [System.Collections.Generic.List[IntPtr]]::new()
$cb = [Win32Cap+EnumWindowsProc]{
  param($h, $l)
  $sb = [System.Text.StringBuilder]::new(64)
  [void][Win32Cap]::GetClassName($h, $sb, 64)
  if ($sb.ToString() -eq "LiquidGlassRenderer") { $found.Add($h) }
  return $true
}
[void][Win32Cap]::EnumWindows($cb, [IntPtr]::Zero)
$i = 0
foreach ($h in $found) {
  $r = New-Object Win32Cap+RECT
  [void][Win32Cap]::GetWindowRect($h, [ref]$r)
  $w = [Math]::Max(1, $r.R - $r.L); $ht = [Math]::Max(1, $r.B - $r.T)
  $bmp = New-Object System.Drawing.Bitmap($w, $ht)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $dc = $g.GetHdc()
  [void][Win32Cap]::PrintWindow($h, $dc, 2)
  $g.ReleaseHdc($dc); $g.Dispose()
  $bmp.Save("$out\surface-$i-$($r.L)x$($r.T).png")
  $bmp.Dispose()
  Write-Host "saved surface-$i rect=$($r.L),$($r.T),$($r.R),$($r.B)"
  $i++
}