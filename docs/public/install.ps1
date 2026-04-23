# Install script for dbmcp (Windows)
# Usage: irm https://dbmcp.haymon.ai/install.ps1 | iex
#    or: powershell -ExecutionPolicy Bypass -File install.ps1
#
# Environment variables:
#   INSTALL_DIR   - Override the install directory
#   FORCE_INSTALL - When set to a truthy value (1/true/yes/on/y, case-insensitive)
#                   reinstall even if the installed version already matches the
#                   latest published release. Without this, re-running the
#                   script on an up-to-date install is a no-op (no download,
#                   no file writes).
#
# The script probes https://github.com/haymon-ai/dbmcp/releases/latest
# (a HEAD request that follows redirects) to learn the latest version. The
# no-op path performs zero downloads and zero writes to the install directory.

& {
    $ErrorActionPreference = "Stop"

    # Enforce TLS 1.2 for GitHub downloads (required on older .NET defaults).
    # Silent fallthrough is intentional: on older .NET that doesn't expose
    # Tls12, we can't do anything, and the subsequent Invoke-WebRequest will
    # fail with a clear error if the connection can't be negotiated.
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch {
        $null = $_
    }

    function Test-Truthy {
        param([string]$Value)
        if (-not $Value) { return $false }
        return @('1','true','yes','on','y') -contains $Value.ToLowerInvariant()
    }

    function Get-NormalizedVersion {
        param([string]$Version)
        if (-not $Version) { return '' }
        $v = $Version.Trim()
        if ($v.StartsWith('dbmcp ')) { $v = $v.Substring('dbmcp '.Length) }
        if ($v.StartsWith('v') -or $v.StartsWith('V')) { $v = $v.Substring(1) }
        return $v
    }

    function Resolve-LatestVersion {
        param([string]$Repo)
        $LatestUrl = "https://github.com/$Repo/releases/latest"
        $resp = $null
        try {
            # Short timeout: the no-op path should be quick or not at all;
            # we don't want to hang for 100+ seconds on a broken network.
            $resp = Invoke-WebRequest -Uri $LatestUrl -Method Head -MaximumRedirection 5 -UseBasicParsing -TimeoutSec 20 -ErrorAction Stop
        } catch {
            return $null
        }
        if (-not $resp -or -not $resp.BaseResponse) { return $null }
        # The final redirect target is exposed differently across PowerShell
        # versions, so try both paths:
        #   PS 5.1 (Desktop)  -> BaseResponse is HttpWebResponse      -> .ResponseUri
        #   PS 7+  (Core)     -> BaseResponse is HttpResponseMessage  -> .RequestMessage.RequestUri
        # Never fall back to $resp.Headers['Location']: that is the FIRST
        # redirect's target and is incorrect on multi-hop chains.
        $finalUri = $null
        if ($resp.BaseResponse.PSObject.Properties['ResponseUri'] -and $resp.BaseResponse.ResponseUri) {
            $finalUri = $resp.BaseResponse.ResponseUri.AbsoluteUri
        } elseif ($resp.BaseResponse.PSObject.Properties['RequestMessage'] -and $resp.BaseResponse.RequestMessage -and $resp.BaseResponse.RequestMessage.RequestUri) {
            $finalUri = $resp.BaseResponse.RequestMessage.RequestUri.AbsoluteUri
        }
        if (-not $finalUri) { return $null }
        $tag = $finalUri.TrimEnd('/').Split('/')[-1]
        if (-not $tag) { return $null }
        if ($tag -notmatch '^v?[0-9]') { return $null }
        return $tag
    }

    function Test-InstalledNewerThanLatest {
        param([string]$InstalledNorm, [string]$LatestNorm)
        if (-not $InstalledNorm -or -not $LatestNorm) { return $false }
        try {
            # Split on `-` to drop pre-release/build suffixes (e.g. `0.7.0-dev`).
            # [version] requires at least major.minor; pad with `.0` if needed.
            $iCore = $InstalledNorm.Split('-')[0]
            $lCore = $LatestNorm.Split('-')[0]
            if ($iCore -notmatch '\.') { $iCore = "$iCore.0" }
            if ($lCore -notmatch '\.') { $lCore = "$lCore.0" }
            return ([version]$iCore -gt [version]$lCore)
        } catch {
            return $false
        }
    }

    function Resolve-InstallDir {
        param([string]$BinaryName)

        # Priority 1: INSTALL_DIR env var
        $EnvDir = $env:INSTALL_DIR
        if ($EnvDir) {
            # If a binary already exists at the override location, treat as
            # an upgrade so the no-op check applies to the binary that will
            # actually be replaced (spec Edge Cases).
            $candidate = Join-Path $EnvDir "$BinaryName.exe"
            if (Test-Path -LiteralPath $candidate) {
                $oldVer = ""
                try {
                    $oldVer = (& $candidate --version 2>&1 | Out-String).Trim()
                } catch {
                    $oldVer = ""
                }
                if (-not $oldVer) { $oldVer = "unknown" }
                return @{ Path = $EnvDir; IsUpgrade = $true; OldVersion = $oldVer }
            }
            return @{ Path = $EnvDir; IsUpgrade = $false; OldVersion = "" }
        }

        # Priority 2: Existing binary detection
        $Existing = Get-Command $BinaryName -ErrorAction SilentlyContinue
        if ($Existing) {
            $ExistingPath = Split-Path $Existing.Source -Parent
            $OldVer = ""
            try {
                $OldVer = (& $Existing.Source --version 2>&1 | Out-String).Trim()
            } catch {
                $OldVer = ""
            }
            if (-not $OldVer) { $OldVer = "unknown" }
            return @{ Path = $ExistingPath; IsUpgrade = $true; OldVersion = "$OldVer" }
        }

        # Priority 3: Default location
        $DefaultDir = Join-Path $env:LOCALAPPDATA "Programs\dbmcp"
        return @{ Path = $DefaultDir; IsUpgrade = $false; OldVersion = "" }
    }

    function Add-ToPath {
        param([string]$Dir)

        $CurrentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
        $AlreadyInPath = $false
        if ($CurrentPath) {
            foreach ($entry in ($CurrentPath -split ";")) {
                if ($entry -ieq $Dir) {
                    $AlreadyInPath = $true
                    break
                }
            }
        }
        if ($AlreadyInPath) {
            return
        }

        Write-Host ""
        Write-Host "warning: $Dir is not in your PATH" -ForegroundColor Yellow
        Write-Host ""

        # Add to user PATH via .NET (preserves REG_EXPAND_SZ and has no 1024-char limit)
        if ($CurrentPath) {
            $NewPath = "$Dir;$CurrentPath"
        } else {
            $NewPath = $Dir
        }
        [Environment]::SetEnvironmentVariable("PATH", $NewPath, "User")

        # Also add to current session
        $env:PATH = "$Dir;$env:PATH"

        Write-Host "  Added $Dir to your user PATH." -ForegroundColor Green
        Write-Host "  Restart your terminal for the change to take effect in new sessions."
        Write-Host ""
    }

    # --- Main flow ---

    $BinaryName = "dbmcp"
    $Repo = "haymon-ai/dbmcp"
    $Target = "x86_64-pc-windows-msvc"
    $Asset = "$BinaryName-$Target.zip"
    $BaseUrl = "https://github.com/$Repo/releases/latest/download"
    $Url = "$BaseUrl/$Asset"

    $TmpDir = $null
    $ForceReinstall = $false

    try {
        Write-Host ""
        Write-Host "Installing dbmcp..." -ForegroundColor Cyan
        Write-Host ""

        # Resolve install directory
        $InstallInfo = Resolve-InstallDir -BinaryName $BinaryName
        $IsUpgrade = $InstallInfo.IsUpgrade
        $OldVersion = $InstallInfo.OldVersion
        $BinDir = $InstallInfo.Path

        if ($IsUpgrade) {
            Write-Host "Found existing dbmcp at $BinDir ($OldVersion)" -ForegroundColor Cyan

            # No-op / force / newer-than-latest check. Only runs when an
            # existing binary was detected. Skipped entirely if the version
            # lookup fails, so a network issue never silently claims
            # "already up to date".
            $LatestVersion = Resolve-LatestVersion -Repo $Repo
            if ($LatestVersion -and $OldVersion) {
                $InstalledNorm = Get-NormalizedVersion $OldVersion
                $LatestNorm = Get-NormalizedVersion $LatestVersion

                if ($InstalledNorm -eq $LatestNorm -and $InstalledNorm) {
                    if (Test-Truthy $env:FORCE_INSTALL) {
                        $ForceReinstall = $true
                        Write-Host "FORCE_INSTALL is set - reinstalling $LatestVersion" -ForegroundColor Cyan
                    } else {
                        Write-Host ""
                        Write-Host "Already on latest version ($LatestVersion). Nothing to do." -ForegroundColor Green
                        return
                    }
                } else {
                    if (Test-InstalledNewerThanLatest -InstalledNorm $InstalledNorm -LatestNorm $LatestNorm) {
                        Write-Host "warning: installed version $OldVersion is newer than the latest release $LatestVersion." -ForegroundColor Yellow
                        Write-Host "         proceeding will replace it with the older release." -ForegroundColor Yellow
                    }
                    Write-Host "Upgrading to $LatestVersion" -ForegroundColor Cyan
                }
            } else {
                Write-Host "Upgrading dbmcp at $BinDir (current: $OldVersion)" -ForegroundColor Cyan
            }
        } else {
            Write-Host "Install directory: $BinDir" -ForegroundColor Cyan
        }

        # Create install directory if needed
        if (-not (Test-Path $BinDir)) {
            New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
        }

        # Create temp directory
        $TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

        $ArchivePath = Join-Path $TmpDir $Asset

        Write-Host "Downloading $Asset..." -ForegroundColor Cyan
        try {
            Invoke-WebRequest -Uri $Url -OutFile $ArchivePath -UseBasicParsing
        } catch {
            throw "download failed - check your network connection or try again later`n  URL: $Url"
        }

        # Extract
        $ExtractDir = Join-Path $TmpDir "extracted"
        try {
            Expand-Archive -Path $ArchivePath -DestinationPath $ExtractDir -Force
        } catch {
            throw "failed to extract archive: $($_.Exception.Message)"
        }

        $BinaryPath = Join-Path $ExtractDir "$BinaryName.exe"
        if (-not (Test-Path $BinaryPath)) {
            throw "binary '$BinaryName.exe' not found in archive"
        }

        # Install
        $Dest = Join-Path $BinDir "$BinaryName.exe"
        try {
            Copy-Item -Path $BinaryPath -Destination $Dest -Force
        } catch {
            throw "failed to install binary to $BinDir - is an existing dbmcp process running? ($($_.Exception.Message))"
        }

        # Verify
        $InstalledVersion = ""
        try {
            $InstalledVersion = (& $Dest --version 2>&1 | Out-String).Trim()
        } catch {
            throw "installation verification failed - $Dest --version did not produce output"
        }
        if (-not $InstalledVersion) {
            throw "installation verification failed - $Dest --version did not produce output"
        }

        Write-Host ""
        if ($ForceReinstall) {
            Write-Host "Successfully reinstalled dbmcp!" -ForegroundColor Green
            Write-Host "  Version: $InstalledVersion"
        } elseif ($IsUpgrade) {
            Write-Host "Successfully upgraded dbmcp!" -ForegroundColor Green
            Write-Host "  $OldVersion -> $InstalledVersion"
        } else {
            Write-Host "Successfully installed dbmcp!" -ForegroundColor Green
            Write-Host "  Version: $InstalledVersion"
        }
        Write-Host "  Location: $Dest"

        # Check PATH and add if needed
        Add-ToPath -Dir $BinDir
    }
    catch {
        Write-Host ""
        Write-Host "error: $($_.Exception.Message)" -ForegroundColor Red
        Write-Host ""
        $global:LASTEXITCODE = 1
    }
    finally {
        # Cleanup temp directory
        if ($TmpDir -and (Test-Path $TmpDir)) {
            Remove-Item -Path $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
