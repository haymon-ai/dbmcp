# Install script for database-mcp (Windows)
# Usage: irm https://database.haymon.ai/install.ps1 | iex
#    or: powershell -ExecutionPolicy Bypass -File install.ps1
#
# Environment variables:
#   INSTALL_DIR - Override the install directory

& {
    $ErrorActionPreference = "Stop"

    # Enforce TLS 1.2 for GitHub downloads (required on older .NET defaults)
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch {}

    function Resolve-InstallDir {
        param([string]$BinaryName)

        # Priority 1: INSTALL_DIR env var
        $EnvDir = $env:INSTALL_DIR
        if ($EnvDir) {
            return @{ Path = $EnvDir; IsUpgrade = $false; OldVersion = "" }
        }

        # Priority 2: Existing binary detection
        $Existing = Get-Command $BinaryName -ErrorAction SilentlyContinue
        if ($Existing) {
            $ExistingPath = Split-Path $Existing.Source -Parent
            $OldVer = ""
            try {
                $OldVer = (& $Existing.Source --version 2>&1 | Out-String).Trim()
            } catch {}
            return @{ Path = $ExistingPath; IsUpgrade = $true; OldVersion = "$OldVer" }
        }

        # Priority 3: Default location
        $DefaultDir = Join-Path $env:LOCALAPPDATA "Programs\database-mcp"
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

    $BinaryName = "database-mcp"
    $Repo = "haymon-ai/database"
    $Target = "x86_64-pc-windows-msvc"
    $Asset = "$BinaryName-$Target.zip"
    $BaseUrl = "https://github.com/$Repo/releases/latest/download"
    $Url = "$BaseUrl/$Asset"

    $TmpDir = $null

    try {
        Write-Host ""
        Write-Host "Installing database-mcp..." -ForegroundColor Cyan
        Write-Host ""

        # Resolve install directory
        $InstallInfo = Resolve-InstallDir -BinaryName $BinaryName
        $IsUpgrade = $InstallInfo.IsUpgrade
        $OldVersion = $InstallInfo.OldVersion
        $BinDir = $InstallInfo.Path

        if ($IsUpgrade) {
            Write-Host "Upgrading database-mcp at $BinDir (current: $OldVersion)" -ForegroundColor Cyan
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
            throw "failed to install binary to $BinDir - is an existing database-mcp process running? ($($_.Exception.Message))"
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
        if ($IsUpgrade) {
            Write-Host "Successfully upgraded database-mcp!" -ForegroundColor Green
            Write-Host "  $OldVersion -> $InstalledVersion"
        } else {
            Write-Host "Successfully installed database-mcp!" -ForegroundColor Green
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
