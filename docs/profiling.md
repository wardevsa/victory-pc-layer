# Finding where a frame goes on a smaller PC

The figures in [findings.md](findings.md) are from an i9-12900K (24 threads) with an
RX 7900 XTX (24 GB). On a PC closer to a Series X, for example eight cores and an
8 GB card, the walls move, and what to work on next should come from numbers taken
there. This is the order to take them in. Every tool named here is already in the
diagnostics build:

    cargo build --locked --profile diag -p victory --features diagnostics

## 1. Fit first: memory

A run that does not fit spends its time paging, and no other figure means anything.

- **Host RAM.** `VICTORY_MEMORY_MAP` (on in the diagnostics build; `=0` turns it off)
  lists the private memory by owner in the log: lists' allocators, windows of title
  memory, uploads. The 7900 XTX runs held 40-46 GB. Idle host lists now go after
  resting (`crate::list::let_go_idle`, `VICTORY_LIST_POOL_AGE=0`: kept as before).
  Compare the allocator figure with that switch on and off.
- **Card memory.** On an 8 GB card use `--console series-s` (the `pareto-residency`
  launcher does): as a Series X the title plans for 10 GB of GPU-optimal memory,
  which cannot fit. Watch the adapter's use against its budget in Task Manager
  ("Dedicated GPU memory") or PresentMon. Use above the budget means WDDM is moving
  resources to system memory over PCIe, and frame times then measure the bus.

## 2. CPU or card?

The log's frame line, once a second, and the F3 panel (`VICTORY_PANEL`) split a
frame into the preparing thread, the card's busy time and the waits. On the 7900 XTX
the preparing thread was the wall (48 ms of 55), with the card close behind (41 ms).

- **The preparing thread** runs on one core. Fewer cores do not slow it down, but
  busier ones do: on eight cores our pools take processors from it and from the
  title's own threads. The log names the number of pipeline workers and list threads
  at startup. `VICTORY_THIN_THREADS` now defaults to a third of the
  logical processors (5 on 16). Try 4 and 6 and compare the frame line.
- **The card**: go on to the replay.

## 3. The card: our passes against the title's

Record one frame (`docs/replay.md`). Indoors in the prologue is easiest: recording in
the open world can trip the title's own watchdog. Then:

    victory replay <folder> --loops 5 --timing detailed
    victory replay <folder> --loops 5 --compare --skip-hash-passes
    victory replay <folder> --loops 5 --compare --batch-clamps 2
    victory replay <folder> --loops 5 --compare --buffers vram

- `--timing detailed` names the costliest pipelines. Ours are named by their own
  DXIL hash; the title's by the names the runtime logs.
- `--skip-hash-passes` shows what our compare and rehash passes over card copies
  cost. On the 7900 XTX, narrowing them took the card from 79 ms to 66
  (findings.md).
- `--batch-clamps` shows the indirect-argument clamps.
- `--buffers vram`/`sysmem` shows what placing buffers costs on this card. It matters
  more with 8 GB.

A capture holds the card's memory the frame reads (8 GB in the made-up test frame).
On an 8 GB card, run with `--no-restore` if a replay will not start.

## 4. Writing it down

So that a change can be judged on another machine, note the following for each run:
CPU, GPU, VRAM, RAM, driver, console profile, launcher selector, and the scene. Also
note the medians of the frame line, the preparing thread and the card, and the peak
private memory and adapter memory. Read differences under about 5 % as nothing:
scenes differ from run to run.
