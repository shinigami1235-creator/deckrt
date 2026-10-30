@echo off
setlocal
rem Downloads the newest deckrt test build from GitHub and installs it.
rem It replaces the installed deckrt. Your decks and other files are not touched.
title Update deckrt
rem The installer replaces this file, so it runs from a copy in the temp folder.
if /I not "%~nx0"=="deckrt-update.bat" (
  copy /y "%~f0" "%TEMP%\deckrt-update.bat" >nul
  "%TEMP%\deckrt-update.bat"
)
rem The installer can't replace deckrt while it runs, so wait until it's closed.
:waitclosed
tasklist /FI "IMAGENAME eq deckrt.exe" 2>nul | find /I "deckrt.exe" >nul
if not errorlevel 1 (
  echo deckrt is open. Save your work, close deckrt, then press a key here.
  pause >nul
  goto waitclosed
)
set "URL=https://github.com/shinigami1235-creator/deckrt/releases/download/latest-build/deckrt-windows-x64.msi"
set "MSI=%TEMP%\deckrt-update-%RANDOM%%RANDOM%.msi"
set "LOG=%TEMP%\deckrt-update.log"
set "BUILD="
for /f "usebackq delims=" %%S in (`powershell -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; try { $r = Invoke-RestMethod -UseBasicParsing -Headers @{'Cache-Control'='no-cache'} -Uri 'https://api.github.com/repos/shinigami1235-creator/deckrt/releases/tags/latest-build'; $r.target_commitish.Substring(0,9) + ', ' + ([datetime]$r.published_at).ToString('yyyy-MM-dd') } catch { '' }"`) do set "BUILD=%%S"
if defined BUILD (
  echo Downloading test build %BUILD%...
) else (
  echo Downloading the newest test build...
)
rem curl first (in Windows 10 and 11), PowerShell when it isn't there. Neither may use a copy
rem saved from an earlier download.
curl.exe -fsSL -H "Cache-Control: no-cache" -o "%MSI%" "%URL%" 2>nul
if errorlevel 1 (
  powershell -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; $ProgressPreference='SilentlyContinue'; try { Invoke-WebRequest -UseBasicParsing -Headers @{'Cache-Control'='no-cache'} -Uri $env:URL -OutFile $env:MSI; exit 0 } catch { Write-Host $_.Exception.Message; exit 1 }"
)
set "SIZE=0"
if exist "%MSI%" for %%A in ("%MSI%") do set "SIZE=%%~zA"
if %SIZE% LSS 10000000 (
  echo.
  echo The download failed. Check the internet connection and open Update deckrt again.
  if exist "%MSI%" del "%MSI%"
  pause
  exit /b 1
)
echo Installing. Click through the installer to finish.
start "" /wait msiexec /i "%MSI%" /l*v "%LOG%"
set "CODE=%ERRORLEVEL%"
del "%MSI%" 2>nul
if "%CODE%"=="0" goto done
if "%CODE%"=="3010" goto done
echo.
if "%CODE%"=="1602" (
  echo The update was cancelled. deckrt is as it was.
) else if "%CODE%"=="1618" (
  echo Another installation is running. Wait for it to finish, then open Update deckrt again.
) else (
  echo The installer stopped with error %CODE%. Its log is %LOG%
)
pause
exit /b 1
:done
echo.
if defined BUILD (
  echo deckrt is updated to test build %BUILD%.
) else (
  echo deckrt is updated.
)
timeout /t 8 >nul
exit /b 0
