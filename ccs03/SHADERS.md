# A report on every shader

`ccs03/shader-report.ps1` writes a text report on every shader program of
the title: whether our translator handles it, in which form, whether
`dxil.dll`'s validator and this machine's driver take what we make of it,
and, given the log of a run, what happened to the shaders that run used.
It is meant for finding what goes wrong with shaders on a given card and
for sharing that with others.

The report holds the programs' names (`CS_DefragHeap_..._Wrapped`), their
hashes and our own messages. It holds no shader code, so it can be shared.
The shaders themselves are the game's files: never put a `.cdxil` or
`.dxil` file, a dump or a shader pack in this repository or online
(`docs/asset-policy.md`).

## Running it

From the kit, after a build (`victory.exe` and `victory-diag.exe` next to
each other):

```
powershell -ExecutionPolicy Bypass -File shader-report.ps1 -Victory victory.exe -Title <game folder>\game_scarlett_master.exe -Out shader-report
```

Without `-Title` the script takes `GAME_EXE`, else `VICTORY_GAME_DIR`, else
`game_dir` from `victory.toml`. It runs

```
victory translate --validate --pipelines --list <game folder>\shaders.rpf <game folder>\game_scarlett_master.exe
```

which takes every shader container out of the shader pack and the
executable (about 26,000 programs), translates each, validates the DXIL
with `dxil.dll` (`VICTORY_DXIL`, else the newest Windows SDK's) and asks
the driver for a pipeline with it. That took 21 minutes on a laptop with a
Ryzen 9 7845HX and an RTX 4060 Laptop GPU (`-Jobs 16`).

To add a run: play with `VICTORY_LOG=info` (as `9-meet.bat` does), close
the game, and pass its log with `-RunLog <log>`. The script then also
translates as that run did (its host wave size and view heap, from the
log's `host wave lanes` and `host view heaps hold` lines). With
`-RunOnly` it reads only the log, against the `programs.tsv` of an earlier
pass in the same `-Out` folder. With `-Reuse` it makes the report again from the
`translate.txt` of an earlier pass in that folder, in seconds, without
translating anything.

## The files

| File | What it holds |
|------|---------------|
| `summary.txt` | the overview below; start here |
| `programs.tsv` | one line per program: stage, name, hash, `ok` or `problem`, the form it was taken in, whether the console ran it in 64-lane waves, the problems, the file it came from |
| `translate.txt` | `victory translate`'s own output: one line per program and outcome, then totals and the failures by reason |
| `census.tsv` | per emitted program, its console instructions and host instructions by kind |
| `run-shaders.tsv` | with a run: every shader the run translated, with its name and how it did in the full pass |
| `run-problems.txt` | with a run: the run's log lines about shaders and pipelines that went wrong, hashes followed by names |

## Reading `summary.txt`

**Every program of the title.** First `victory translate`'s totals, then a
table per hardware stage:

- `ok`: translated, validated and made into a pipeline by the driver;
- `validator`: `dxil.dll` refuses the DXIL we made (a translator bug);
- `driver`: the validator takes it but the driver refuses the pipeline
  (on NVIDIA this is mostly a program that still needs 64-lane waves);
- `not translated`: the translator stopped before DXIL; the reason says
  where (`unsupported:`, `structure:`, `resources:`, `I/O:`, `emit:`).

Geometry programs that read an address no root parameter holds are not
lowered by `translate` at all; they show up under `not translated`.

**Console wave64 compute programs.** The console runs many compute
programs in 64-lane waves; NVIDIA cards have 32-lane waves. Such a program
is taken in the first form that runs on 32 lanes:

- `as written`: nothing in it depends on the wave size;
- `without early-outs`, `without skips`, `without skips, scalars
  followed`: exact forms, the program's own work rearranged so that the
  cross-lane steps happen where every lane runs;
- `wave-wide branches`: exact, with group barriers inside branches (only
  for the programs the runtime's list allows);
- `local waves (idiom)`: exact, the cross-lane steps are an allocation
  idiom each 32-lane half can do on its own;
- `local waves (APPROXIMATE...)`: each 32-lane half gives its own result
  where the console gave one for 64 lanes. This runs, but can be wrong on
  screen (blocky shadows, hair, black tiles); the names are listed;
- `wave64 kept`: no form worked, the program keeps its 64-lane waves and
  the driver refuses it. These are listed too.

The runtime picks forms the same way, with one difference: the programs
on its wave-wide list (`shader_cache.rs`) may take `wave-wide branches`
there, but not in this pass.

**Programs with a problem.** Every reason with how many programs have it
and the first names. All of them are in `programs.tsv`.

**The run.** How many shaders the run translated, by stage, and how many
of those the full pass found in the title's files (the hashes are the same
in the log and the pass). Then counts of what went wrong in the run:

- `shader does not translate`: the run dropped that shader's draws;
- `host pipeline refused`: the driver refused a pipeline of the run (with
  its real render targets and states, which the full pass does not have);
- `host root signature refused`, `no host pipeline`: other reasons a
  pipeline was not made;
- `draws or dispatches dropped`: per-frame counts of work left out, with
  the reason (most are the ray-tracing structure builder, which is off);
- `device removed or lost`: the card was lost.

## Sending it to someone

Send the `shader-report` folder, or at least `summary.txt` and
`programs.tsv`. If they want to look at one program in detail, they need
the shader itself: that is a file of the game, so they take it from their
own copy (`VICTORY_SHADER_SAVE=<folder>` saves the ones a run uses, named
by the same hash).
