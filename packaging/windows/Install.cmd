@echo off
setlocal
set "SRC=%~dp0"
set "DEST=%LOCALAPPDATA%\SRL"
echo Installing SRL to %DEST% ...
if not exist "%DEST%" mkdir "%DEST%"
copy /Y "%SRC%sr-launcher.exe" "%DEST%\sr-launcher.exe" >nul
copy /Y "%SRC%srl.ico" "%DEST%\srl.ico" >nul
if exist "%SRC%frpc" xcopy /E /I /Y "%SRC%frpc" "%DEST%\frpc" >nul
if exist "%SRC%THIRD_PARTY_NOTICES.md" copy /Y "%SRC%THIRD_PARTY_NOTICES.md" "%DEST%\THIRD_PARTY_NOTICES.md" >nul
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ws=New-Object -ComObject WScript.Shell; $desktop=[Environment]::GetFolderPath('Desktop'); $s=$ws.CreateShortcut($desktop+'\SRL.lnk'); $s.TargetPath='%DEST%\sr-launcher.exe'; $s.WorkingDirectory='%DEST%'; $s.IconLocation='%DEST%\srl.ico'; $s.Save(); $start=[Environment]::GetFolderPath('StartMenu')+'\Programs'; $s2=$ws.CreateShortcut($start+'\SRL.lnk'); $s2.TargetPath='%DEST%\sr-launcher.exe'; $s2.WorkingDirectory='%DEST%'; $s2.IconLocation='%DEST%\srl.ico'; $s2.Save()"
if errorlevel 1 (
  echo Failed to create shortcuts. You can run sr-launcher.exe directly.
  pause
  exit /b 1
)
echo Installed. Starting SRL...
start "" "%DEST%\sr-launcher.exe"
