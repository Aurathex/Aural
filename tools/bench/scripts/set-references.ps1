# Copy benchmark results (run-matrix.ps1 output: one <variant>.json per variant) into the
# catalog's `reference` numbers, which the hardware test scales to estimate other PCs.
#
#   ./tools/bench/scripts/set-references.ps1 -Results $env:USERPROFILE\aural-bench-corpus\results-v02 `
#       -Clips $env:USERPROFILE\aural-bench-corpus\results-v02-eval
#
# -Clips (optional): the same variants run on the 20 built-in clips; their WER becomes
# `clips_wer`, the scale the hardware test's own measurements use.
#
# Variants without a result file keep their current reference.
param([Parameter(Mandatory)][string]$Results, [string]$Clips)
$ErrorActionPreference = 'Stop'
$path = Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')) 'manifests\catalog.v2.json'
$catalog = Get-Content $path -Raw | ConvertFrom-Json
$updated = 0
foreach ($m in $catalog.models) {
    foreach ($v in $m.variants) {
        $file = Join-Path $Results "$($m.id)@$($v.backend).json"
        if (-not (Test-Path $file)) { continue }
        $r = Get-Content $file -Raw | ConvertFrom-Json
        $v.reference = [ordered]@{
            wer     = [math]::Round($r.summary.corpus_wer, 4)
            p50_ms  = [int][math]::Round($r.summary.p50_ms)
            rtf     = [math]::Round($r.summary.mean_rtf, 3)
            load_ms = [int][math]::Round($r.load_ms)
            ram_mb  = [int][math]::Round($r.engine_ram_mb)
            vram_mb = if ($v.backend -eq 'cpu' -or $null -eq $r.gpu_memory_mb) { 0 } else { [int]$r.gpu_memory_mb }
        }
        $clipFile = if ($Clips) { Join-Path $Clips "$($m.id)@$($v.backend).json" }
        if ($clipFile -and (Test-Path $clipFile)) {
            $v.reference.clips_wer = [math]::Round((Get-Content $clipFile -Raw | ConvertFrom-Json).summary.corpus_wer, 4)
        }
        $updated++
    }
}
($catalog | ConvertTo-Json -Depth 8) + "`n" | Set-Content $path -NoNewline -Encoding utf8NoBOM
"updated $updated references in $path"
