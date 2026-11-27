# Buffer-copy admission on an 8 GB adapter

Measured on an RTX 4060 Laptop GPU (8 GB), driver 617.42, on AC power.
Both builds used revision `28ebb28`, the same warm cache, Series S,
`VICTORY_CARD_BUFFERS_MIN_KB=64`, the existing 512 MiB copy budget,
both buffer ports, and the same automatic input and launch profile.
No shader translation or shader skips changed in this comparison.

## Cause

Committed buffer copies are rounded to 64 KiB. The placement policy also
tested that padded allocation size against its minimum. Consequently,
thousands of 1–4 KiB title buffers qualified for local copies when the
minimum was 64 KiB. The copy quota filled before a repeatedly accessed
20.1 MiB neural-upscaling scratch buffer arrived. Allocation logs confirmed
the quota refusal; the bound scratch resource was CUSTOM/WRITE_BACK/L0
system memory, rather than a DEFAULT resource in local memory.

Correcting the minimum alone admitted more useful buffers, including a
128 KiB weight buffer, but other copies still filled the quota first.
Keeping bounded room for later large buffers admitted the scratch buffer.
The weight buffer could remain shared without the earlier slowdown.

## Policy

- Test the logical title length against the configured minimum. Continue
  charging the padded allocation size against the byte quota.
- Keep up to one eighth of that quota, capped at 64 MiB, for later buffers
  of at least 16 MiB. Large copies use any free ordinary room first and
  charge only the remaining bytes to the reserve. Small copies use ordinary
  room only. Both pools share the original total.
- Preserve a single oversized copy that fits the total, small explicit
  quotas, the VRAM-pressure check, and the existing shared-memory fallback.
- Account admission, allocation failure, and release consistently. Release
  the admission lock before calling the driver.
- Apply the same policy to alignment copies. Small shared alignment copies
  retain their existing 256-byte padding; local copies use 64 KiB padding
  and the existing upload/compare/writeback path.

## Measurements

Per-command timing disables asynchronous submission and is used only to
identify dispatch cost. The hottest upscaling dispatch fell from about
63 ms to 0.27–0.33 ms after its scratch buffer became GPU-local. Two other
expensive dispatches fell from about 40 ms to 0.30 ms and 29 ms to 0.62 ms.
The packed integer dot-product instructions were already native DXIL;
changing shader arithmetic was unnecessary for this improvement.

Separate ordinary release runs, with per-command timing off, yielded the
following medians over the last twelve stable gameplay reports. Each run
continued for more than five minutes; loading and pipeline compilation
were excluded. Animation and draw counts varied between runs.

| Measurement | Original | Corrected |
| --- | ---: | ---: |
| Frame time | 348.35 ms | 87.90 ms |
| FPS | 2.9 | 11.35 |
| Buffer data prepared per frame | 77.45 MB | 77.65 MB |
| Preparation time | 39.50 ms | 26.60 ms |
| Reported GPU-wait time | 300.95 ms | 56.20 ms |
| Copyback time | 24.75 ms | 18.75 ms |

These CPU-side categories overlap and should not be added together.
No descriptor exhaustion or device removal was reported in either run.
Existing unavailable pipelines remained; this is a performance correction,
not a claim of complete rendering support.

The remaining frame time includes other GPU work and synchronization for
title reads of GPU-written memory, preparation of buffer copies, and GPU
writeback. The copyback and preparation paths deserve separate profiling;
the upscaler's arithmetic no longer explains the large stall.

## Verification and tracing

The synthetic GPU regression checks a misaligned 20 MiB logical range,
CPU changes made after recording, reuse of the same closed list, uploads
across a page boundary and the last word, and CPU writeback of GPU changes.
Pure admission tests cover minimum-size padding, reserved room, total
limits, small/zero budgets, and an oversized single copy. The existing
alignment-view regression also checks the original small-copy width.

In a diagnostics build, `VICTORY_TRACE_BUFFER_PLACEMENT` accepts a
comma-separated list of shader identifiers. It reports bound buffer
placement once per view, plus allocation-quota refusals. It changes no
allocation or synchronization decisions. Use `VICTORY_GPU_TIMING=1` for
dispatch attribution, then turn it off for frame-rate comparisons.

Runtime logs, shader inspection files, and captures remain outside the
repository under the caller's scratch directory.
