@echo off
setlocal
cd /d "%~dp0"
echo.
echo SIMBERS NATIVE - WINDOWS x64 BUILD
echo ----------------------------------
where cargo >nul 2>&1
if errorlevel 1 (
  echo ERROR: Rust/Cargo is missing from PATH.
  echo Install Rust at https://rustup.rs/ and Visual Studio Build Tools with
  echo Desktop development with C++ ^(MSVC^) installed.
  echo Then open a NEW terminal and rerun this file.
  > "%~dp0build-log.txt" echo ERROR: Cargo not found in PATH. Install Rust and the MSVC C++ toolchain.
  pause
  exit /b 1
)
echo Building release exe. First build downloads Rust crates onto this PC.
echo Build output will be saved to build-log.txt.
echo.
cargo build --release > "%~dp0build-log.txt" 2>&1
if errorlevel 1 (
  echo BUILD FAILED. The last 45 log lines are below:
  powershell -NoProfile -Command "Get-Content -LiteralPath '%~dp0build-log.txt' -Tail 45"
  echo.
  echo Upload build-log.txt to ChatGPT so compiler errors can be fixed.
  pause
  exit /b 1
)
if not exist "target\release\simbers-native.exe" (
  echo ERROR: Build completed but EXE is missing.
  pause
  exit /b 1
)
copy /Y "target\release\simbers-native.exe" "%~dp0SimbersNative.exe" >nul
if errorlevel 1 (
  echo ERROR: Failed to copy EXE. Try target\release\simbers-native.exe
  pause
  exit /b 1
)
echo.
echo SUCCESS: %~dp0SimbersNative.exe
echo This file is the compiled application; it does not download source crates at runtime.
pause
