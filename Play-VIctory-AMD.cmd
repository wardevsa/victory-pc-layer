@echo off
setlocal EnableExtensions DisableDelayedExpansion
rem High-performance profile for AMD Radeon cards (RX 7000+, 16 GB+ VRAM).
rem Enables all renderer ports that the leading community forks run with.
rem For NVIDIA GPUs or 8 GB VRAM cards, use Play-VIctory.cmd instead.
if not defined VICTORY_GAME_DIR (
  echo Set VICTORY_GAME_DIR to the folder containing your title executable. 1>&2
  exit /b 2
)
if not exist "%VICTORY_GAME_DIR%\game_scarlett_master.exe" (
  echo game_scarlett_master.exe not found in VICTORY_GAME_DIR: "%VICTORY_GAME_DIR%" 1>&2
  exit /b 2
)
set "runner=%~dp0victory-3d-rtx-pareto-residency.exe"
if not exist "%runner%" set "runner=%~dp0target\release\victory.exe"
if not exist "%runner%" (
  echo Extract the Windows release or run cargo build --release --locked -p victory. 1>&2
  exit /b 2
)
if not defined VICTORY_AMBIENT_AIR set "VICTORY_AMBIENT_AIR=1.5"
if not defined VICTORY_CACHE_DIR set "VICTORY_CACHE_DIR=%LOCALAPPDATA%\VIctory\cache-pareto"
if not defined VICTORY_TEXTURE_NO_WAIT set "VICTORY_TEXTURE_NO_WAIT=1"
if not defined VICTORY_RELAY set "VICTORY_RELAY=2"
if not defined VICTORY_WIDE_BACK set "VICTORY_WIDE_BACK=1"
if not defined VICTORY_CARD_REHASH_RUNS set "VICTORY_CARD_REHASH_RUNS=1"
if not defined VICTORY_JOIN_COPIES set "VICTORY_JOIN_COPIES=1"
if not defined VICTORY_INDIRECT_WINDOW set "VICTORY_INDIRECT_WINDOW=1"
if not defined VICTORY_RELAY_COPY set "VICTORY_RELAY_COPY=1"
if not defined VICTORY_CARD_STEPS set "VICTORY_CARD_STEPS=2"
if not defined VICTORY_INDIRECT_BATCH set "VICTORY_INDIRECT_BATCH=2"
if not defined VICTORY_MAP_THREADS set "VICTORY_MAP_THREADS=1"
if not defined VICTORY_CARD_AWAKE set "VICTORY_CARD_AWAKE=1"
if not defined VICTORY_SHOW_AFTER set "VICTORY_SHOW_AFTER=1"
if not defined VICTORY_AMBIENT set "VICTORY_AMBIENT=constant:0.8"
if not defined VICTORY_TEXTURE_GUARD set "VICTORY_TEXTURE_GUARD=0"

rem Async paths: THIN and JOIN_PARTS tested stable on AMD.
if not defined VICTORY_THIN set "VICTORY_THIN=1"
if not defined VICTORY_JOIN_PARTS set "VICTORY_JOIN_PARTS=8"
if not defined VICTORY_UPLOAD_AHEAD set "VICTORY_UPLOAD_AHEAD=1"

rem All renderer ports on: the AMD profile from the community's top forks.
rem Port 1: title's constants heap read in place by the card (~6 ms saved).
if not defined VICTORY_PORT_CONSTANTS set "VICTORY_PORT_CONSTANTS=1"
rem Port 5: constant buffers read as raw bytes (~4.7 ms, ~22k fills eliminated).
if not defined VICTORY_PORT_CBV set "VICTORY_PORT_CBV=1"
rem Port 6: write-guard lock shared by faults and refreshes (~2 ms saved).
if not defined VICTORY_PORT_GUARD set "VICTORY_PORT_GUARD=1"
rem Port 7: journal notes split into 64 parts with their own locks (~1-3 ms).
if not defined VICTORY_PORT_JOURNAL set "VICTORY_PORT_JOURNAL=1"

if not defined VICTORY_LIBRARY_STORE_LATER set "VICTORY_LIBRARY_STORE_LATER=1"
if not defined VICTORY_PIPELINE_WAIT_MS set "VICTORY_PIPELINE_WAIT_MS=0"
if not defined VICTORY_LEVELS_ON_DEMAND set "VICTORY_LEVELS_ON_DEMAND=1"
if not defined VICTORY_CARD_BUFFERS_MIN_KB set "VICTORY_CARD_BUFFERS_MIN_KB=64"
if not defined VICTORY_LOG if not defined RUST_LOG set "VICTORY_LOG=warn"
"%runner%" run "%VICTORY_GAME_DIR%\game_scarlett_master.exe" --console series-x --execute
exit /b %errorlevel%
