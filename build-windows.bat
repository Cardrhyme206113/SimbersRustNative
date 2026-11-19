@echo off
setlocal
cd /d "%~dp0"
echo Compiling native Rust Windows app...
cargo test --release || exit /b 1
cargo build --release || exit /b 1
echo.
echo EXE is at target\release\simbers-native.exe
pause
