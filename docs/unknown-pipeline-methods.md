# Two console pipeline creation methods (0x1c8 not implemented yet, 0x218 off by default)

The title creates thousands of pipelines through two console
device methods that the layer does not implement (`device.rs`,
`unknown_pipeline`). Every call fails and the game silently goes without the
pipeline, so whatever it draws or computes with it is missing. In one session
on the test PC 0x1c8 was called over 16,000 times and 0x218 over 2,000.

This file records what is known so far, from the arguments of the first 16
calls of each (`VICTORY_PIPELINE_DUMP`, see the README) and the code they
carry, disassembled with
`cargo run -p victory-shader --example disasm_raw -- <file>`.

Neither method's shaders are containers from the title's shader archive:
their machine code is already in GPU memory, and searching that archive,
the title's smaller archives and its executable for it finds nothing (it may
be packed in the larger archives, or built at run time). A translation therefore has to start from the native code and the
metadata blocks below rather than from a container's `XDAT`/`FSSM` record
(`victory-shader/src/native.rs`).

## 0x1c8: graphics pipelines, `(this, description, iid, out)`

The description, in 64-bit words:

| Word | Contents |
|------|----------|
| 0 | the root signature (one of our objects) |
| 1, 2 | a state block and its size: 0x184 bytes with a pixel shader, 0x17c without |
| 3, 4 | the input layout and its size (0x50-0xbc): semantic names such as `POSITION`, `COLOR`, `TEXCOORD` appear in it |
| 5, 6 | pixel shader code (GPU memory) and its size; both 0 when there is no pixel shader (depth-only pipelines) |
| 7, 8 | the pixel shader's metadata block and its size |
| 9, 10 | vertex shader code (GPU memory) and its size |
| 11, 12 | the vertex shader's metadata block and its size |
| 13.. | 0 |

Seen in the first call: the pixel shader interpolates `attr0` and `attr1`,
samples a 2D texture with an immediate sampler, and exports `mrt0`
compressed (fp16); the vertex shader fetches position, an 8_8_8_8 UNORM
colour and a texture coordinate with `idxen` loads from the vertex buffer
table in `s[0:1]` (index `s2 + v0`), multiplies by two matrices from
constant buffers, and exports `pos0`, `param0`, `param1`.

### Metadata blocks (pixel 0x30-0x310 bytes, vertex 0x30-0xd0)

A 16-byte header, then entries of 32 bytes:

```text
header  u32 2, u32 block size - 12, u32 0x76, u32 stage word
        (0x00030440 pixel, 0x00028440 vertex in the first call)
entry   u32 kind, then 7 u32
kind 0x41  a descriptor the shader loads: u32 a, u32 table offset (dwords),
           u32 c, u32 d, u32 e (low byte: descriptor dwords, 4 or 8),
           u32 0xffffffff, u32 0
kind 0x42  u32 0, u32 (0x000c0001, 0x00020001, 0x000d0201 seen), u32,
           u32 1, u32 0, u32 0xffffffff, u32 0
kind 0x43  a 16-byte hash
```

Checked against the code of the first call: the vertex shader's two 0x41
entries have table offsets 8 and 0x18 dwords, and it loads its two constant
buffer descriptors at bytes 0x20 and 0x60 of the table in `s[8:9]`; the pixel
shader's have offsets 0 (a 4-dword constant buffer, loaded at byte 0) and
0x10 (an 8-dword texture, loaded at byte 0x40). One pixel entry has
`a = 0x401` and size word 0x20004: probably the immediate sampler, which the
code builds from literals.

Not known yet: the state block (blend, rasterizer, depth-stencil, render
target formats are expected there), the exact input layout format, the 0x42
entries (likely the user-data layout: which SGPRs hold which root
parameter), and where the pixel shader's input mapping and export formats
are.

## 0x218: compute pipelines (implemented, off by default)

One compute pipeline per call, `(this, description, iid, out)`. The
description is 12 words (the dumps show several, as descriptions lie side by
side in memory):

| Word | Contents |
|------|----------|
| 0 | the root signature |
| 1, 2 | the register block (0x1c bytes) |
| 3, 4 | a 0x14-byte block: a hash, then 3 (not needed) |
| 5, 6 | machine code (GPU memory) and its size |
| 7, 8 | the metadata block and its size |
| 9-11 | two addresses and a 64-bit hash (not needed) |

Register block: `u32 0x4005cf00, u32 0x6c090102, u32 0, u32 x | y << 16,
u32 z | (something, 0x10 seen) << 16, u32 COMPUTE_PGM_RSRC1,
u32 COMPUTE_PGM_RSRC2`. Groups of 8x8, 16x16, 32x32, 8x16, 32x1, 64x1, 128x1
and 1024x1 were seen.

Metadata block: `u32 0x403, u32 (bytes after the first 0x14), u32 (0x20 per
0x42 entry), u32 0, u32 0`, a stage word, then the 32-byte entries described
above. The 0x41 entries are the console's used-resource table in another
packing: `(root type << 8 | resource type)` (resource 0 SRV, 1 sampler,
2 CBV, 3 UAV; root 0 constants, 1 descriptor, 3 table, 4 immediate sampler),
table offset, shader register, stride, `dwords | user-data dword << 8 |
root slot << 16`. Root constants run from their user-data dword to the last
user SGPR `RSRC2` gives.

The wave size is not in the blocks; `exec` writes in the code tell it
(64-bit forms: 64 lanes; `exec_lo`: 32), and programs with neither are
taken as 32 lanes.

`victory_shader::native::precompiled_compute` builds a `NativeProgram` from
the blocks and `native_container` wraps it as a console container, so the
pipeline takes the usual translation, cache and creation path
(`pipeline::create_precompiled_compute`). Offline, `cargo run -p
victory-shader --features host --example precompiled -- pipeline-dumps`
translates every dumped call and makes its D3D12 pipeline. Of 400 calls
dumped in the prologue, 126 translate; what stops the rest is general
translator ground (addresses with two computed terms 101, lane masks of
other lanes read as data 51, partial image stores 21, ...).

In the game the pipelines are made only with `VICTORY_PRECOMPILED=1`
(`jugar.ps1 -Precompiled`): the frame rate rose from about 3 to 4 fps, but the
picture went grey, most likely because programs that translate now run
while others of the same chain still fail and leave their outputs unwritten.

## Next steps

1. Decode the remaining fields from more calls (the dump keeps 16 of each;
   raise the limit in `dump_unknown_pipeline` for more).
2. 0x218: find which running program turns the picture grey (or leave out
   chains with a member that fails), and cover more of the 274 programs that
   do not translate.
3. 0x1c8: the same path for vertex and pixel programs, which need their
   input and output signatures (the input layout block, the pixel inputs)
   and the graphics state block.
