# Benchmark every catalog variant (model + backend) on one corpus and collect the rows.
#
#   ./tools/bench/scripts/run-matrix.ps1 -Corpus $env:USERPROFILE\aural-bench-corpus\librispeech-100\corpus.tsv `
#       -Out $env:USERPROFILE\aural-bench-corpus\results-v02 -Backends cpu,directml,vulkan -TargetDir C:\Claude\agt
#
# Variants come from manifests/catalog.v2.json (probe models included: their numbers
# calibrate the hardware test). Models are read from -Models: a folder per model id, or,
# for ggml files, the flat `whisper\` folder the bench README uses. Runs are sequential
# so they don't compete for the processor or graphics card. Vulkan needs the Vulkan SDK
# at build time. Each run writes <variant>.json; table.md collects the rows.
param(
    [Parameter(Mandatory)][string]$Corpus,
    [Parameter(Mandatory)][string]$Out,
    [string[]]$Backends = @('cpu'),
    [string]$Models = "$env:LOCALAPPDATA\Aural\models",
    [int]$Threads = 8,
    # Only variants whose id matches this pattern (e.g. 'parakeet').
    [string]$Only = '.',
    # The Vulkan build nests CMake projects deep enough to pass MSVC's 260-character
    # path limit under a long checkout path; build GPU runs in a short folder.
    [string]$TargetDir
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..\..')
# pwsh -File passes 'cpu,vulkan' as one string; accept both forms.
$Backends = $Backends -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ }
if ($bad = $Backends | Where-Object { $_ -notin 'cpu', 'vulkan', 'cuda', 'directml' }) { throw "unknown backend: $bad" }
New-Item -ItemType Directory -Force $Out | Out-Null
$table = Join-Path $Out 'table.md'
$b = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue
$power = if (-not $b) { 'desktop' } elseif ($b.BatteryStatus -eq 2) { 'AC' } else { "battery ($($b.EstimatedChargeRemaining)%)" }
$gpu = (nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>$null) -join '; '
@(
    "Corpus: $Corpus",
    "Date: $(Get-Date -Format s)",
    "CPU: $((Get-CimInstance Win32_Processor).Name)",
    "GPU: $gpu",
    "Power: $power",
    '',
    '| variant | backend used | threads | clips | WER | p50 ms | p95 ms | mean RTF | load ms | RAM MB | GPU MB |',
    '|---|---|---|---|---|---|---|---|---|---|---|'
) | Set-Content $table

$catalog = Get-Content (Join-Path $root 'manifests\catalog.v2.json') -Raw | ConvertFrom-Json
$failed = 0
foreach ($m in $catalog.models) {
    foreach ($v in $m.variants) {
        $backend = $v.backend
        $variant = "$($m.id)@$backend"
        if ($backend -notin $Backends -or $variant -notmatch $Only) { continue }
        $modelPath = if ($m.runtime -eq 'onnx') {
            Join-Path $Models $m.id
        } else {
            $file = $m.files[0].name
            @((Join-Path $Models "$($m.id)\$file"), (Join-Path $Models "whisper\$file")) | Where-Object { Test-Path $_ } | Select-Object -First 1
        }
        if (-not $modelPath -or -not (Test-Path $modelPath)) { "skip $variant`: model files not found"; continue }
        $features = if ($m.runtime -eq 'onnx') { 'onnx' } elseif ($backend -eq 'cpu') { 'ggml' } else { "ggml,$backend" }
        # GPU ggml builds go to the short folder; everything else uses the default target.
        $env:CARGO_TARGET_DIR = if ($TargetDir -and $m.runtime -eq 'ggml') { $TargetDir } else { $null }
        "== $variant start $(Get-Date -Format 'yyyy/MM/dd HH:mm:ss')"
        $json = Join-Path $Out "$variant.json"
        $cargoArgs = @('run', '--release', '-q', '-p', 'aural-bench', '--manifest-path', (Join-Path $root 'Cargo.toml'),
            '--features', $features, '--', '--engine', $m.engine, '--model', $modelPath, '--backend', $backend,
            '--corpus', $Corpus, '--out', $json)
        if ($m.engine -eq 'whisper') { $cargoArgs += @('--threads', "$Threads") }
        $rows = & cargo @cargoArgs 2>&1 | ForEach-Object { "$_" }
        if ($LASTEXITCODE -ne 0) { $failed++; "FAILED ($LASTEXITCODE):"; $rows | Select-Object -Last 5; continue }
        $r = Get-Content $json -Raw | ConvertFrom-Json
        $s = $r.summary
        $row = '| {0} | {1} | {2} | {3} | {4:N2}% | {5:N0} | {6:N0} | {7:N3} | {8:N0} | {9:N0} | {10} |' -f `
            $variant, $r.backend_used, $r.threads, $s.clips, ($s.corpus_wer * 100), $s.p50_ms, $s.p95_ms, $s.mean_rtf,
            $r.load_ms, $r.engine_ram_mb, $(if ($null -ne $r.gpu_memory_mb) { $r.gpu_memory_mb } else { 'n/a' })
        $row
        "== $variant end $(Get-Date -Format 'yyyy/MM/dd HH:mm:ss')"
        Add-Content $table $row
    }
}
"results: $table"
exit $failed
