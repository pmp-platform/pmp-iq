@echo off
REM Bring up the docker compose stack. Optional first arg: a compose profile.
setlocal
cd /d "%~dp0.."

set "PROFILE=%~1"
call "%~dp0down.bat" %PROFILE%
if "%PROFILE%"=="" (
  docker compose up
) else (
  docker compose --profile %PROFILE% up
)
endlocal
