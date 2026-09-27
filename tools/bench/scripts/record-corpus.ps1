# Record the (optional) personal dictation corpus, a check on real dictation: one WAV per prompt, plus a
# corpus.tsv the bench reads. Audio stays outside the repo.
#
#   ./tools/bench/scripts/record-corpus.ps1                   # record every prompt
#   ./tools/bench/scripts/record-corpus.ps1 -Only p07,p12     # re-record some
#   ./tools/bench/scripts/record-corpus.ps1 -ListDevices
#
# For each prompt: press Enter to start, read the line aloud, press Enter to stop.
# Then choose keep / redo / skip. If you said something different from the prompt,
# edit that line of corpus.tsv so the reference matches what you actually said.
# Needs ffmpeg on PATH.
param(
    [string]$Out = "$env:USERPROFILE\aural-bench-corpus\personal",
    [string]$Prompts = "$PSScriptRoot\..\corpus\personal-prompts.txt",
    [string]$Device,
    [string[]]$Only,
    [switch]$ListDevices
)
$ErrorActionPreference = 'Stop'

function Get-Mics {
    ffmpeg -hide_banner -list_devices true -f dshow -i dummy 2>&1 |
        Select-String '"(.+)" \(audio\)' | ForEach-Object { $_.Matches[0].Groups[1].Value }
}
if ($ListDevices) { Get-Mics; return }
if (-not $Device) {
    $Device = Get-Mics | Where-Object { $_ -match 'Microphone' -and $_ -notmatch 'Steam' } | Select-Object -First 1
    if (-not $Device) { throw 'no microphone found; pass -Device (see -ListDevices)' }
}
"Microphone: $Device"

$lines = Get-Content $Prompts | Where-Object { $_ -and -not $_.StartsWith('#') }
New-Item -ItemType Directory -Force (Join-Path $Out 'wav') | Out-Null
$tsv = Join-Path $Out 'corpus.tsv'
$rows = [ordered]@{}
if (Test-Path $tsv) {
    foreach ($l in Get-Content $tsv | Where-Object { $_ -and -not $_.StartsWith('#') }) {
        $rows[($l -split "`t")[0]] = $l
    }
}

function Save-Tsv {
    $out = @('# Personal dictation corpus for Aural M0. id<TAB>wav<TAB>reference')
    $out += $rows.Keys | Sort-Object | ForEach-Object { $rows[$_] }
    Set-Content $tsv $out -Encoding utf8NoBOM
}

function Record([string]$wav) {
    $psi = [Diagnostics.ProcessStartInfo]::new('ffmpeg')
    # A small capture buffer and per-packet flushing let the script see audio arrive.
    foreach ($a in @('-hide_banner', '-loglevel', 'error', '-y', '-f', 'dshow', '-audio_buffer_size', '50',
                     '-i', "audio=$Device", '-ac', '1', '-ar', '16000', '-c:a', 'pcm_s16le',
                     '-flush_packets', '1', $wav)) { $psi.ArgumentList.Add($a) }
    $psi.RedirectStandardInput = $true
    $psi.UseShellExecute = $false
    $p = [Diagnostics.Process]::Start($psi)
    # The microphone takes about a second to open; don't prompt until audio is flowing,
    # or the first words are lost.
    $t = [Diagnostics.Stopwatch]::StartNew()
    while (-not ((Test-Path $wav) -and (Get-Item $wav).Length -gt 44 + 3200)) {
        if ($p.HasExited -or $t.Elapsed.TotalSeconds -gt 10) { throw "ffmpeg could not record from '$Device'" }
        Start-Sleep -Milliseconds 50
    }
    [void](Read-Host '  SPEAK NOW - press Enter when done')
    $p.StandardInput.Write('q')
    $p.StandardInput.Flush()
    if (-not $p.WaitForExit(5000)) { $p.Kill() }
}

for ($i = 0; $i -lt $lines.Count; $i++) {
    $id = 'p{0:D2}' -f ($i + 1)
    if ($Only -and $id -notin $Only) { continue }
    $text = $lines[$i]
    $wav = Join-Path $Out "wav\$id.wav"
    while ($true) {
        ''
        "[$id] $text"
        [void](Read-Host '  press Enter, then read the line')
        Record $wav
        $secs = [math]::Round(((Get-Item $wav).Length - 44) / 32000, 1)
        $choice = Read-Host "  $secs s recorded. [k]eep, [r]edo, [s]kip"
        if ($choice -eq 'r') { continue }
        if ($choice -eq 's') { Remove-Item $wav -ErrorAction SilentlyContinue; break }
        $rows[$id] = "$id`twav/$id.wav`t$text"
        Save-Tsv
        break
    }
}
''
"$($rows.Count) clips in $tsv"
