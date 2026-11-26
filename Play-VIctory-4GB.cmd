@echo off
setlocal EnableExtensions DisableDelayedExpansion
if not defined VICTORY_GAME_DIR set "VICTORY_GAME_DIR=%~dp0..\game"
set "runner=%~dp0target\release\victory.exe"
if not exist "%runner%" set "runner=%~dp0victory-3d-rtx-pareto-residency.exe"

set "VICTORY_CACHE_DIR=%LOCALAPPDATA%\VIctory\cache-pareto"
set "VICTORY_AMBIENT_AIR=1.5"
set "VICTORY_AMBIENT=0.8"
set "VICTORY_TEXTURE_NO_WAIT=1"
set "VICTORY_RELAY=2"
set "VICTORY_WIDE_BACK=1"
set "VICTORY_CARD_REHASH_RUNS=1"
set "VICTORY_JOIN_COPIES=1"
set "VICTORY_INDIRECT_WINDOW=1"
set "VICTORY_RELAY_COPY=1"
set "VICTORY_CARD_STEPS=2"
set "VICTORY_INDIRECT_BATCH=2"
set "VICTORY_MAP_THREADS=1"
set "VICTORY_CARD_AWAKE=1"
set "VICTORY_SHOW_AFTER=1"
if not defined VICTORY_THIN set "VICTORY_THIN=0"
if not defined VICTORY_JOIN_PARTS set "VICTORY_JOIN_PARTS=0"
if not defined VICTORY_LIBRARY_STORE_LATER set "VICTORY_LIBRARY_STORE_LATER=1"
if not defined VICTORY_PIPELINE_WAIT_MS set "VICTORY_PIPELINE_WAIT_MS=0"
if not defined VICTORY_PORT_CONSTANTS set "VICTORY_PORT_CONSTANTS=0"
if not defined VICTORY_PORT_CBV set "VICTORY_PORT_CBV=0"
if not defined VICTORY_UPLOAD_AHEAD set "VICTORY_UPLOAD_AHEAD=1"
set "VICTORY_LEVELS_ON_DEMAND=1"

:: 4 GB VRAM tuning: reduce card-buffer thrashing and driver background recompile threads
set "VICTORY_CARD_BUFFERS_MIN_KB=4096"
set "VICTORY_CARD_BUFFERS_MB=256"
set "VICTORY_D3D12_BACKGROUND=0"
if not defined VICTORY_LOG set "VICTORY_LOG=warn"

"%runner%" run "%VICTORY_GAME_DIR%\game_scarlett_master.exe" --console series-s --execute
exit /b %errorlevel%
