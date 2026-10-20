@echo off
rem Downloads the newest deckrt test build from GitHub and runs its installer.
rem It replaces the installed deckrt. Your decks and other files are not touched.
title Update deckrt
rem The installer can't replace deckrt while it runs, so wait until it's closed.
:waitclosed
tasklist /FI "IMAGENAME eq deckrt.exe" 2>nul | find /I "deckrt.exe" >nul
if not errorlevel 1 (
  echo deckrt is open. Save your work, close deckrt, then press a key here.
  pause >nul
  goto waitclosed
)
echo Downloading the newest deckrt test build...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ProgressPreference='SilentlyContinue'; try { Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/shinigami1235-creator/deckrt/releases/download/latest-build/deckrt-windows-x64.msi' -OutFile (Join-Path $env:TEMP 'deckrt-windows-x64.msi'); exit 0 } catch { Write-Host $_.Exception.Message; exit 1 }"
if errorlevel 1 (
  echo.
  echo The download failed. Check the internet connection and run this file again.
  pause
  exit /b 1
)
start "" msiexec /i "%TEMP%\deckrt-windows-x64.msi"
