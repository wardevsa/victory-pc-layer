# The two pipeline creation methods not implemented yet (0x1c8, 0x218)

Grand Theft Auto VI creates thousands of pipelines through two console
device methods that the layer does not implement (`device.rs`,
`unknown_pipeline`). Every call fails and the game silently goes without the
pipeline, so whatever it draws or computes with it is missing. In one session
on the test PC 0x1c8 was called over 16,000 times and 0x218 over 2,000.

This file records what is known so far, from the arguments of the first 16
calls of each (`VICTORY_PIPELINE_DUMP`, see the README) and the code they
carry, disassembled with
`cargo run -p victory-shader --example disasm_raw -- <file>`.

Neither method's shaders are containers from `shaders.rpf`: their machine
code is already in GPU memory, and searching `shaders.rpf`, `data.rpf`,
`textures.rpf`, `common.rpf`, `script.rpf`, `gameos.xvd` and the executable
for it finds nothing (it may be packed in the larger archives, or built at run
time). A translation therefore has to start from the native code and the
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

## 0x218: compute pipelines, several per call

The description lists several pipelines in a row, each as four
(address, size) pairs: a 0x1c-byte block, a 0x14-byte block, the code (GPU
memory, 0x180-0x400 bytes) and a metadata block (0x118-0x1d8 bytes), with
other words between groups. The first call's first program samples a 2D
array texture at a level (`image_sample_l`) and stores the result with
`image_store`: an ordinary compute program.

## Next steps

1. Decode the remaining fields from more calls (the dump keeps 16 of each;
   raise the limit in `dump_unknown_pipeline` for more).
2. Build a `NativeProgram` (`victory-shader/src/native.rs`) from code plus
   metadata, so that the existing translator can take these programs; 0x218
   (compute, no input/output signatures) is the simpler place to start.
3. Implement 0x218, then 0x1c8 with its graphics state, on top of
   `pipeline::hand_out`.
