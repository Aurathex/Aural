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
$target = Join-Path $root "target\$BuildProfile"
$cargoArgs = @('build')
if ($BuildProfile -eq 'release') { $cargoArgs += '--release' }

# The workers use mutually exclusive engine features, so build them one at a time.
foreach ($worker in 'aural-stt-onnx', 'aural-stt-ggml') {
    Write-Host "Building $worker ($BuildProfile)..."
    & cargo @cargoArgs -p $worker --manifest-path (Join-Path $root 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "cargo build -p $worker failed" }
}

New-Item -ItemType Directory -Force $bin, $runtime | Out-Null
foreach ($worker in 'aural-stt-onnx', 'aural-stt-ggml') {
    Copy-Item (Join-Path $target "$worker.exe") (Join-Path $bin "$worker-$triple.exe") -Force
}

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
