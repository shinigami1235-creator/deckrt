@echo off
rem Starts deckrt. Put this file next to deckrt.exe, or in the folder you cloned the source to.
setlocal
cd /d "%~dp0"
if exist "deckrt.exe" (
  start "" "deckrt.exe" %*
  goto :eof
)
if exist "target\release\deckrt.exe" (
  start "" "target\release\deckrt.exe" %*
  goto :eof
)
where cargo >nul 2>nul
if errorlevel 1 (
  echo deckrt.exe is not here and Rust is not installed. Install Rust from https://rustup.rs and run Build deckrt.bat, or download the zip from the GitHub Actions build.
  pause
  goto :eof
)
echo Building deckrt. The first build takes a while.
cargo build --release -p deckcraft
if errorlevel 1 (
  echo The build failed. The lines above say why.
  pause
  goto :eof
)
start "" "target\release\deckrt.exe" %*
