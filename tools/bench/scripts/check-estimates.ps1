# How good are the hardware test's speed estimates? For every catalog variant, compare
#   estimated p50 = calibrator's p50 here × (variant's reference p50 ÷ calibrator's reference p50)
# with the variant's own measured p50, both measured on the same clips (the built-in
# clips the hardware test uses). Calibrators are the probe models, and for ONNX on the
# graphics card (no probe runs there) a measured Parakeet v2, as the app does.
#
#   ./tools/bench/scripts/check-estimates.ps1 -Measured $env:USERPROFILE\aural-bench-corpus\results-v02-eval
#
# Prints a markdown table; the spec's pass mark is within ±35% for each variant.
param(
    [Parameter(Mandatory)][string]$Measured,
    [double]$Tolerance = 0.35
)
$ErrorActionPreference = 'Stop'
$catalog = Get-Content (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')) 'manifests\catalog.v2.json') -Raw | ConvertFrom-Json
$calibrator = @{
    'ggml/cpu'      = 'whisper-tiny.en-q8@cpu'
    'ggml/vulkan'   = 'whisper-tiny.en-q8@vulkan'
    'onnx/cpu'      = 'moonshine-tiny-int8@cpu'
    'onnx/directml' = 'parakeet-tdt-0.6b-v2-int8@directml'
}
$ref = @{}
foreach ($m in $catalog.models) { foreach ($v in $m.variants) { $ref["$($m.id)@$($v.backend)"] = @{ model = $m; p50 = $v.reference.p50_ms } } }
function Here($variant) {
    $f = Join-Path $Measured "$variant.json"
    if (Test-Path $f) { (Get-Content $f -Raw | ConvertFrom-Json).summary.p50_ms }
}
'| variant | calibrated by | estimated p50 ms | measured p50 ms | error | within ±{0:P0} |' -f $Tolerance
'|---|---|---|---|---|---|'
$misses = 0
foreach ($id in $ref.Keys | Sort-Object) {
    $m = $ref[$id].model
    if ($m.probe) { continue }
    $cal = $calibrator["$($m.runtime)/$($id.Split('@')[1])"]
    if ($cal -eq $id) { continue }
    $calHere = Here $cal
    $here = Here $id
    if (-not $calHere -or -not $here -or -not $ref[$id].p50 -or -not $ref[$cal].p50) { "| $id | $cal | – | – | – | no data |"; continue }
    $est = $calHere * $ref[$id].p50 / $ref[$cal].p50
    $err = $est / $here - 1
    $ok = [math]::Abs($err) -le $Tolerance
    if (-not $ok) { $misses++ }
    '| {0} | {1} | {2:N0} | {3:N0} | {4:+0%;-0%} | {5} |' -f $id, $cal, $est, $here, $err, $(if ($ok) { 'yes' } else { '**no**' })
}
''
"misses: $misses"
