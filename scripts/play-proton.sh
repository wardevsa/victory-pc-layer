#!/usr/bin/env bash
# Starts the title on Linux under Proton (umu-run) or Wine, with the settings
# of Play-VIctory.cmd's pareto-residency selector. Untested with the title:
# see docs/linux.md.
#
#   VICTORY_GAME_DIR=~/Games/MyDump scripts/play-proton.sh
#
# Any VICTORY_* variable already set wins over the defaults below.
# VICTORY_EXE: another victory.exe. VICTORY_WINE=1: plain wine, not umu-run.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
game_dir=${VICTORY_GAME_DIR:?set VICTORY_GAME_DIR to your own dump folder (it holds game_scarlett_master.exe)}
game_dir=$(realpath "$game_dir")
title="$game_dir/game_scarlett_master.exe"
runner=${VICTORY_EXE:-$root/target/x86_64-pc-windows-msvc/release/victory.exe}

[[ -f $title ]] || { echo "No $title. Set VICTORY_GAME_DIR to your own dump folder." >&2; exit 2; }
[[ -f $runner ]] || {
    echo "No $runner. Build it: cargo xwin build --release --locked -p victory --target x86_64-pc-windows-msvc" >&2
    exit 2
}

# A Linux path as Wine sees it: drive Z: is the root of the file system.
win() { local p; p=$(realpath -m "$1"); printf 'Z:%s' "${p//\//\\}"; }

# Kept outside the Wine prefix (and outside this checkout, which the tools
# refuse to write game-derived data into), so a new prefix keeps both.
cache=${XDG_CACHE_HOME:-$HOME/.cache}/victory/cache-pareto
data=${XDG_DATA_HOME:-$HOME/.local/share}/victory/data
mkdir -p "$cache" "$data"

export VICTORY_GAME_DIR="$(win "$game_dir")"
: "${VICTORY_CACHE_DIR:=$(win "$cache")}"
: "${VICTORY_DATA_DIR:=$(win "$data")}"
export VICTORY_CACHE_DIR VICTORY_DATA_DIR
# vkd3d-proton's own cache (translated shaders), beside ours: it would go in
# whatever folder the script was started from.
mkdir -p "$cache/../vkd3d"
: "${VKD3D_SHADER_CACHE_PATH:=$(win "$cache/../vkd3d")}"
export VKD3D_SHADER_CACHE_PATH
# Proton does not pass the title's stderr through: the log goes to a file.
log=${XDG_STATE_HOME:-$HOME/.local/state}/victory/victory.log
mkdir -p "$(dirname "$log")"
: "${VICTORY_LOG_FILE:=$(win "$log")}"
export VICTORY_LOG_FILE

# Play-VIctory.cmd's defaults (pareto-residency, as of upstream 472e841),
# each only if not set; and for Wine, VICTORY_COMMITTED_QUERY=1: there
# QueryWorkingSetEx holds Wine's memory lock while it reads /proc/self/pagemap
# (1.5 s of every 8 in the world), where VirtualQuery is cheap.
# VICTORY_AMBIENT=1 (the estimate from the frame) where Play-VIctory.cmd has
# constant:0.8: the prologue's rooms were dark red and its yard milky white
# with the constant, lit by their lamps and the sunset with the estimate.
# VICTORY_HAIR=1: upstream leaves the hair strands' passes out as broken; on
# an RTX 5060 under Proton they draw right (101 ms a frame with, 119 without).
# VICTORY_GUARD_REST=50000 (50 ms, 3 by default): a change of protection costs
# far more under Wine (Wine's lock, every core's TLB); a block the title just
# wrote rests longer: blocks made read-only a frame 861 -> 461, faults 478 -> 364.
for setting in \
    VICTORY_AMBIENT_AIR=1.5 VICTORY_TEXTURE_NO_WAIT=1 VICTORY_RELAY=2 VICTORY_WIDE_BACK=1 \
    VICTORY_CARD_REHASH_RUNS=1 VICTORY_JOIN_COPIES=1 VICTORY_INDIRECT_WINDOW=1 VICTORY_RELAY_COPY=1 \
    VICTORY_CARD_STEPS=2 VICTORY_INDIRECT_BATCH=2 VICTORY_MAP_THREADS=1 VICTORY_CARD_AWAKE=1 \
    VICTORY_SHOW_AFTER=1 VICTORY_AMBIENT=1 VICTORY_THIN=0 VICTORY_JOIN_PARTS=0 \
    VICTORY_TEXTURE_GUARD=0 VICTORY_PORT_CONSTANTS=1 VICTORY_PORT_CBV=1 \
    VICTORY_UPLOAD_AHEAD=1 VICTORY_LEVELS_ON_DEMAND=1 VICTORY_CARD_BUFFERS_MIN_KB=64 \
    VICTORY_COMMITTED_QUERY=1 VICTORY_HAIR=1 VICTORY_GUARD_REST=50000; do
    name=${setting%%=*}
    [[ -n ${!name:-} ]] || export "$setting"
done
[[ -n ${VICTORY_LOG:-} || -n ${RUST_LOG:-} ]] || export VICTORY_LOG=warn
# On an NVIDIA card the title's own upscaler (VICTORY_UPSCALER=6), not
# FidelityFX's (7, the title's choice on Series S): FidelityFX's is a network
# of 8-bit dot products, and its eight largest layers took 120 of the 200 ms
# of an RTX 5060's frame (the prologue: 207 -> 115 ms a frame). It also
# avoids its divide by zero at output sizes other than 1920x1080.
if [[ -z ${VICTORY_UPSCALER:-} && -e /proc/driver/nvidia/version ]]; then
    export VICTORY_UPSCALER=6
fi

# VICTORY_CONSOLE: series-x or series-s (series-s fits an 8 GB card: docs/profiling.md).
# Wine hands threads the priorities asked for (the title's ahead of our
# pipeline workers and streaming) only if it may raise priority: a hard
# RLIMIT_NICE above 20. Without, every thread is at the same niceness, on
# eight cores busy with some 140 threads (prologue: 120 -> 105 ms a frame,
# p90 140 -> 114, with it). Fedora's gamemode group is allowed -10.
if (( $(ulimit -He) <= 20 )); then
    echo "Thread priorities off: RLIMIT_NICE is $(ulimit -He). For about 15 ms a frame less:" >&2
    echo "  sudo usermod -aG gamemode $USER   (then log in again)" >&2
fi

args=(run "$(win "$title")" --console "${VICTORY_CONSOLE:-series-x}" --execute)
if [[ ${VICTORY_WINE:-0} != 1 ]] && command -v umu-run >/dev/null; then
    # A GE-Proton whose wineserver keeps views in a tree (docs/linux.md): with
    # the stock one every map and unmap of the title's ~30,000 page views
    # searches them all while Wine's memory lock is held. Used if installed.
    patched=${STEAM_COMPAT_TOOLS:-$HOME/.local/share/Steam/compatibilitytools.d}/GE-Proton11-14-victory
    [[ -n ${PROTONPATH:-} || ! -x $patched/proton ]] || PROTONPATH=$patched
    export GAMEID=${GAMEID:-umu-default} PROTONPATH=${PROTONPATH:-GE-Proton}
    exec umu-run "$runner" "${args[@]}"
fi
command -v wine >/dev/null || { echo "Install umu-launcher (or wine)." >&2; exit 2; }
echo "umu-run not found (or VICTORY_WINE=1): plain wine, without Proton's vkd3d-proton unless the prefix has it." >&2
exec wine "$runner" "${args[@]}"
