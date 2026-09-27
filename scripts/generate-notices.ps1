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
- **ONNX Runtime 1.24.2** — © Microsoft Corporation, MIT License (text below);
  statically linked into `aural-stt-onnx.exe`. ONNX Runtime's own third-party notices
  are installed next to Aural as `licenses\onnxruntime-ThirdPartyNotices.txt`.
- **transcribe-rs 0.3.11** — MIT License (listed with the Rust crates below); compiled into
  `aural-stt-onnx.exe` with a small Aural change (choosing the DirectML graphics device),
  described in `third_party/transcribe-rs/AURAL-PATCH.md` in the source.
- **whisper.cpp / ggml** — © The ggml authors, MIT License (text below); compiled into
  `aural-stt-ggml.exe`.
- **LibriSpeech test-clean** (20 short recordings in `eval\`, used by the Hardware Test to
  check accuracy and speed) — by Vassil Panayotov, Guoguo Chen, Daniel Povey and Sanjeev
  Khudanpur, from LibriVox audiobooks — https://www.openslr.org/12 — licensed CC BY 4.0
  (https://creativecommons.org/licenses/by/4.0/). Re-encoded to 16 kHz mono FLAC; the
  audio is otherwise unmodified.
- **Mozilla CA certificate list** (via `webpki-roots`) — CDLA-Permissive-2.0; used to
  verify HTTPS model downloads.

## Installed separately

- **Microsoft Edge WebView2 Runtime** — installed or updated by Microsoft's own
  bootstrapper if missing; not redistributed by Aural.

## Speech models (downloaded on request, never bundled)

@@MODELS@@

## JavaScript packages compiled into the user interface

'@
    # One line per catalog model, from the attribution the catalog carries.
    $catalog = Get-Content (Join-Path $root 'manifests\catalog.v2.json') -Raw | ConvertFrom-Json
    $models = ($catalog.models | ForEach-Object { "- **$($_.name)** — $($_.license.attribution) License: $($_.license.url)" }) -join "`n"
    $header = $header.Replace('@@MODELS@@', $models)
    $native = foreach ($pair in @(@('ONNX Runtime', 'licenses\onnxruntime\LICENSE'), @('whisper.cpp / ggml', 'licenses\ggml\LICENSE'))) {
        $text = (Get-Content (Join-Path $root $pair[1]) -Raw).Trim()
        "### $($pair[0])`n`n``````text`n$text`n```````n"
    }
    $body = $header.Replace('## Installed separately', "## Native library licenses`n`n" + ($native -join "`n") + "`n## Installed separately") +
        ($npm -join "`n") + "`n" + (Get-Content $rust -Raw)
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $root 'THIRD_PARTY_NOTICES.md'), $body.Replace("`r`n", "`n"), $utf8)
    Write-Output "Wrote THIRD_PARTY_NOTICES.md"
}
finally {
    Pop-Location
}
