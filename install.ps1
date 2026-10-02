param([switch]$Uninstall)
$ErrorActionPreference = 'Stop'
$src   = Split-Path -Parent $MyInvocation.MyCommand.Path
$dst   = Join-Path $env:LOCALAPPDATA 'MDView'
$prog  = 'MDView.Markdown'
$exts  = '.md', '.markdown', '.mdown', '.mkd'
$lnk   = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\MD Viewer.lnk'
$hkcu  = [Microsoft.Win32.Registry]::CurrentUser

function Notify { Add-Type -Namespace W -Name S -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e,int f,IntPtr a,IntPtr b);'; [W.S]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero) }

if ($Uninstall) {
    foreach ($e in $exts) {
        $k = $hkcu.OpenSubKey("Software\Classes\$e\OpenWithProgids", $true); if ($k) { $k.DeleteValue($prog, $false); $k.Close() }
        $hkcu.DeleteSubKeyTree("Software\Classes\SystemFileAssociations\$e\shell\MDView", $false)
    }
    $hkcu.DeleteSubKeyTree("Software\Classes\$prog", $false)
    Remove-Item $lnk -Force -ErrorAction SilentlyContinue
    Remove-Item $dst -Recurse -Force -ErrorAction SilentlyContinue
    Notify; Write-Host 'MD Viewer removed.'; return
}

# 1. copy app
New-Item -ItemType Directory -Force -Path $dst | Out-Null
'mdview.html', 'mdview.ps1', 'mdview.vbs' | ForEach-Object { Copy-Item (Join-Path $src $_) $dst -Force }

# 2. icon (drawn on the fly: pink→lavender rounded square with "M↓")
$ico = Join-Path $dst 'mdview.ico'
try {
    Add-Type -AssemblyName System.Drawing
    $bmp = New-Object Drawing.Bitmap 64, 64
    $g = [Drawing.Graphics]::FromImage($bmp); $g.SmoothingMode = 'AntiAlias'; $g.TextRenderingHint = 'AntiAliasGridFit'
    $path = New-Object Drawing.Drawing2D.GraphicsPath; $r = 18
    $path.AddArc(0, 0, $r, $r, 180, 90); $path.AddArc(63 - $r, 0, $r, $r, 270, 90)
    $path.AddArc(63 - $r, 63 - $r, $r, $r, 0, 90); $path.AddArc(0, 63 - $r, $r, $r, 90, 90); $path.CloseFigure()
    $br = New-Object Drawing.Drawing2D.LinearGradientBrush ((New-Object Drawing.Point 0, 0), (New-Object Drawing.Point 64, 64), [Drawing.Color]::FromArgb(224, 85, 138), [Drawing.Color]::FromArgb(124, 111, 240))
    $g.FillPath($br, $path)
    $f = New-Object Drawing.Font 'Consolas', 24, ([Drawing.FontStyle]::Bold), ([Drawing.GraphicsUnit]::Pixel)
    $sf = New-Object Drawing.StringFormat; $sf.Alignment = 'Center'; $sf.LineAlignment = 'Center'
    $g.DrawString([string]::Concat('M', [char]0x2193), $f, [Drawing.Brushes]::White, (New-Object Drawing.RectangleF 0, 0, 64, 66), $sf)
    $icon = [Drawing.Icon]::FromHandle($bmp.GetHicon())
    $fs = [IO.File]::Create($ico); $icon.Save($fs); $fs.Close(); $g.Dispose()
} catch { $ico = "$env:SystemRoot\System32\shell32.dll,70" }

# 3. file association (per-user, no admin needed)
$cmd = 'wscript.exe "' + (Join-Path $dst 'mdview.vbs') + '" "%1"'
$k = $hkcu.CreateSubKey("Software\Classes\$prog"); $k.SetValue('', 'Markdown Document'); $k.Close()
$k = $hkcu.CreateSubKey("Software\Classes\$prog\DefaultIcon"); $k.SetValue('', $ico); $k.Close()
$k = $hkcu.CreateSubKey("Software\Classes\$prog\shell\open"); $k.SetValue('', 'Open with MD Viewer'); $k.SetValue('Icon', $ico); $k.Close()
$k = $hkcu.CreateSubKey("Software\Classes\$prog\shell\open\command"); $k.SetValue('', $cmd); $k.Close()
foreach ($e in $exts) {
    $k = $hkcu.CreateSubKey("Software\Classes\$e\OpenWithProgids"); $k.SetValue($prog, [byte[]]@(), 'None'); $k.Close()
    $k = $hkcu.CreateSubKey("Software\Classes\SystemFileAssociations\$e\shell\MDView"); $k.SetValue('', 'Open in MD Viewer'); $k.SetValue('Icon', $ico); $k.Close()
    $k = $hkcu.CreateSubKey("Software\Classes\SystemFileAssociations\$e\shell\MDView\command"); $k.SetValue('', $cmd); $k.Close()
}

# 4. Start-menu shortcut (empty viewer: drop / open / paste)
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$s.TargetPath = "$env:SystemRoot\System32\wscript.exe"; $s.Arguments = '"' + (Join-Path $dst 'mdview.vbs') + '"'
$s.IconLocation = $ico; $s.Description = 'Markdown viewer'; $s.Save()

Notify
Write-Host ''
Write-Host 'MD Viewer installed.' -ForegroundColor Magenta
Write-Host '  - Start menu: "MD Viewer"'
Write-Host '  - Right-click any .md > Open with > MD Viewer > "Always"   (Windows only lets you pick the default yourself)'
Write-Host '  - Win11: right-click > Show more options > "Open in MD Viewer"'
