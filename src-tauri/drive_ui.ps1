Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class SendInput2 {
  [StructLayout(LayoutKind.Sequential)]
  public struct INPUT { public uint type; public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)]
  public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  public const uint LEFTDOWN = 0x0002, LEFTUP = 0x0004, RIGHTDOWN = 0x0008, RIGHTUP = 0x0010;
  public static void Click(uint down, uint up) {
    var i = new INPUT[2];
    i[0].type = 0; i[0].mi.dwFlags = down;
    i[1].type = 0; i[1].mi.dwFlags = up;
    SendInput(2, i, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
function Click-At([int]$x, [int]$y, [bool]$right) {
  [void][SendInput2]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 120
  if ($right) { [SendInput2]::Click([SendInput2]::RIGHTDOWN, [SendInput2]::RIGHTUP) }
  else { [SendInput2]::Click([SendInput2]::LEFTDOWN, [SendInput2]::LEFTUP) }
  Start-Sleep -Milliseconds 400
}
# right-click an app button near the left-center of the pill
Click-At 2450 1398 $true
# open the settings dropdown via the cog at the pill's right end
Click-At 2790 1398 $false
# click empty space to close everything
Click-At 1200 700 $false
"input sequence done"