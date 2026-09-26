<#
.SYNOPSIS
  Regenerates THIRD_PARTY_NOTICES.md: bundled Microsoft components, speech models,
  the npm packages compiled into the UI, and every Rust crate (via cargo-about).
  Run after changing dependencies; needs `cargo install cargo-about --features cli`.
#>
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
Push-Location $root
try {
    $rust = Join-Path $root 'target\rust-notices.md'
    & cargo about generate --all-features --workspace about.hbs -o $rust
    if ($LASTEXITCODE -ne 0) { throw 'cargo about failed' }

    $npm = foreach ($pkg in '@tauri-apps/api', 'svelte') {
        $dir = Join-Path $root "app\node_modules\$pkg"
        $meta = Get-Content (Join-Path $dir 'package.json') -Raw | ConvertFrom-Json
        $licenseFile = Get-ChildItem $dir -File | Where-Object { $_.Name -match '^LICEN[CS]E' } | Select-Object -First 1
        $text = if ($licenseFile) { Get-Content $licenseFile.FullName -Raw } else { "License: $($meta.license)" }
        "### $pkg $($meta.version) ($($meta.license))`n`n``````text`n$($text.Trim())`n```````n"
    }

    $header = @'
# Third-party notices

Aural includes or downloads the components below. Each keeps its own license; Aural's
own license (LICENSE.md) does not change their terms.

## Bundled with the installer

- **DirectML** (`DirectML.dll`) — Microsoft DirectML Redistributable, © Microsoft
  Corporation. Required by ONNX Runtime; redistributed unmodified under Microsoft's
  redistribution terms, only as part of Aural.
- **Microsoft Visual C++ runtime** (`msvcp140.dll`, `msvcp140_1.dll`,
  `vcruntime140.dll`, `vcruntime140_1.dll`) — © Microsoft Corporation; redistributable
  files from Visual Studio, shipped unmodified next to Aural's programs.
- **ONNX Runtime** — © Microsoft Corporation, MIT License; statically linked into
  `aural-stt-onnx.exe` (license text under Rust crates: `ort-sys`).
- **whisper.cpp / ggml** — © The ggml authors, MIT License; compiled into
  `aural-stt-ggml.exe` (license text under Rust crates: `whisper-rs-sys`).
- **Mozilla CA certificate list** (via `webpki-roots`) — CDLA-Permissive-2.0; used to
  verify HTTPS model downloads.

## Installed separately

- **Microsoft Edge WebView2 Runtime** — installed or updated by Microsoft's own
  bootstrapper if missing; not redistributed by Aural.

## Speech models (downloaded on request, never bundled)

- **Parakeet TDT 0.6B v2** by NVIDIA — https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2 —
  licensed CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Aural downloads the
  ONNX conversion with int8 quantization by istupakov
  (https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx); the weights were
  modified from the original by that conversion.
- **Whisper small.en and base.en** by OpenAI — MIT License — in ggml format from the
  whisper.cpp project (https://huggingface.co/ggerganov/whisper.cpp), 8-bit quantized.

## JavaScript packages compiled into the user interface

'@
    $body = $header + ($npm -join "`n") + "`n" + (Get-Content $rust -Raw)
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $root 'THIRD_PARTY_NOTICES.md'), $body.Replace("`r`n", "`n"), $utf8)
    Write-Output "Wrote THIRD_PARTY_NOTICES.md"
}
finally {
    Pop-Location
}
