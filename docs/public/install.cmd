@echo off
setlocal enabledelayedexpansion

REM Install script for dbmcp (Windows CMD)
REM Usage: curl -fsSL https://dbmcp.haymon.ai/install.cmd -o install.cmd && install.cmd && del install.cmd
REM
REM Environment variables:
REM   INSTALL_DIR   - Override the install directory
REM   FORCE_INSTALL - When set to a truthy value (1/true/yes/on/y, case-insensitive)
REM                   reinstall even if the installed version already matches the
REM                   latest published release. Without this, re-running the
REM                   script on an up-to-date install is a no-op (no download,
REM                   no file writes).
REM
REM The script probes https://github.com/haymon-ai/dbmcp/releases/latest
REM (a HEAD request that follows redirects) to learn the latest version. The
REM no-op path performs zero downloads and zero writes to the install directory.

set "BINARY_NAME=dbmcp"
set "REPO=haymon-ai/dbmcp"
set "TARGET=x86_64-pc-windows-msvc"
set "ASSET=%BINARY_NAME%-%TARGET%.zip"
set "BASE_URL=https://github.com/%REPO%/releases/latest/download"
set "URL=%BASE_URL%/%ASSET%"
set "IS_REINSTALL=0"

echo.
echo Installing dbmcp...
echo.

REM Resolve install directory
set "BIN_DIR="
set "IS_UPGRADE=0"
set "OLD_VERSION="

REM Priority 1: INSTALL_DIR env var
if defined INSTALL_DIR (
    set "BIN_DIR=%INSTALL_DIR%"
    REM If a binary already exists at the override location, treat as an
    REM upgrade so the no-op check applies to the binary that will actually
    REM be replaced (spec Edge Cases). Pre-seed OLD_VERSION to `unknown` so
    REM that if --version fails, downstream code sees a sentinel rather than
    REM an empty string (the for /f below silently overwrites on success).
    if exist "%INSTALL_DIR%\%BINARY_NAME%.exe" (
        set "IS_UPGRADE=1"
        set "OLD_VERSION=unknown"
        for /f "delims=" %%v in ('""%INSTALL_DIR%\%BINARY_NAME%.exe" --version" 2^>nul') do set "OLD_VERSION=%%v"
    )
    goto :resolved
)

REM Priority 2: Existing binary detection
where %BINARY_NAME% >nul 2>&1
if %errorlevel% equ 0 (
    for /f "delims=" %%i in ('where %BINARY_NAME%') do (
        set "EXISTING=%%i"
        goto :found_existing
    )
)
goto :default_dir

:found_existing
for %%i in ("%EXISTING%") do set "BIN_DIR=%%~dpi"
REM Remove trailing backslash
if "%BIN_DIR:~-1%"=="\" set "BIN_DIR=%BIN_DIR:~0,-1%"
set "IS_UPGRADE=1"
set "OLD_VERSION=unknown"
for /f "delims=" %%v in ('""%EXISTING%" --version" 2^>nul') do set "OLD_VERSION=%%v"
goto :resolved

:default_dir
REM Priority 3: Default location
set "BIN_DIR=%LOCALAPPDATA%\Programs\dbmcp"

:resolved
if %IS_UPGRADE% equ 1 (
    echo Found existing dbmcp at %BIN_DIR% ^(%OLD_VERSION%^)
) else (
    echo Install directory: %BIN_DIR%
    goto :prepare_install
)

REM ---------------------------------------------------------------------------
REM No-op / force / newer-than-latest check.
REM Only runs on the upgrade path (an existing binary was detected). Delegates
REM HTTP + URL parsing + version comparison to a single PowerShell call that
REM prints one line of the form "<ACTION> <tag>". ACTION is one of:
REM   NOOP    - installed matches latest, FORCE_INSTALL not set   -> skip install
REM   FORCE   - installed matches latest, FORCE_INSTALL set       -> reinstall
REM   NEWER   - installed is newer than latest                    -> warn + install
REM   UPGRADE - installed is older than latest                    -> normal upgrade
REM If PowerShell fails or the lookup cannot be performed, the command emits
REM nothing, ACTION stays empty, and the script falls through to the normal
REM download-and-install flow (FR-011).
REM ---------------------------------------------------------------------------
set "ACTION="
set "LATEST_VERSION="
set "DBMCP_OLD_VERSION=%OLD_VERSION%"
set "DBMCP_FORCE=%FORCE_INSTALL%"
set "DBMCP_REPO=%REPO%"

REM Write a small helper script to a temp file and invoke it. This keeps the
REM PowerShell logic readable without relying on the fragile combination of
REM `for /f`, backquotes, and caret line-continuation.
set "NOOP_HELPER=%TEMP%\dbmcp-noop-%RANDOM%%RANDOM%.ps1"
> "%NOOP_HELPER%" echo $ErrorActionPreference='SilentlyContinue'
>>"%NOOP_HELPER%" echo try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch {}
>>"%NOOP_HELPER%" echo $r = $null
>>"%NOOP_HELPER%" echo try { $r = Invoke-WebRequest -Uri ('https://github.com/' + $env:DBMCP_REPO + '/releases/latest') -Method Head -MaximumRedirection 5 -UseBasicParsing -TimeoutSec 20 -ErrorAction Stop } catch { exit 0 }
>>"%NOOP_HELPER%" echo if (-not $r -or -not $r.BaseResponse) { exit 0 }
>>"%NOOP_HELPER%" echo $u = $null
>>"%NOOP_HELPER%" echo if ($r.BaseResponse.PSObject.Properties['ResponseUri'] -and $r.BaseResponse.ResponseUri) { $u = $r.BaseResponse.ResponseUri.AbsoluteUri } elseif ($r.BaseResponse.PSObject.Properties['RequestMessage'] -and $r.BaseResponse.RequestMessage -and $r.BaseResponse.RequestMessage.RequestUri) { $u = $r.BaseResponse.RequestMessage.RequestUri.AbsoluteUri }
>>"%NOOP_HELPER%" echo if (-not $u) { exit 0 }
>>"%NOOP_HELPER%" echo $tag = $u.TrimEnd('/').Split('/')[-1]
>>"%NOOP_HELPER%" echo if (-not ($tag -match '^^v?[0-9]')) { exit 0 }
>>"%NOOP_HELPER%" echo $inst = $env:DBMCP_OLD_VERSION
>>"%NOOP_HELPER%" echo if (-not $inst) { exit 0 }
>>"%NOOP_HELPER%" echo $iN = $inst.Trim()
>>"%NOOP_HELPER%" echo if ($iN.StartsWith('dbmcp ')) { $iN = $iN.Substring('dbmcp '.Length) }
>>"%NOOP_HELPER%" echo if ($iN.StartsWith('v') -or $iN.StartsWith('V')) { $iN = $iN.Substring(1) }
>>"%NOOP_HELPER%" echo $lN = $tag
>>"%NOOP_HELPER%" echo if ($lN.StartsWith('v') -or $lN.StartsWith('V')) { $lN = $lN.Substring(1) }
>>"%NOOP_HELPER%" echo $truthy = $false
>>"%NOOP_HELPER%" echo $force = $env:DBMCP_FORCE
>>"%NOOP_HELPER%" echo if ($force) { $truthy = @('1','true','yes','on','y') -contains $force.ToLowerInvariant() }
>>"%NOOP_HELPER%" echo if ($iN -eq $lN) { if ($truthy) { Write-Output ('FORCE ' + $tag) } else { Write-Output ('NOOP ' + $tag) }; exit 0 }
>>"%NOOP_HELPER%" echo try { $iC = $iN.Split('-')[0]; $lC = $lN.Split('-')[0]; if ($iC -notmatch '\.') { $iC = $iC + '.0' }; if ($lC -notmatch '\.') { $lC = $lC + '.0' }; if ([version]$iC -gt [version]$lC) { Write-Output ('NEWER ' + $tag); exit 0 } } catch {}
>>"%NOOP_HELPER%" echo Write-Output ('UPGRADE ' + $tag)

for /f "usebackq tokens=1,2 delims= " %%a in (`powershell -NoProfile -ExecutionPolicy Bypass -File "%NOOP_HELPER%" 2^>nul`) do (
    set "ACTION=%%a"
    set "LATEST_VERSION=%%b"
)
del /q "%NOOP_HELPER%" 2>nul
set "NOOP_HELPER="
set "DBMCP_OLD_VERSION="
set "DBMCP_FORCE="
set "DBMCP_REPO="

if /i "%ACTION%"=="NOOP" (
    echo.
    echo Already on latest version ^(%LATEST_VERSION%^). Nothing to do.
    exit /b 0
)
if /i "%ACTION%"=="FORCE" (
    set "IS_REINSTALL=1"
    echo FORCE_INSTALL is set - reinstalling %LATEST_VERSION%
)
if /i "%ACTION%"=="NEWER" (
    echo warning: installed version %OLD_VERSION% is newer than the latest release %LATEST_VERSION%.
    echo          proceeding will replace it with the older release.
)
if /i "%ACTION%"=="UPGRADE" (
    echo Upgrading to %LATEST_VERSION%
)
if "%ACTION%"=="" (
    echo Upgrading dbmcp at %BIN_DIR% ^(current: %OLD_VERSION%^)
)

:prepare_install
REM Create install directory if needed
if not exist "%BIN_DIR%" mkdir "%BIN_DIR%"

REM Create temp directory (double %RANDOM% for better collision resistance)
set "TMPDIR=%TEMP%\dbmcp-install-%RANDOM%%RANDOM%"
mkdir "%TMPDIR%"

echo Downloading %ASSET%...

REM Download
curl -fsSL "%URL%" -o "%TMPDIR%\%ASSET%"
if %errorlevel% neq 0 (
    echo error: download failed - check your network connection or try again later
    echo   URL: %URL%
    goto :cleanup_fail
)

REM Extract via PowerShell (available on all Win10+ systems)
powershell -NoProfile -ExecutionPolicy Bypass -Command "Expand-Archive -Path '%TMPDIR%\%ASSET%' -DestinationPath '%TMPDIR%\extracted' -Force" 2>nul
if %errorlevel% neq 0 (
    echo error: failed to extract archive
    goto :cleanup_fail
)

REM Check binary exists
if not exist "%TMPDIR%\extracted\%BINARY_NAME%.exe" (
    echo error: binary '%BINARY_NAME%.exe' not found in archive
    goto :cleanup_fail
)

REM Install
copy /y "%TMPDIR%\extracted\%BINARY_NAME%.exe" "%BIN_DIR%\%BINARY_NAME%.exe" >nul
if %errorlevel% neq 0 (
    echo error: failed to install binary to %BIN_DIR%
    echo   Is an existing dbmcp process running?
    goto :cleanup_fail
)

REM Verify
set "INSTALLED_VERSION="
for /f "delims=" %%v in ('""%BIN_DIR%\%BINARY_NAME%.exe" --version" 2^>nul') do set "INSTALLED_VERSION=%%v"
if not defined INSTALLED_VERSION (
    echo error: installation verification failed
    goto :cleanup_fail
)

echo.
if %IS_REINSTALL% equ 1 (
    echo Successfully reinstalled dbmcp!
    echo   Version: %INSTALLED_VERSION%
) else (
    if %IS_UPGRADE% equ 1 (
        echo Successfully upgraded dbmcp!
        echo   %OLD_VERSION% -^> %INSTALLED_VERSION%
    ) else (
        echo Successfully installed dbmcp!
        echo   Version: %INSTALLED_VERSION%
    )
)
echo   Location: %BIN_DIR%\%BINARY_NAME%.exe

REM Add to PATH via PowerShell (uses .NET API — no 1024 char limit, preserves REG_EXPAND_SZ,
REM exact-match check to avoid substring false positives)
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "$bin='%BIN_DIR%';" ^
    "$cur=[Environment]::GetEnvironmentVariable('PATH','User');" ^
    "$found=$false;" ^
    "if ($cur) { foreach ($e in ($cur -split ';')) { if ($e -ieq $bin) { $found=$true; break } } }" ^
    "if (-not $found) {" ^
    "  Write-Host ''; Write-Host ('warning: ' + $bin + ' is not in your PATH') -ForegroundColor Yellow; Write-Host '';" ^
    "  if ($cur) { $new = $bin + ';' + $cur } else { $new = $bin };" ^
    "  [Environment]::SetEnvironmentVariable('PATH', $new, 'User');" ^
    "  Write-Host ('  Added ' + $bin + ' to your user PATH.') -ForegroundColor Green;" ^
    "  Write-Host '  Restart your terminal for the change to take effect.'; Write-Host ''" ^
    "}"

goto :cleanup_ok

:cleanup_fail
if exist "%TMPDIR%" rmdir /s /q "%TMPDIR%" 2>nul
exit /b 1

:cleanup_ok
if exist "%TMPDIR%" rmdir /s /q "%TMPDIR%" 2>nul
exit /b 0
