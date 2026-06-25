@echo off
REM Tear down the docker compose stack. Optional first arg: a compose profile.
setlocal
cd /d "%~dp0.."

set "PROFILE=%~1"
if "%PROFILE%"=="" (
  docker compose rm -f --all
) else (
  docker compose --profile %PROFILE% rm -f --all
)
endlocal
