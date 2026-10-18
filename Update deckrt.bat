@echo off
rem Downloads the newest deckrt test build from GitHub and runs its installer.
rem It replaces the installed deckrt. Your decks and other files are not touched.
title Update deckrt
echo Downloading the newest deckrt test build...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ProgressPreference='SilentlyContinue'; try { Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/shinigami1235-creator/deckrt/releases/download/latest-build/deckrt-windows-x64.msi' -OutFile (Join-Path $env:TEMP 'deckrt-windows-x64.msi'); exit 0 } catch { Write-Host $_.Exception.Message; exit 1 }"
if errorlevel 1 (
  echo.
  echo The download failed. Check the internet connection and run this file again.
  pause
  exit /b 1
)
start "" msiexec /i "%TEMP%\deckrt-windows-x64.msi"
