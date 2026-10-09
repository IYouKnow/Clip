<#
.SYNOPSIS
    Downloads an MSVC-built FFmpeg shared distribution and unpacks it for linking.

.DESCRIPTION
    trace links FFmpeg as libraries (not a sidecar). ffmpeg-sys-next needs
    MSVC import libraries (.lib) and headers, which the System233/ffmpeg-msvc-prebuilt
    project publishes. This script pins one version so builds are reproducible and
    installs it to third_party/ffmpeg as { bin, include, lib }.

    The LGPL variant is used deliberately: it keeps the app free of GPL code
    (h264_mf from Media Foundation is the software fallback instead of libx264).

    After running, the build finds FFmpeg through FFMPEG_PREFIX in .cargo/config.toml.

.PARAMETER Version
    FFmpeg release to fetch. Defaults to the pinned version.

.PARAMETER Variant
    Archive variant. Defaults to the x64 MSVC shared LGPL build.
#>
[CmdletBinding()]
param(
    [string]$Version = "9.0.2",
    [string]$Variant = "x64-windows-shared-lgpl"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $repoRoot "third_party\ffmpeg"
$asset = "ffmpeg-${Version}_$Variant.zip"
$url = "https://github.com/System233/ffmpeg-msvc-prebuilt/releases/download/ffmpeg-$Version/$asset"

# The bundler ships these DLLs next to the executable (bundle.resources in
# tauri.conf.json), so mirror the FFmpeg bin directory there.
$bundleDir = Join-Path $repoRoot "src-tauri\resources\ffmpeg"

function Sync-BundleResources {
    New-Item -ItemType Directory -Force -Path $bundleDir | Out-Null
    Copy-Item -Force -Path (Join-Path $dest "bin\*.dll") -Destination $bundleDir
}

if (Test-Path (Join-Path $dest "include\libavcodec\avcodec.h")) {
    Write-Host "FFmpeg $Version already present at $dest"
    Sync-BundleResources
    exit 0
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("trace-ffmpeg-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

try {
    $zip = Join-Path $tmp $asset
    Write-Host "Downloading $asset ..."
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing

    Write-Host "Extracting ..."
    $extracted = Join-Path $tmp "extracted"
    Expand-Archive -Path $zip -DestinationPath $extracted -Force

    # Some archives wrap everything in a single top-level directory; this build
    # publishes bin/ include/ lib/ directly at the archive root, so detect both.
    $source = $extracted
    if (-not (Test-Path (Join-Path $source "include\libavcodec\avcodec.h"))) {
        $inner = Get-ChildItem -Path $extracted -Directory | Select-Object -First 1
        if ($inner) { $source = $inner.FullName }
    }
    if (-not (Test-Path (Join-Path $source "include\libavcodec\avcodec.h"))) {
        throw "unexpected archive layout: include/libavcodec/avcodec.h not found"
    }

    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    New-Item -ItemType Directory -Force -Path $dest | Out-Null

    foreach ($part in @("bin", "include", "lib")) {
        $from = Join-Path $source $part
        if (Test-Path $from) {
            Copy-Item -Recurse -Force -Path $from -Destination (Join-Path $dest $part)
        }
    }

    Write-Host "FFmpeg $Version installed to $dest"
    Sync-BundleResources
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
