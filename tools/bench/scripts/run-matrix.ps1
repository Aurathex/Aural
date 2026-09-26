# Run the M0 benchmark matrix on one corpus and collect the table rows.
#
#   ./tools/bench/scripts/run-matrix.ps1 -Corpus $env:USERPROFILE\aural-bench-corpus\personal\corpus.tsv `
#       -Out $env:USERPROFILE\aural-bench-corpus\results-personal -Backends cpu,vulkan,cuda
#
# Models are read from -Models (default: %LOCALAPPDATA%\Aural\models, the layout used by
# the bench README). Runs are sequential so they don't compete for the CPU or GPU.
# Vulkan needs the Vulkan SDK and CUDA the CUDA Toolkit 12.x at build time.
param(
    [Parameter(Mandatory)][string]$Corpus,
    [Parameter(Mandatory)][string]$Out,
    [string[]]$Backends = @('cpu'),
    [string]$Models = "$env:LOCALAPPDATA\Aural\models",
    [int]$Threads = 8,
    # The Vulkan build nests CMake projects deep enough to pass MSVC's 260-character
    # path limit under a long checkout path; build GPU runs in a short folder.
    [string]$TargetDir
)
$ErrorActionPreference = 'Stop'
# pwsh -File passes 'cpu,cuda' as one string; accept both forms.
$Backends = $Backends -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ }
if ($bad = $Backends | Where-Object { $_ -notin 'cpu', 'vulkan', 'cuda' }) { throw "unknown backend: $bad" }
if ($TargetDir) { $env:CARGO_TARGET_DIR = $TargetDir }
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
    '| engine | backend | threads | clips | WER | p50 ms | p95 ms | mean RTF | load ms | engine RAM MB |',
    '|---|---|---|---|---|---|---|---|---|---|'
) | Set-Content $table

$whisper = @(
    @('whisper-base.en-q8', 'whisper\ggml-base.en-q8_0.bin'),
    @('whisper-small.en-q8', 'whisper\ggml-small.en-q8_0.bin'),
    @('whisper-large-v3-turbo-q5', 'whisper\ggml-large-v3-turbo-q5_0.bin')
)
$runs = @()
if ('cpu' -in $Backends) {
    $runs += , @('onnx', 'parakeet', 'parakeet-tdt-0.6b-v2-int8', 'cpu')
    $runs += , @('onnx', 'parakeet', 'parakeet-tdt-0.6b-v2-fp32', 'cpu')
}
foreach ($backend in $Backends) {
    foreach ($w in $whisper) { $runs += , @('ggml', 'whisper', $w[1], $backend, $w[0]) }
}

$failed = 0
foreach ($r in $runs) {
    $feat, $engine, $rel, $backend = $r[0..3]
    $name = if ($r.Count -gt 4) { $r[4] } else { $rel }
    $features = if ($feat -eq 'ggml' -and $backend -ne 'cpu') { "ggml,$backend" } else { $feat }
    $model = Join-Path $Models $rel
    if (-not (Test-Path $model)) { "skip $name ($backend): $model not found"; continue }
    "== $name on $backend start $(Get-Date -Format 'yyyy/MM/dd HH:mm:ss')"
    $cargoArgs = @('run', '--release', '-q', '-p', 'aural-bench', '--features', $features, '--',
              '--engine', $engine, '--model', $model, '--backend', $backend,
              '--corpus', $Corpus, '--out', (Join-Path $Out "$name-$backend.json"))
    if ($engine -eq 'whisper') { $cargoArgs += @('--threads', "$Threads") }
    $rows = & cargo @cargoArgs 2>&1 | ForEach-Object { "$_" }
    if ($LASTEXITCODE -ne 0) { $failed++; "FAILED ($LASTEXITCODE):"; $rows | Select-Object -Last 5; continue }
    $row = $rows | Where-Object { $_ -match '^\| ' -and $_ -notmatch '^\| engine' } | Select-Object -Last 1
    $row = $row -replace '^\| [^|]+\|', "| $name |"
    $row
    "== $name on $backend end $(Get-Date -Format 'yyyy/MM/dd HH:mm:ss')"
    Add-Content $table $row
}
"results: $table"
exit $failed
