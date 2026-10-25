Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap(1020, 120)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(2300, 1330, 0, 0, (New-Object System.Drawing.Size(1020, 120)))
$g.Dispose()
$bmp.Save("$env:TEMP\opencode\bottom-strip.png")
$bmp.Dispose()
Write-Host "saved bottom-strip.png"