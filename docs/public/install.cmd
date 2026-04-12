@echo off
setlocal enabledelayedexpansion

REM Install script for database-mcp (Windows CMD)
REM Usage: curl -fsSL https://database.haymon.ai/install.cmd -o install.cmd && install.cmd && del install.cmd
REM
REM Environment variables:
REM   INSTALL_DIR - Override the install directory

set "BINARY_NAME=database-mcp"
set "REPO=haymon-ai/database"
set "TARGET=x86_64-pc-windows-msvc"
set "ASSET=%BINARY_NAME%-%TARGET%.zip"
set "BASE_URL=https://github.com/%REPO%/releases/latest/download"
set "URL=%BASE_URL%/%ASSET%"

echo.
echo Installing database-mcp...
echo.

REM Resolve install directory
set "BIN_DIR="
set "IS_UPGRADE=0"
set "OLD_VERSION="

REM Priority 1: INSTALL_DIR env var
if defined INSTALL_DIR (
    set "BIN_DIR=%INSTALL_DIR%"
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
for /f "delims=" %%v in ('""%EXISTING%" --version" 2^>nul') do set "OLD_VERSION=%%v"
goto :resolved

:default_dir
REM Priority 3: Default location
set "BIN_DIR=%LOCALAPPDATA%\Programs\database-mcp"

:resolved
if %IS_UPGRADE% equ 1 (
    echo Upgrading database-mcp at %BIN_DIR% ^(current: %OLD_VERSION%^)
) else (
    echo Install directory: %BIN_DIR%
)

REM Create install directory if needed
if not exist "%BIN_DIR%" mkdir "%BIN_DIR%"

REM Create temp directory (double %RANDOM% for better collision resistance)
set "TMPDIR=%TEMP%\database-mcp-install-%RANDOM%%RANDOM%"
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
    echo   Is an existing database-mcp process running?
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
if %IS_UPGRADE% equ 1 (
    echo Successfully upgraded database-mcp!
    echo   %OLD_VERSION% -^> %INSTALLED_VERSION%
) else (
    echo Successfully installed database-mcp!
    echo   Version: %INSTALLED_VERSION%
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
