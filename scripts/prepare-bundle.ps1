<#
.SYNOPSIS
  Builds the speech workers and stages everything the Tauri bundle needs next to
  aural.exe: the two worker sidecars (with Tauri's target-triple suffix), DirectML
  (required by ONNX Runtime) and the app-local MSVC runtime DLLs.

.NOTES
  Staged files are build output and are git-ignored. Run from any directory.
  Needs CMake and LLVM (LIBCLANG_PATH) for the ggml worker; see docs/building-windows.md.
#>
[CmdletBinding()]
param(
    [ValidateSet('release', 'debug')]
    [string]$BuildProfile = 'release'
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$triple = 'x86_64-pc-windows-msvc'
$tauri = Join-Path $root 'app\src-tauri'
$bin = Join-Path $tauri 'binaries'
$runtime = Join-Path $tauri 'runtime'
$targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
$target = Join-Path $targetRoot $BuildProfile
$cargoArgs = @('build')
if ($BuildProfile -eq 'release') { $cargoArgs += '--release' }

# The workers use mutually exclusive engine features, so build them one at a time.
Write-Host "Building aural-stt-onnx ($BuildProfile)..."
& cargo @cargoArgs -p aural-stt-onnx --manifest-path (Join-Path $root 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'cargo build -p aural-stt-onnx failed' }

# The Whisper worker is built with Vulkan so it can use NVIDIA and AMD graphics cards
# (the Vulkan loader comes with the GPU driver; nothing extra ships). Building it needs
# the Vulkan SDK. whisper.cpp's shader build nests CMake projects deep enough to pass
# MSVC's 260-character path limit (error C1083) under a long checkout path, so it gets
# a short build folder when needed: AURAL_VK_TARGET_DIR, or <drive>\aural-vk-target.
if (-not $env:VULKAN_SDK) { $env:VULKAN_SDK = [Environment]::GetEnvironmentVariable('VULKAN_SDK', 'Machine') }
if (-not $env:VULKAN_SDK -or -not (Test-Path (Join-Path $env:VULKAN_SDK 'Bin\glslc.exe'))) {
    throw 'The Vulkan SDK is needed to build the Whisper worker (winget install KhronosGroup.VulkanSDK); see docs/building-windows.md'
}
$env:PATH = "$(Join-Path $env:VULKAN_SDK 'Bin');$env:PATH"
$ggmlTargetRoot = $targetRoot
if ($env:AURAL_VK_TARGET_DIR) {
    $ggmlTargetRoot = $env:AURAL_VK_TARGET_DIR
} elseif ($targetRoot.Length -gt 40) {
    $ggmlTargetRoot = Join-Path ([IO.Path]::GetPathRoot("$root")) 'aural-vk-target'
}
Write-Host "Building aural-stt-ggml with Vulkan ($BuildProfile) in $ggmlTargetRoot..."
$saved = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $ggmlTargetRoot
try {
    & cargo @cargoArgs -p aural-stt-ggml --features vulkan --manifest-path (Join-Path $root 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'cargo build -p aural-stt-ggml --features vulkan failed' }
} finally {
    $env:CARGO_TARGET_DIR = $saved
}

New-Item -ItemType Directory -Force $bin, $runtime | Out-Null
Copy-Item (Join-Path $target 'aural-stt-onnx.exe') (Join-Path $bin "aural-stt-onnx-$triple.exe") -Force
Copy-Item (Join-Path (Join-Path $ggmlTargetRoot $BuildProfile) 'aural-stt-ggml.exe') (Join-Path $bin "aural-stt-ggml-$triple.exe") -Force

# ONNX Runtime's prebuilt library imports DirectML.dll; ship the version it was built
# against rather than relying on the older copy in System32 on early Windows 10 builds.
Copy-Item (Join-Path $target 'DirectML.dll') (Join-Path $runtime 'DirectML.dll') -Force

# App-local MSVC runtime (Microsoft permits this for the redist DLLs), so a per-user,
# no-admin install works on PCs without the VC++ Redistributable.
$crt = Get-ChildItem "C:\Program Files*\Microsoft Visual Studio\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC14*.CRT" -Directory |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $crt) { throw 'MSVC redist folder not found (install the VS C++ build tools)' }
foreach ($dll in 'msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll', 'vcruntime140_1.dll') {
    Copy-Item (Join-Path $crt.FullName $dll) (Join-Path $runtime $dll) -Force
}
Write-Host "Staged sidecars in $bin and runtime DLLs in $runtime (from $($crt.FullName))."
