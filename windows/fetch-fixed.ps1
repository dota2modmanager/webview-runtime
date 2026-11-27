<#
.SYNOPSIS
  Takes Microsoft's WebView2 Fixed Version runtime (x64 cab), checks it, and prints its
  runtime.json entry.

.DESCRIPTION
  Microsoft publishes the Fixed Version runtime only as links on its download page
  (https://developer.microsoft.com/microsoft-edge/webview2/, "Fixed Version", x64). Copy the
  link of the version to ship and pass it here. The cab is not changed: our release carries the
  same bytes, and Microsoft's link stays in the entry as the second place to fetch them from.

  Checked before anything is printed: the cab unpacks into one folder named after the version,
  that folder has msedgewebview2.exe, and that exe (and every other exe and dll in the folder)
  carries a valid Authenticode signature from Microsoft.

.PARAMETER Url
  Microsoft's link to Microsoft.WebView2.FixedVersionRuntime.<version>.x64.cab.

.PARAMETER ReleaseUrl
  Where our own release will serve the same file. Goes first in the entry's urls.

.PARAMETER Out
  Folder for the cab and the entry (runtime-windows-x64.json).
#>
param(
  [Parameter(Mandatory)] [string] $Url,
  [string] $ReleaseUrl = "",
  [string] $Out = "dist"
)
$ErrorActionPreference = "Stop"

if ($Url -notmatch '^https://[^/]*\.microsoft\.com/.*/Microsoft\.WebView2\.FixedVersionRuntime\.(\d+\.\d+\.\d+\.\d+)\.x64\.cab$') {
  throw "not a Microsoft x64 Fixed Version cab link: $Url"
}
$version = $Matches[1]
$name = "Microsoft.WebView2.FixedVersionRuntime.$version.x64"
New-Item -ItemType Directory -Force $Out | Out-Null
$cab = Join-Path $Out "$name.cab"

Write-Host "downloading $version"
Invoke-WebRequest -Uri $Url -OutFile $cab -UseBasicParsing
$size = (Get-Item $cab).Length
$sha = (Get-FileHash -Algorithm SHA256 $cab).Hash.ToLowerInvariant()

$unpacked = Join-Path $Out "unpacked"
if (Test-Path $unpacked) { Remove-Item -Recurse -Force $unpacked }
New-Item -ItemType Directory $unpacked | Out-Null
& "$env:SystemRoot\System32\expand.exe" $cab "-F:*" $unpacked | Out-Null
if ($LASTEXITCODE -ne 0) { throw "expand.exe failed with $LASTEXITCODE" }

$engine = Join-Path $unpacked $name
if (-not (Test-Path (Join-Path $engine "msedgewebview2.exe"))) { throw "$name\msedgewebview2.exe is not in the cab" }

$bad = Get-ChildItem -Recurse -File $engine -Include *.exe, *.dll | ForEach-Object {
  $sig = Get-AuthenticodeSignature $_.FullName
  if ($sig.Status -ne "Valid" -or $sig.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') { $_.FullName }
}
if ($bad) { throw "not signed by Microsoft:`n$($bad -join "`n")" }

$urls = @()
if ($ReleaseUrl) { $urls += $ReleaseUrl }
$urls += $Url
$entry = [ordered]@{
  platform = "windows-x64"
  version  = $version
  archive  = "cab"
  size     = $size
  sha256   = $sha
  urls     = $urls
  root     = $name
  check    = "msedgewebview2.exe"
}
$json = $entry | ConvertTo-Json -Depth 3
[IO.File]::WriteAllText((Join-Path $Out "runtime-windows-x64.json"), $json)
Remove-Item -Recurse -Force $unpacked
Write-Output $json
