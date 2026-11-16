@echo off
rem Builds GMAudio into an installer and a plain .exe.
rem Needs Rust, Node.js and the Visual Studio C++ build tools (see README).
cd /d "%~dp0"
call npm.cmd install || goto :failed
call npm.cmd run tauri build || goto :failed
echo.
echo Done.
echo   Installer: target\release\bundle\nsis
echo   Plain exe: target\release\gmaudio.exe
start "" "target\release\bundle\nsis"
pause
exit /b 0
:failed
echo.
echo The build failed. The messages above say why.
pause
exit /b 1
