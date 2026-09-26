<#
.SYNOPSIS
  Writes the WinGet manifests (version, installer, en-US locale) for one Aural release.

.DESCRIPTION
  The installer hash must be the SHA-256 of the exact file published on GitHub
  Releases, so manifests are generated from that file at release time rather than kept
  in the repo. The release workflow runs this script and attaches the output; a
  maintainer then submits it to microsoft/winget-pkgs (see packaging/winget/README.md).

.EXAMPLE
  ./New-WingetManifests.ps1 -Version 0.1.0 -Installer ..\..\target\release\bundle\nsis\Aural_0.1.0_x64-setup.exe `
      -Repository Djdhmf/Aural -License "PolyForm-Noncommercial-1.0.0" -OutDir out
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Version,
    [Parameter(Mandatory)] [string]$Installer,
    [Parameter(Mandatory)] [string]$Repository,
    [Parameter(Mandatory)] [string]$License,
    [string]$LicenseUrl = "https://github.com/$Repository/blob/v$Version/LICENSE.md",
    [Parameter(Mandatory)] [string]$OutDir
)
$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must look like 1.2.3, got '$Version'" }
$id = 'Aurathex.Aural'
$schema = '1.10.0'
$file = Split-Path $Installer -Leaf
$sha = (Get-FileHash -Algorithm SHA256 $Installer).Hash.ToUpperInvariant()
$url = "https://github.com/$Repository/releases/download/v$Version/$file"
$dir = Join-Path $OutDir "manifests\a\Aurathex\Aural\$Version"
New-Item -ItemType Directory -Force $dir | Out-Null

$header = { param($kind) "# yaml-language-server: `$schema=https://aka.ms/winget-manifest.$kind.$schema.schema.json`n" }

$versionYaml = (& $header 'version') + @"
PackageIdentifier: $id
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
"@

# Tauri's per-user NSIS installer: silent with /S, registers under HKCU with the
# uninstall key "Aural", no admin rights needed.
$installerYaml = (& $header 'installer') + @"
PackageIdentifier: $id
PackageVersion: $Version
Platform:
- Windows.Desktop
MinimumOSVersion: 10.0.17763.0
InstallerType: nullsoft
Scope: user
InstallModes:
- interactive
- silent
- silentWithProgress
InstallerSwitches:
  Silent: /S
  SilentWithProgress: /S
UpgradeBehavior: install
ProductCode: Aural
AppsAndFeaturesEntries:
- DisplayName: Aural
  Publisher: Aurathex
  DisplayVersion: $Version
  ProductCode: Aural
Installers:
- Architecture: x64
  InstallerUrl: $url
  InstallerSha256: $sha
ManifestType: installer
ManifestVersion: $schema
"@

$localeYaml = (& $header 'defaultLocale') + @"
PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: en-US
Publisher: Aurathex
PublisherUrl: https://github.com/$Repository
PublisherSupportUrl: https://github.com/$Repository/issues
PrivacyUrl: https://github.com/$Repository/blob/v$Version/PRIVACY.md
PackageName: Aural
PackageUrl: https://github.com/$Repository
License: $License
LicenseUrl: $LicenseUrl
Copyright: Copyright (c) 2026 Aurathex
ShortDescription: Local voice dictation for Windows. Hold a hotkey, speak, release.
Description: |-
  Aural transcribes your speech on this PC and types it into the app you are using.
  Hold the hotkey (Ctrl + Win by default), speak, and let go. Speech models run
  locally and are downloaded from inside the app; audio is never uploaded, there is
  no account and no telemetry.
Moniker: aural
Tags:
- dictation
- speech-to-text
- voice-typing
- transcription
- offline
- privacy
ReleaseNotesUrl: https://github.com/$Repository/releases/tag/v$Version
ManifestType: defaultLocale
ManifestVersion: $schema
"@

$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $dir "$id.yaml"), $versionYaml.Replace("`r`n", "`n") + "`n", $utf8)
[IO.File]::WriteAllText((Join-Path $dir "$id.installer.yaml"), $installerYaml.Replace("`r`n", "`n") + "`n", $utf8)
[IO.File]::WriteAllText((Join-Path $dir "$id.locale.en-US.yaml"), $localeYaml.Replace("`r`n", "`n") + "`n", $utf8)
Write-Output $dir
