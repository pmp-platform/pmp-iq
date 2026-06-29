@echo off
REM Tear down a docker compose topology. Optional first arg:
REM   single | distributed | <profile> | (empty)
setlocal
cd /d "%~dp0.."

set "TARGET=%~1"
if "%TARGET%"=="single" (
  docker compose -f docker-compose.single.yml rm -f --all
) else if "%TARGET%"=="distributed" (
  docker compose -f docker-compose.distributed.yml rm -f --all
) else if "%TARGET%"=="" (
  docker compose rm -f --all
) else (
  docker compose --profile %TARGET% rm -f --all
)
endlocal
