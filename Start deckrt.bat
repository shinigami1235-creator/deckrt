@echo off
rem Starts deckrt. Put this file next to deckrt.exe, or in the folder you cloned the source to.
rem
rem If no window opens: deckrt tries DirectX 12, Vulkan, OpenGL and then Windows' software
rem renderer on its own, and writes what happened to %APPDATA%\deckrt\logs\startup.log.
rem To force one of them, remove the "rem" from one of these two lines:
rem set WGPU_BACKEND=gl
rem set DECKRT_SOFTWARE_GPU=1
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
