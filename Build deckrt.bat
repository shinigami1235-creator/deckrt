@echo off
rem Builds deckrt.exe from this source folder. Needs Rust (https://rustup.rs) and the Visual Studio C++ build tools.
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust is not installed. Install it from https://rustup.rs, then run this file again.
  pause
  goto :eof
)
echo Building deckrt. The first build takes a while.
cargo build --release -p deckcraft -p deckcraft-cli
if errorlevel 1 (
  echo The build failed. The lines above say why.
  pause
  goto :eof
)
copy /y "target\release\deckrt.exe" "deckrt.exe" >nul
copy /y "target\release\deckcraft-cli.exe" "deckcraft-cli.exe" >nul
echo deckrt.exe is in this folder. Double-click Start deckrt.bat to open it.
pause
