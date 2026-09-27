<#
.SYNOPSIS
  Builds the 20 short speech clips the Hardware Test uses to check accuracy and speed
  (assets/eval): FLAC, 16 kHz mono, plus clips.tsv with the reference text.

.NOTES
  Source: the 100-clip LibriSpeech test-clean corpus made by
  tools/bench/scripts/prepare-librispeech.ps1 (LibriSpeech is CC BY 4.0; see
  THIRD_PARTY_NOTICES.md). Selection is fixed: clips 5–12 s long, in sorted-id order,
  spread evenly to 20. Needs ffmpeg on PATH. Output must stay under 3 MB.

    ./scripts/prepare-eval-clips.ps1 -Corpus $env:USERPROFILE\aural-bench-corpus\librispeech-100
#>
param(
    [Parameter(Mandatory)][string]$Corpus,
    [int]$Count = 20
)
$ErrorActionPreference = 'Stop'
$out = Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..')) 'assets\eval'

$rows = Get-Content (Join-Path $Corpus 'corpus.tsv') | Where-Object { $_ -and $_ -notmatch '^#' } | ForEach-Object {
    $id, $wav, $text = $_ -split "`t", 3
    $path = Join-Path $Corpus $wav
    [pscustomobject]@{ Id = $id; Wav = $path; Text = $text; Secs = ((Get-Item $path).Length - 44) / 32000 }
}
$inRange = @($rows | Where-Object { $_.Secs -ge 5 -and $_.Secs -le 12 } | Sort-Object Id)
if ($inRange.Count -lt $Count) { throw "only $($inRange.Count) clips are 5-12 s long" }
$picked = for ($i = 0; $i -lt $Count; $i++) { $inRange[[math]::Floor($i * $inRange.Count / $Count)] }

New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.flac | ForEach-Object { Remove-Item -LiteralPath $_.FullName }
$lines = @("# LibriSpeech test-clean (CC BY 4.0), $Count clips of 5-12 s. id<TAB>file<TAB>reference")
foreach ($c in $picked) {
    $flac = Join-Path $out "$($c.Id).flac"
    ffmpeg -nostdin -loglevel error -y -i $c.Wav -ac 1 -ar 16000 -sample_fmt s16 -c:a flac -compression_level 12 $flac
    if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed on $($c.Wav)" }
    $lines += "$($c.Id)`t$($c.Id).flac`t$($c.Text)"
}
Set-Content (Join-Path $out 'clips.tsv') $lines -Encoding utf8NoBOM
$bytes = (Get-ChildItem $out -File | Measure-Object Length -Sum).Sum
if ($bytes -gt 3MB) { throw "assets/eval is $([math]::Round($bytes / 1MB, 2)) MB; the limit is 3 MB" }
"$Count clips, $([math]::Round(($picked | Measure-Object Secs -Sum).Sum)) s, $([math]::Round($bytes / 1KB)) KB -> $out"
