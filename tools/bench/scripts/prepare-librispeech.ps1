# Build a fixed 100-clip benchmark corpus from LibriSpeech test-clean (CC BY 4.0,
# https://www.openslr.org/12). Clips are every Nth utterance in sorted-id order, so the
# selection is the same on every machine and spreads across all speakers.
#
#   ./tools/bench/scripts/prepare-librispeech.ps1 -Source <dir containing LibriSpeech\test-clean> `
#       -Out $env:USERPROFILE\aural-bench-corpus\librispeech-100
#
# Needs ffmpeg on PATH (FLAC -> 16 kHz mono WAV). Writes <Out>\wav\*.wav and <Out>\corpus.tsv.
param(
    [Parameter(Mandatory)][string]$Source,
    [Parameter(Mandatory)][string]$Out,
    [int]$Count = 100
)
$ErrorActionPreference = 'Stop'
$root = Join-Path $Source 'LibriSpeech\test-clean'
if (-not (Test-Path $root)) { throw "not found: $root" }

$utts = foreach ($t in Get-ChildItem $root -Recurse -Filter *.trans.txt) {
    foreach ($line in Get-Content $t.FullName) {
        $id, $text = $line -split ' ', 2
        [pscustomobject]@{ Id = $id; Text = $text; Flac = Join-Path $t.DirectoryName "$id.flac" }
    }
}
$utts = $utts | Sort-Object Id
$step = [math]::Floor($utts.Count / $Count)
$picked = for ($i = 0; $i -lt $Count; $i++) { $utts[$i * $step] }

New-Item -ItemType Directory -Force (Join-Path $Out 'wav') | Out-Null
$lines = @("# LibriSpeech test-clean, $Count clips (every ${step}th of $($utts.Count) utterances). CC BY 4.0.")
foreach ($u in $picked) {
    $wav = Join-Path $Out "wav\$($u.Id).wav"
    if (-not (Test-Path $wav)) {
        ffmpeg -nostdin -loglevel error -y -i $u.Flac -ac 1 -ar 16000 -c:a pcm_s16le $wav
        if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed on $($u.Flac)" }
    }
    $lines += "$($u.Id)`twav/$($u.Id).wav`t$($u.Text)"
}
Set-Content (Join-Path $Out 'corpus.tsv') $lines -Encoding utf8NoBOM
"$Count clips -> $(Join-Path $Out 'corpus.tsv')"
