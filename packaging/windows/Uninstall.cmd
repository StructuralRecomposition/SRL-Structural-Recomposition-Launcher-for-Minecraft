@echo off
setlocal
set "DEST=%LOCALAPPDATA%\SRL"
echo Removing SRL program files...
if exist "%DEST%" rmdir /S /Q "%DEST%"
del "%USERPROFILE%\Desktop\SRL.lnk" >nul 2>nul
del "%APPDATA%\Microsoft\Windows\Start Menu\Programs\SRL.lnk" >nul 2>nul
echo Done. Your launcher data in %APPDATA% is kept.
pause
