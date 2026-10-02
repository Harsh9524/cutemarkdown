param(
    [string]$Path,
    [switch]$NoOpen   # just build the page and print its path (for testing)
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$page = [IO.File]::ReadAllText((Join-Path $here 'mdview.html'), [Text.Encoding]::UTF8)
$target = Join-Path $here 'mdview.html'

if ($Path -and (Test-Path -LiteralPath $Path -PathType Leaf)) {
    $full  = (Resolve-Path -LiteralPath $Path).ProviderPath
    $b64   = [Convert]::ToBase64String([IO.File]::ReadAllBytes($full))
    $name  = [Net.WebUtility]::HtmlEncode([IO.Path]::GetFileName($full))
    $dir   = ([Uri]((Split-Path -Parent $full).TrimEnd('\') + '\')).AbsoluteUri
    $page  = $page.Replace('<!--BASE-->', '<base href="' + $dir + '">')
    $page  = $page.Replace('<!--BOOT-->', '<script id="boot" type="text/plain" data-name="' + $name + '">' + $b64 + '</script>')

    $tmp = Join-Path $env:TEMP 'MDView'
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    Get-ChildItem $tmp -Filter '*.html' -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -lt (Get-Date).AddDays(-1) } | Remove-Item -Force -ErrorAction SilentlyContinue
    $target = Join-Path $tmp ([guid]::NewGuid().ToString('N').Substring(0, 8) + '.html')
    [IO.File]::WriteAllText($target, $page, (New-Object Text.UTF8Encoding($false)))
}

if ($NoOpen) { $target; return }

$url = ([Uri]$target).AbsoluteUri
$args = @("--app=$url", '--window-size=1100,860')
foreach ($b in 'msedge', 'chrome', 'brave', 'vivaldi') {
    try { Start-Process $b -ArgumentList $args -ErrorAction Stop; return } catch { }
}
Start-Process $target   # last resort: default browser
