<#
.SYNOPSIS
    Fetches libclang.dll into third_party/libclang so bindgen can run.

.DESCRIPTION
    ffmpeg-sys-next runs bindgen at build time, which needs libclang. Installing
    the full LLVM toolchain is a large, machine-wide change, so instead this
    pulls the `libclang` PyPI wheel (which bundles libclang.dll) and extracts
    just the DLL into the project. .cargo/config.toml points LIBCLANG_PATH here.

.PARAMETER Version
    libclang package version. Empty means "latest" from PyPI.
#>
[CmdletBinding()]
param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $repoRoot "third_party\libclang"
$dll = Join-Path $dest "libclang.dll"

if (Test-Path $dll) {
    Write-Host "libclang already present at $dll"
    exit 0
}

$indexUrl = if ($Version) {
    "https://pypi.org/pypi/libclang/$Version/json"
} else {
    "https://pypi.org/pypi/libclang/json"
}

Write-Host "Resolving libclang wheel ..."
$meta = Invoke-RestMethod -Uri $indexUrl -UseBasicParsing
$wheel = $meta.urls | Where-Object { $_.filename -like "*win_amd64.whl" } | Select-Object -First 1
if (-not $wheel) {
    throw "no win_amd64 wheel found for libclang (version '$Version')"
}
Write-Host ("Using " + $wheel.filename)

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("trace-libclang-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

try {
    $whl = Join-Path $tmp $wheel.filename
    Invoke-WebRequest -Uri $wheel.url -OutFile $whl -UseBasicParsing

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($whl)
    try {
        $entry = $zip.Entries | Where-Object { $_.FullName -like "*/libclang.dll" } | Select-Object -First 1
        if (-not $entry) { throw "libclang.dll not found inside the wheel" }

        New-Item -ItemType Directory -Force -Path $dest | Out-Null
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $dll, $true)
    }
    finally {
        $zip.Dispose()
    }

    Write-Host "libclang installed to $dll"
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
