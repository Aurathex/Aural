# E2E: the GUI upgrade path runs the old uninstaller (no /UPDATE), then installs.
# Autostart that was on before must still be on afterwards (and off must stay off).
# Installs and uninstalls Aural for the current user; run it with Aural not installed.
# An existing settings folder and "start with Windows" entry are set aside first and
# put back afterwards, even if the test fails.
# Usage: ./scripts/test-upgrade-autostart.ps1 -Installer <setup.exe> [-Off]
param([switch]$Off, [Parameter(Mandatory)][string]$Installer)
$ErrorActionPreference = 'Stop'
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$uninstKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Aural'
$cfg = "$env:APPDATA\com.aurathex.aural"
$exe = "$env:LOCALAPPDATA\Aural\aural.exe"

function Wait-Until([scriptblock]$cond, [int]$secs = 60) {
  $t = [Diagnostics.Stopwatch]::StartNew()
  while (-not (& $cond)) { if ($t.Elapsed.TotalSeconds -gt $secs) { throw "timeout waiting for $cond" }; Start-Sleep -Milliseconds 500 }
}
function Uninstall-Silently {
  Start-Process "$env:LOCALAPPDATA\Aural\uninstall.exe" -ArgumentList '/S' -Wait
  Wait-Until { -not (Test-Path $uninstKey) -and -not (Test-Path $exe) }
}

if (Test-Path $uninstKey) { throw 'Aural is already installed; uninstall it first' }

# Set the real settings and autostart entry aside.
$backup = $null
if (Test-Path $cfg) {
  $backup = "$cfg.upgrade-test-$(Get-Date -Format yyyyMMddHHmmss)"
  Move-Item -LiteralPath $cfg -Destination $backup
}
$savedRun = (Get-ItemProperty $run -Name Aural -ErrorAction SilentlyContinue).Aural

try {
  Start-Process $Installer -ArgumentList '/S' -Wait
  Wait-Until { Test-Path $exe }
  # State after a user used Aural with "Start Aural with Windows" on.
  New-Item -ItemType Directory -Force $cfg | Out-Null
  Set-Content "$cfg\.aural-root" '' -NoNewline
  $expected = "`"$exe`" --autostart"
  if ($Off) {
    $expected = $null
    Remove-ItemProperty $run -Name Aural -ErrorAction SilentlyContinue
  } else {
    Set-ItemProperty $run -Name Aural -Value $expected
  }

  Uninstall-Silently                                 # old uninstaller, as the upgrade does
  Start-Process $Installer -ArgumentList '/S' -Wait  # new version installs
  Wait-Until { Test-Path $exe }

  $after = (Get-ItemProperty $run -Name Aural -ErrorAction SilentlyContinue).Aural
  $flagLeft = Test-Path "$cfg\restore-autostart"
  Uninstall-Silently
} finally {
  # Remove only the folder this test created, then put the real one back.
  if (Test-Path "$cfg\.aural-root") { Remove-Item -LiteralPath $cfg -Recurse -Force }
  if ($backup) { Move-Item -LiteralPath $backup -Destination $cfg }
  if ($savedRun) { Set-ItemProperty $run -Name Aural -Value $savedRun }
  else { Remove-ItemProperty $run -Name Aural -ErrorAction SilentlyContinue }
}

"autostart after upgrade: '$after'"
"flag file left behind: $flagLeft"
if ($after -ne $expected) { 'FAIL: autostart lost on upgrade'; exit 1 }
if ($flagLeft) { 'FAIL: restore flag not cleaned up'; exit 1 }
'PASS'
