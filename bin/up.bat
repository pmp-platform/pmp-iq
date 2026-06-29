@echo off
REM Bring up a docker compose topology. Optional first arg:
REM   single | distributed | <profile> | (empty)
setlocal
cd /d "%~dp0.."

set "TARGET=%~1"
call "%~dp0down.bat" %TARGET%
if "%TARGET%"=="single" (
  docker compose -f docker-compose.single.yml up --build
) else if "%TARGET%"=="distributed" (
  docker compose -f docker-compose.distributed.yml up --build
) else if "%TARGET%"=="" (
  docker compose up
) else (
  docker compose --profile %TARGET% up
)
endlocal
