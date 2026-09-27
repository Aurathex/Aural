<#
.SYNOPSIS
  Prints the catalog `files` array for a Hugging Face model at a pinned commit: name,
  immutable URL, SHA-256 and size for each requested file.

.DESCRIPTION
  Large files (Git LFS) take their SHA-256 and size from the Hub's tree API. Small files
  stored in git have no published SHA-256, so they are downloaded and hashed here; the
  script fails if one can't be. Each -File is "<path in repo>" or
  "<path in repo>=<local name>" (to rename, e.g. onnx/encoder_model_int8.onnx=encoder_model.int8.onnx).

    ./scripts/catalog-entry.ps1 -Repo onnx-community/moonshine-tiny-ONNX `
        -Revision a6da1241cd305dcd64eab1edbd615f2bb9aabb95 `
        -File config.json, tokenizer.json, onnx/encoder_model_int8.onnx=encoder_model.int8.onnx
#>
param(
    [Parameter(Mandatory)][string]$Repo,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$Revision,
    [Parameter(Mandatory)][string[]]$File
)
$ErrorActionPreference = 'Stop'
$tree = Invoke-RestMethod -MaximumRetryCount 5 -RetryIntervalSec 5 "https://huggingface.co/api/models/$Repo/tree/$Revision`?recursive=true"
$out = foreach ($spec in $File) {
    $path, $name = $spec -split '=', 2
    if (-not $name) { $name = Split-Path $path -Leaf }
    $entry = $tree | Where-Object { $_.type -eq 'file' -and $_.path -eq $path }
    if (-not $entry) { throw "$path not found in $Repo@$Revision" }
    $url = "https://huggingface.co/$Repo/resolve/$Revision/$path"
    if ($entry.lfs) {
        $sha = $entry.lfs.oid
        $size = [int64]$entry.lfs.size
    } else {
        $tmp = New-TemporaryFile
        try {
            Invoke-WebRequest $url -OutFile $tmp -UseBasicParsing -MaximumRetryCount 5 -RetryIntervalSec 5
            $size = (Get-Item $tmp).Length
            if ($size -ne [int64]$entry.size) { throw "$path downloaded $size bytes, the Hub says $($entry.size)" }
            $sha = (Get-FileHash $tmp -Algorithm SHA256).Hash.ToLowerInvariant()
        } finally {
            Remove-Item -LiteralPath $tmp -Force
        }
    }
    [ordered]@{ name = $name; url = $url; sha256 = $sha; size = $size }
}
ConvertTo-Json @($out) -Depth 3
