//! A stand-in for the indirect light the title traces rays for.
//!
//! With the ray-tracing structure builder off (`crate::pipeline::builder_runs`)
//! the title's rays find nothing. Its program that resolves the diffuse light
//! of its screen probes (`CS_ScreenProbes_TemporalResolve_*`) then leaves the
//! sky's light outdoors, nothing indoors (walls are black where no lamp or
//! sun reaches) and, in blocks of the picture's tiles, what the probes'
//! memory held. Right after that program the light is raised to an estimate
//! and bounded by a multiple of it.
//!
//! That a stand-in can be put there, and in which of the program's images,
//! is beloooo's find (VIctory-Belobug-AMD, 06031237, 4f45ab88, cd9f78ce,
//! GPL-3.0-or-later), which puts a constant there, shaded in creases from the
//! scene's depth. The estimate here is worked out from the frame instead, so
//! it follows the hour, the room and its lamps by itself: the light the
//! visible surfaces send out (the title keeps it for its own screen traces:
//! `CS_CacheScreenSpaceRadiance`, the frame before) is averaged in 64x36
//! cells, and every point gets the mean of the cells weighted by the share of
//! its surroundings they cover, as a directional value (first-order harmonics
//! at 160x90 points, taken between them by depth).
//!
//! What it assumes: what is not on screen sends out what is (with a slow
//! mean of the last seconds, so that turning into a dark corner is not black
//! at once). No occlusion but the creases'.
//!
//! `VICTORY_AMBIENT`:
//! - unset or `0`/`off`: the title's own output.
//! - a number: that share of the estimate (1: all of it).
//! - `constant:<level>`: the constant, as beloooo's fork has it.
//!
//! `VICTORY_AMBIENT_SHADE=<strength>,<radius in metres>` sets the creases.
//! `VICTORY_AMBIENT_SETTLE=<ms>[,<jump>]` has the slow mean follow by the
//! clock, not by the frame (see `settle`).
//!
//! The air of a room is lit by the same probes: without rays they hold the
//! sky's light indoors too, a brown veil at sunset. `VICTORY_AMBIENT_AIR`
//! bounds what the air scatters by a multiple of the light around.
//!
//! That bounds the title's volume of froxels. The veil itself comes another
//! way: marched along rays (`scattering_transmittance_system`:
//! `CS_Gather_Classified_*` write it at 180x102, the two
//! `CS_ReconstructRaymarchClassified*` take it to 360x204), lit by the
//! transparency probes, which without rays are the open sky indoors.
//! `VICTORY_AMBIENT_MARCH=<multiple>` (with `VICTORY_AMBIENT` a number)
//! holds that image to the multiple of the room's slow mean before either
//! reconstruct reads it. Unset or 0: off. Measured in one interior on a
//! 4 GB card: the image's mean 0.24 (high 0.31) in a clean run, 0.67 (high
//! 0.94) in a veiled one, the room's mean near 0.29 (inferred, not read).
//! So about 1 should leave the clean run as it is and cut the veiled one to
//! under half: an estimate to begin measuring from, not a measured value.

use std::sync::{Mutex, OnceLock};

use windows::Win32::Graphics::Direct3D12::*;
use windows::core::s;

use crate::host::{HostList, host};
use crate::object::This;

/// Programs of the title the stand-in follows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Role {
    /// Resolves the diffuse light.
    Resolve,
    /// Keeps the light visible surfaces send out.
    Radiance,
    /// Adds the lamps' light to what the air scatters (the last program
    /// that writes that volume before it is summed up along the view).
    Air,
    /// Rebuilds the in-scatter marched along rays from its gathered image:
    /// followed before it runs, not after ([`clear_march`]).
    March,
}

/// The program's part in this, by its name in the container.
pub(crate) fn role(container: &[u8]) -> Option<Role> {
    let has = |name: &[u8]| container.windows(name.len()).any(|w| w == name);
    if has(b"CS_ScreenProbes_TemporalResolve") {
        Some(Role::Resolve)
    } else if has(b"CS_CacheScreenSpaceRadiance") {
        Some(Role::Radiance)
    } else if has(b"CS_AccumulateLocalLightInscatteringVolume") {
        Some(Role::Air)
    } else if has(b"CS_ReconstructRaymarchClassified") {
        Some(Role::March)
    } else {
        None
    }
}

/// Where the resolve writes the diffuse light (R9G9B9E5 as 32-bit integers)
/// and reads the scene's depth, and where the other writes the light of
/// visible surfaces: root parameter and place in its table, in dwords.
pub(crate) const LIGHT: (u8, u32) = (1, 176);
pub(crate) const DEPTH: (u8, u32) = (0, 464);
pub(crate) const RADIANCE: (u8, u32) = (1, 16);
/// Where the light the air scatters is.
pub(crate) const AIR: (u8, u32) = (1, 32);
/// Where the reconstructs read the gathered in-scatter (R16G16B16A16_FLOAT,
/// 180x102 on Series S; `Full` reads both, `NoAlpha` the first. The image
/// of that format at 72 is velocity).
pub(crate) const MARCH: [(u8, u32); 2] = [(1, 96), (1, 112)];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Set {
    Off,
    Estimate(f32),
    Constant(f32),
}

fn parse(set: &str) -> Set {
    let set = set.trim();
    if set.is_empty() {
        return Set::Estimate(1.0);
    }
    if set == "off" || set == "0" {
        return Set::Off;
    }
    let number = |t: &str| t.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0);
    match set.strip_prefix("constant:") {
        Some(level) => number(level).map_or(Set::Off, Set::Constant),
        None => number(set).map_or(Set::Off, Set::Estimate),
    }
}

fn set() -> Set {
    static SET: OnceLock<Set> = OnceLock::new();
    *SET.get_or_init(|| parse(&std::env::var("VICTORY_AMBIENT").unwrap_or_default()))
}

/// Whether the stand-in is on: set, and the title's rays find nothing.
pub(crate) fn on() -> bool {
    set() != Set::Off && !crate::pipeline::builder_runs()
}

/// The most the air scatters, as a multiple of the light around
/// (`VICTORY_AMBIENT_AIR=<multiple>`; unset or 0: what the title has).
fn air() -> f32 {
    static SET: OnceLock<f32> = OnceLock::new();
    *SET.get_or_init(|| {
        let set = std::env::var("VICTORY_AMBIENT_AIR").unwrap_or_default();
        set.trim().parse().ok().filter(|v: &f32| v.is_finite() && *v > 0.0).unwrap_or(0.0)
    })
}

fn multiple(set: &str) -> f32 {
    set.trim().parse().ok().filter(|v: &f32| v.is_finite() && *v > 0.0).unwrap_or(0.0)
}

/// The most the in-scatter marched along rays holds, as a multiple of the
/// room's slow mean (`VICTORY_AMBIENT_MARCH=<multiple>`; unset or 0: what
/// the title has. About 1 to begin with: see the module's text).
pub(crate) fn march() -> f32 {
    static SET: OnceLock<f32> = OnceLock::new();
    *SET.get_or_init(|| multiple(&std::env::var("VICTORY_AMBIENT_MARCH").unwrap_or_default()))
}

/// How many times the estimate the title's own result may keep
/// (`VICTORY_AMBIENT_MOST=<multiple>`, 4 when unset). Without rays the
/// probes hold the sky's light indoors too; at 4 times the estimate a room
/// looked towards a window or the sky goes very bright.
fn most() -> f32 {
    static SET: OnceLock<f32> = OnceLock::new();
    *SET.get_or_init(|| {
        let set = std::env::var("VICTORY_AMBIENT_MOST").unwrap_or_default();
        set.trim().parse().ok().filter(|v: &f32| v.is_finite() && *v > 0.0).unwrap_or(4.0)
    })
}

/// How the slow mean of the light around follows the frames
/// (`VICTORY_AMBIENT_SETTLE=<time constant in ms>[,<jump factor>]`, e.g.
/// `250,3`; unset or 0: 0.04 of the way a frame, which is 25 frames: 0.8 s
/// at 30 frames a second, 4-6 s at 4-6). Set, it is the same share of the
/// way a second whatever the frame rate, and a change by more than the jump
/// factor (3 when not given; 0: none) is taken at once, as a new room is.
fn settle() -> Option<[f32; 2]> {
    static SET: OnceLock<Option<[f32; 2]>> = OnceLock::new();
    *SET.get_or_init(|| parse_settle(&std::env::var("VICTORY_AMBIENT_SETTLE").unwrap_or_default()))
}

fn parse_settle(set: &str) -> Option<[f32; 2]> {
    let mut parts = set.split(',').map(|p| p.trim().parse::<f32>().ok());
    let ms = parts.next().flatten().filter(|v| v.is_finite() && *v > 0.0)?;
    let jump = parts.next().flatten().filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(3.0);
    Some([ms, jump])
}

/// The share of the way the slow mean goes at a keeping that comes `ms`
/// after the last.
fn follow(ms: f32) -> f32 {
    settle().map_or(0.04, |[constant, _]| 1.0 - (-ms / constant).exp())
}

/// Milliseconds since the last keeping ([`Role::Radiance`], once a frame);
/// the first, and after a pause of over a second, as a frame of 30.
fn frame_ms() -> f32 {
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = crate::host::now_ms();
    let last = LAST.swap(now, std::sync::atomic::Ordering::Relaxed);
    match now.saturating_sub(last) {
        0 => 1.0,
        dt if last == 0 || dt > 1000 => 33.0,
        dt => dt as f32,
    }
}

/// Strength and radius of the creases' shade.
fn shade() -> [f32; 2] {
    static SET: OnceLock<[f32; 2]> = OnceLock::new();
    *SET.get_or_init(|| {
        let mut shade = [1.0, 0.6];
        let set = std::env::var("VICTORY_AMBIENT_SHADE").unwrap_or_default();
        for (to, v) in shade.iter_mut().zip(set.split(',')) {
            if let Ok(v) = v.trim().parse() {
                *to = v;
            }
        }
        shade
    })
}

const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=13, b0), DescriptorTable(SRV(t0)), DescriptorTable(UAV(u0)), UAV(u1), UAV(u2)"
#define CX 64
#define CY 36
#define CELLS (CX * CY)
#define PX 160
#define PY 90
cbuffer Set : register(b0) {
    float level; float constant; float strength; float radius;
    float near; float tan_y; uint width; uint height;
    float follow; float most; float air; float unused;
    float jump;
};
Texture2D<float> depth : register(t0);
// Cells: the light they send out (then, at CELLS, the slow mean), from
// CELLS + 1 on where they are.
RWStructuredBuffer<float4> cells : register(u1);
// Three a point: red, green and blue, each a mean and a direction.
RWStructuredBuffer<float4> points : register(u2);
static const float3 LUMA = float3(0.2126, 0.7152, 0.0722);

// The point seen at a texel, in metres from the eye (x right, y up, z away).
float3 seen(int2 p, float2 size) {
    float d = depth.Load(int3(clamp(p, int2(0, 0), int2(size) - 1), 0));
    float z = min(near / max(d, 1e-7), 1000.0);
    float2 ndc = (float2(p) + 0.5) / size * 2.0 - 1.0;
    return float3(ndc.x * tan_y * size.x / size.y * z, -ndc.y * tan_y * z, z);
}

#ifdef KEEP
RWTexture2D<float3> radiance : register(u0);
[RootSignature(RS)]
[numthreads(8, 4, 1)]
void keep(uint3 at : SV_DispatchThreadID) {
    uint w, h;
    radiance.GetDimensions(w, h);
    float3 sum = 0;
    for (uint y = 0; y < 5; y++) for (uint x = 0; x < 5; x++) {
        uint2 p = uint2((float2(at.xy) + (float2(x, y) + 0.5) / 5.0) * float2(w, h) / float2(CX, CY));
        float3 c = max(radiance[p], 0.0);
        // NaN or infinity: every exponent bit set (no `isnan`/`isinf`, which
        // Wine's compiler lacks).
        c = any((asuint(c) & 0x7f800000u) == 0x7f800000u) ? 0.0 : c;
        // Lamps and glints count, but not for everything.
        sum += c / max(1.0, dot(c, LUMA) / 20.0);
    }
    cells[at.y * CX + at.x] = float4(sum / 25.0, 1.0);
}
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void mean() {
    float3 sum = 0;
    for (uint i = 0; i < CELLS; i++) sum += cells[i].rgb;
    float4 was = cells[CELLS];
    float3 now = sum / CELLS;
    // (`jump` > 0: a change of more than that factor either way is
    // followed at once, as a new room is.)
    float a = max(dot(now, LUMA), 1e-5), b = max(dot(was.rgb, LUMA), 1e-5);
    float share = (jump > 0.0 && max(a / b, b / a) > jump) ? 1.0 : follow;
    cells[CELLS] = float4(was.a > 0.0 ? lerp(was.rgb, now, share) : now, 1.0);
}
#endif

#ifdef PLACE
[RootSignature(RS)]
[numthreads(8, 4, 1)]
void place(uint3 at : SV_DispatchThreadID) {
    float2 size = float2(width, height);
    float3 p = seen(int2((float2(at.xy) + 0.5) * size / float2(CX, CY)), size);
    cells[CELLS + 1 + at.y * CX + at.x] = float4(p, p.z * p.z);
}
#endif

#ifdef GATHER
[RootSignature(RS)]
[numthreads(8, 2, 1)]
void gather(uint3 at : SV_DispatchThreadID) {
    float2 size = float2(width, height);
    float3 p = seen(int2((float2(at.xy) + 0.5) * size / float2(PX, PY)), size);
    float r2 = 0.25 + 0.0025 * p.z * p.z;
    float whole = 0;
    float3 toward = 0, light = 0, red = 0, green = 0, blue = 0;
    for (uint i = 0; i < CELLS; i++) {
        float4 c = cells[CELLS + 1 + i];
        float3 L = cells[i].rgb;
        float3 d = c.xyz - p;
        float dd = dot(d, d);
        float w = c.w / (dd + r2);
        float3 dir = d / sqrt(dd + 0.25 * r2);
        whole += w;
        toward += w * dir;
        light += w * L;
        red += w * L.r * dir;
        green += w * L.g * dir;
        blue += w * L.b * dir;
    }
    whole = max(whole, 1e-6);
    light /= whole;
    toward /= whole;
    uint at0 = (at.y * PX + at.x) * 3;
    points[at0] = float4(red / whole - light.r * toward, light.r);
    points[at0 + 1] = float4(green / whole - light.g * toward, light.g);
    points[at0 + 2] = float4(blue / whole - light.b * toward, light.b);
}
#endif

#ifdef AIR
RWTexture3D<float3> scattered : register(u0);
// No more than a multiple of the light around: of the room as the last
// seconds showed it, and of the surface seen in this direction (towards a
// window the air outside may be as bright as the sky is).
[RootSignature(RS)]
[numthreads(8, 8, 2)]
void clear_air(uint3 at : SV_DispatchThreadID) {
    uint w, h, d;
    scattered.GetDimensions(w, h, d);
    if (at.x >= w || at.y >= h || at.z >= d) return;
    float cap;
    if (constant > 0.0) {
        cap = air * constant;
    } else {
        uint2 q = min(uint2((float2(at.xy) + 0.5) / float2(w, h) * float2(PX, PY)), uint2(PX - 1, PY - 1));
        uint at0 = (q.y * PX + q.x) * 3;
        float3 seen_there = float3(points[at0].w, points[at0 + 1].w, points[at0 + 2].w);
        float far = float(at.z) / float(d);
        cap = air * lerp(dot(cells[CELLS].rgb, LUMA), dot(seen_there, LUMA), far);
    }
    float3 c = scattered[at];
    // A NaN or infinity in the air is summed along every view ray through
    // it: black or white patches floating in the room (tested as in `keep`).
    if (any((asuint(c) & 0x7f800000u) == 0x7f800000u)) { scattered[at] = float3(0.0, 0.0, 0.0); return; }
    float has = dot(c, LUMA);
    if (has > cap) scattered[at] = c * (cap / has);
}
#endif

#ifdef MARCH
RWTexture2D<float4> marched : register(u0);
// No more than a multiple of the room's light as the last seconds showed
// it (`air` is that multiple here); the hue and the fourth channel stay.
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void clear_march(uint3 at : SV_DispatchThreadID) {
    uint w, h;
    marched.GetDimensions(w, h);
    if (at.x >= w || at.y >= h) return;
    float4 c = marched[at.xy];
    // (NaN or infinity: tested as in `keep`.)
    if (any((asuint(c.rgb) & 0x7f800000u) == 0x7f800000u)) { marched[at.xy] = float4(0.0, 0.0, 0.0, c.a); return; }
    float cap = air * dot(cells[CELLS].rgb, LUMA);
    float has = dot(c.rgb, LUMA);
    if (has > cap) marched[at.xy] = float4(c.rgb * (cap / has), c.a);
}
#endif

#ifdef RAISE
RWTexture2D<uint> light : register(u0);
float3 unpack(uint v) {
    return float3(v & 511, (v >> 9) & 511, (v >> 18) & 511) * exp2(float(v >> 27) - 24.0);
}
uint pack(float3 c) {
    c = clamp(c, 0.0, 65408.0);
    float top = max(c.r, max(c.g, c.b));
    int e = clamp(int(floor(log2(max(top, 1e-9)))) + 16, 0, 31);
    float scale = exp2(float(24 - e));
    if (top * scale >= 511.5) { e = min(e + 1, 31); scale = exp2(float(24 - e)); }
    uint3 m = min(uint3(c * scale + 0.5), 511u);
    return m.r | (m.g << 9) | (m.b << 18) | (uint(e) << 27);
}
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void raise(uint3 at : SV_DispatchThreadID) {
    float2 size = float2(width, height);
    if (at.x >= width || at.y >= height) return;
    int2 p = int2(at.xy);
    float3 c = seen(p, size);
    float3 l = seen(p + int2(-1, 0), size), r = seen(p + int2(1, 0), size);
    float3 u = seen(p + int2(0, -1), size), d = seen(p + int2(0, 1), size);
    float3 dx = abs(l.z - c.z) < abs(r.z - c.z) ? c - l : r - c;
    float3 dy = abs(u.z - c.z) < abs(d.z - c.z) ? c - u : d - c;
    float3 n = normalize(cross(dy, dx) + float3(0, 0, -1e-9));
    if (dot(n, c) > 0.0) n = -n;
    // How open it is (as in beloooo's cd9f78ce).
    float open = 1.0;
    float px = min(radius * size.y / (2.0 * tan_y * c.z), 128.0);
    float noise = frac(52.9829189 * frac(dot(float2(p), float2(0.06711056, 0.00583715))));
    if (strength > 0.0 && px >= 1.5 && c.z < 400.0) {
        float sum = 0.0;
        const int N = 12;
        for (int i = 0; i < N; i++) {
            float a = i * 2.399963 + noise * 6.2831853;
            float2 o = float2(cos(a), sin(a)) * (sqrt((i + 0.5) / N) * px);
            float3 v = seen(p + int2(round(o)), size) - c;
            float vv = dot(v, v);
            float over = dot(v, n) - 0.004 * c.z - 0.005;
            sum += max(over, 0.0) / sqrt(vv + 1e-6) * saturate(1.0 - vv / (radius * radius));
        }
        open = saturate(1.0 - strength * 2.0 * sum / N);
    }
    float3 found;
    if (constant > 0.0) {
        found = float3(1.0, 0.98, 0.94) * constant * open;
        light[p] = pack(found);
        return;
    }
    // Between the four points around, those at its own depth first; which
    // four is dithered a little.
    float2 g = (float2(p) + 0.5) / size * float2(PX, PY) - 0.5 + (noise - 0.5) * 0.5;
    int2 g0 = int2(floor(g));
    float2 f = g - float2(g0);
    float4 sr = 0, sg = 0, sb = 0;
    float sum = 0;
    for (int k = 0; k < 4; k++) {
        int2 q = clamp(g0 + int2(k & 1, k >> 1), int2(0, 0), int2(PX, PY) - 1);
        float z = seen(int2((float2(q) + 0.5) * size / float2(PX, PY)), size).z;
        float2 b = float2(k & 1 ? f.x : 1 - f.x, k >> 1 ? f.y : 1 - f.y);
        float w = (b.x * b.y + 1e-3) / (0.02 + abs(z - c.z) / c.z);
        uint at0 = (q.y * PX + q.x) * 3;
        sr += w * points[at0];
        sg += w * points[at0 + 1];
        sb += w * points[at0 + 2];
        sum += w;
    }
    float3 all = float3(sr.w, sg.w, sb.w) / sum;
    found = all + 1.2 * float3(dot(sr.xyz, n), dot(sg.xyz, n), dot(sb.xyz, n)) / sum;
    found = clamp(found, 0.2 * all, 2.5 * all);
    found = lerp(found, cells[CELLS].rgb, 0.2) * level * open;
    found = any((asuint(found) & 0x7f800000u) == 0x7f800000u) ? 0.0 : max(found, 0.0);
    float3 was = unpack(light[p]);
    // What the program left counts as far as it can be true: the sky's light
    // outdoors, not the blocks of other memory.
    float top = max(dot(found, LUMA) * most, 1e-6);
    float has = dot(was, LUMA);
    was *= min(1.0, top / max(has, 1e-6));
    light[p] = pack(max(was, found));
}
#endif
"#;

/// Bytes of the cells' buffer and of the points'.
const CELLS: u64 = 64 * 36;
const CELL_BYTES: u64 = (2 * CELLS + 1) * 16;
const POINT_BYTES: u64 = 160 * 90 * 48;

struct Pass {
    root: ID3D12RootSignature,
    keep: ID3D12PipelineState,
    mean: ID3D12PipelineState,
    place: ID3D12PipelineState,
    gather: ID3D12PipelineState,
    raise: ID3D12PipelineState,
    air: ID3D12PipelineState,
    /// The optional clamp for the marched in-scatter.
    march: Option<ID3D12PipelineState>,
    cells: ID3D12Resource,
    points: ID3D12Resource,
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Pass {}
// SAFETY: as above.
unsafe impl Sync for Pass {}

fn make(device: &ID3D12Device, buffer: impl Fn(u64) -> Option<ID3D12Resource>) -> windows::core::Result<Pass> {
    let mut root: Option<ID3D12RootSignature> = None;
    let mut pipeline = |part: &str, entry| -> windows::core::Result<ID3D12PipelineState> {
        let code = crate::host::compile(&format!("#define {part}\n{SOURCE}"), entry, s!("cs_5_1"))?;
        // SAFETY: the root signature is inside the shader; plain creations.
        unsafe {
            if root.is_none() {
                // As the shader's `RS`, made here: Wine's compiler embeds none.
                use crate::rootsig::Param;
                root = Some(crate::rootsig::make(
                    device,
                    &[
                        Param::Constants { register: 0, count: 13 },
                        Param::SrvTable { register: 0, count: 1, visibility: D3D12_SHADER_VISIBILITY_ALL },
                        Param::UavTable { register: 0, count: 1 },
                        Param::Uav(1),
                        Param::Uav(2),
                    ],
                    "indirect light stand-in",
                )?);
            }
            let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::transmute_copy(&root),
                CS: D3D12_SHADER_BYTECODE { pShaderBytecode: code.as_ptr().cast(), BytecodeLength: code.len() },
                ..Default::default()
            };
            device.CreateComputePipelineState(&desc)
        }
    };
    let keep = pipeline("KEEP", s!("keep"))?;
    let mean = pipeline("KEEP", s!("mean"))?;
    let place = pipeline("PLACE", s!("place"))?;
    let gather = pipeline("GATHER", s!("gather"))?;
    let raise = pipeline("RAISE", s!("raise"))?;
    let air = pipeline("AIR", s!("clear_air"))?;
    let march = (march() > 0.0).then(|| pipeline("MARCH", s!("clear_march")))
        .and_then(|made| made.inspect_err(|e| tracing::warn!(error = %e, "no clamp of the marched in-scatter")).ok());
    let made = |bytes| buffer(bytes).ok_or_else(windows::core::Error::empty);
    Ok(Pass {
        root: root.ok_or_else(windows::core::Error::empty)?,
        keep,
        mean,
        place,
        gather,
        raise,
        air,
        march,
        cells: made(CELL_BYTES)?,
        points: made(POINT_BYTES)?,
    })
}

fn pass() -> Option<&'static Pass> {
    static PASS: OnceLock<Option<Pass>> = OnceLock::new();
    PASS.get_or_init(|| {
        let host = host()?;
        make(&host.device, |bytes| host.uav_buffer(bytes))
            .inspect_err(|e| tracing::warn!(error = %e, "no stand-in for the indirect light"))
            .ok()
    })
    .as_ref()
}

fn uav_barrier(list: &ID3D12GraphicsCommandList) {
    let barrier = D3D12_RESOURCE_BARRIER { Type: D3D12_RESOURCE_BARRIER_TYPE_UAV, ..Default::default() };
    // SAFETY: an open list; the barrier names no resource.
    unsafe { list.ResourceBarrier(&[barrier]) };
}

/// Whether the light of a frame has been kept: the estimate has something
/// to begin with.
static KEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the passes read and write, as views in the heap the list has set.
pub(crate) struct Views {
    /// The image the title's program just wrote.
    pub(crate) image: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// The scene's depth, for [`Role::Resolve`].
    pub(crate) depth: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// The size of the image, and how many slices it has.
    pub(crate) size: [u32; 2],
    pub(crate) slices: u32,
}

/// After the title's program of that role, in `list` with the program's view
/// heap set. Replaces the list's compute root signature and pipeline; false
/// if it recorded nothing.
pub(crate) fn after(list: &HostList, role: Role, views: &Views) -> bool {
    use std::sync::atomic::Ordering;
    let Some(pass) = pass() else {
        return false;
    };
    let (level, constant) = match set() {
        Set::Off => return false,
        Set::Estimate(level) => (level, 0.0f32),
        Set::Constant(level) => (0.0, level),
    };
    if role != Role::Radiance && constant == 0.0 && !KEPT.load(Ordering::Relaxed) {
        return false;
    }
    if role == Role::March || role == Role::Air && air() == 0.0 {
        return false;
    }
    static SAID: Mutex<u32> = Mutex::new(0);
    {
        let mut said = SAID.lock().unwrap_or_else(|e| e.into_inner());
        if *said < 4 {
            *said += 1;
            tracing::info!(?role, size = ?views.size, level, constant, "a stand-in for the indirect light");
        }
    }
    list.keep(&pass.cells);
    list.keep(&pass.points);
    let [strength, radius] = shade();
    let floats = [level, constant, strength, radius, 0.1, 0.45];
    let mut words = [0u32; 13];
    for (to, v) in words.iter_mut().zip(floats) {
        *to = v.to_bits();
    }
    words[6] = views.size[0];
    words[7] = views.size[1];
    // (The mean alone reads these two; the clock is asked at a keeping only.)
    if role == Role::Radiance {
        words[8] = follow(frame_ms()).to_bits();
        words[12] = settle().map_or(0.0, |[_, jump]| jump).to_bits();
    }
    words[9] = most().to_bits();
    words[10] = air().to_bits();
    let l = &list.list;
    // SAFETY: an open list with the heap of the views set; the title's
    // program has just used both views, so their textures are in the states
    // these views need; buffers the list keeps.
    unsafe {
        uav_barrier(l);
        l.SetComputeRootSignature(&pass.root);
        l.SetComputeRoot32BitConstants(0, 13, words.as_ptr().cast(), 0);
        l.SetComputeRootDescriptorTable(1, views.depth);
        l.SetComputeRootDescriptorTable(2, views.image);
        l.SetComputeRootUnorderedAccessView(3, pass.cells.GetGPUVirtualAddress());
        l.SetComputeRootUnorderedAccessView(4, pass.points.GetGPUVirtualAddress());
        let run = |pso: &ID3D12PipelineState, x, y, z| {
            l.SetPipelineState(pso);
            l.Dispatch(x, y, z);
            uav_barrier(l);
        };
        match role {
            Role::Radiance => {
                run(&pass.keep, 8, 9, 1);
                run(&pass.mean, 1, 1, 1);
                KEPT.store(true, Ordering::Relaxed);
            }
            Role::Air => run(&pass.air, views.size[0].div_ceil(8), views.size[1].div_ceil(8), views.slices.div_ceil(2)),
            // (Returned above: before its program, not after.)
            Role::March => {}
            Role::Resolve => {
                if constant == 0.0 {
                    run(&pass.place, 8, 9, 1);
                    run(&pass.gather, 20, 45, 1);
                }
                run(&pass.raise, views.size[0].div_ceil(8), views.size[1].div_ceil(8), 1);
            }
        }
    }
    true
}

/// Holds `image` (the gathered in-scatter a reconstruct is about to read:
/// to unordered access, clamped, back) to [`march`] times the slow mean's
/// luminance, hue kept. Sets the list's view heap and replaces its compute
/// root signature and pipeline; false if it recorded nothing.
pub(crate) fn clear_march(list: &HostList, image: This) -> bool {
    use std::sync::atomic::Ordering;
    let (Some(pass), Some(host)) = (pass(), host()) else {
        return false;
    };
    let Some(clamp) = &pass.march else {
        return false;
    };
    // (The slow mean: none with the constant, none before a keeping.)
    if !matches!(set(), Set::Estimate(_)) || !KEPT.load(Ordering::Relaxed) {
        return false;
    }
    let Some(copy) = crate::resource::host_copy(image) else {
        return false;
    };
    // SAFETY: a live resource.
    let d = unsafe { copy.texture.GetDesc() };
    // (R16G16B16A16_FLOAT.)
    if d.Format.0 != 10
        || d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0 == 0
    {
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| {
            tracing::warn!(format = d.Format.0, flags = d.Flags.0, width = d.Width, "marched in-scatter: an image that cannot be clamped");
        });
        return false;
    }
    let views = match copy.march_uav(&host.device) {
        Ok(views) => views,
        Err(e) => {
            static WARN: std::sync::Once = std::sync::Once::new();
            WARN.call_once(|| tracing::warn!(error = %e, "no UAV for the marched in-scatter"));
            return false;
        }
    };
    const WRITTEN: u32 = D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32;
    const READ: u32 =
        (D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0) as u32;
    list.settle(&copy, WRITTEN);
    list.keep(&pass.cells);
    list.keep(&pass.points);
    let mut words = [0u32; 11];
    words[10] = march().to_bits();
    let l = &list.list;
    // (The title's gathers before it wrote the image through other views.)
    uav_barrier(l);
    // SAFETY: the list's texture state holds this exact copy and its immutable heap until reset.
    unsafe {
        let gpu = views.GetGPUDescriptorHandleForHeapStart();
        l.SetDescriptorHeaps(&[Some(views.clone())]);
        l.SetComputeRootSignature(&pass.root);
        l.SetComputeRoot32BitConstants(0, 11, words.as_ptr().cast(), 0);
        l.SetComputeRootDescriptorTable(2, gpu);
        l.SetComputeRootUnorderedAccessView(3, pass.cells.GetGPUVirtualAddress());
        l.SetComputeRootUnorderedAccessView(4, pass.points.GetGPUVirtualAddress());
        l.SetPipelineState(clamp);
        l.Dispatch((d.Width as u32).div_ceil(8), d.Height.div_ceil(8), 1);
    }
    uav_barrier(l);
    list.settle(&copy, READ);
    #[cfg(feature = "diagnostics")]
    {
        crate::capture::note_target(image);
        list.note_op(|| format!("ambient March target={image:p}"));
    }
    static TOLD: Mutex<u32> = Mutex::new(0);
    let mut said = TOLD.lock().unwrap_or_else(|e| e.into_inner());
    if *said < 4 {
        *said += 1;
        tracing::info!(width = d.Width, height = d.Height, most = march(), "the marched in-scatter is held to the light of the room");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings() {
        assert_eq!(parse(""), Set::Estimate(1.0));
        assert_eq!(parse("off"), Set::Off);
        assert_eq!(parse("0"), Set::Off);
        assert_eq!(parse("0.8"), Set::Estimate(0.8));
        assert_eq!(parse("constant:0.3"), Set::Constant(0.3));
        assert_eq!(parse("constant:nan"), Set::Off);
        assert_eq!(parse_settle(""), None);
        assert_eq!(parse_settle("0"), None);
        assert_eq!(parse_settle("250"), Some([250.0, 3.0]));
        assert_eq!(parse_settle("250, 0"), Some([250.0, 0.0]));
        assert_eq!(multiple(""), 0.0);
        assert_eq!(multiple("0"), 0.0);
        assert_eq!(multiple("-1"), 0.0);
        assert_eq!(multiple("nan"), 0.0);
        assert_eq!(multiple(" 1.5 "), 1.5);
    }

    #[test]
    fn roles_by_name() {
        assert_eq!(role(b"..CS_ScreenProbes_TemporalResolve_a911b49c_Wrapped.."), Some(Role::Resolve));
        assert_eq!(role(b"..CS_CacheScreenSpaceRadiance_eed2fc35.."), Some(Role::Radiance));
        assert_eq!(role(b"CS_AccumulateLocalLightInscatteringVolume_7e2f5eef"), Some(Role::Air));
        assert_eq!(role(b"..CS_ReconstructRaymarchClassifiedFull_2d384eab_Wrapped.."), Some(Role::March));
        assert_eq!(role(b"CS_ReconstructRaymarchClassifiedNoAlpha_2d384eab"), Some(Role::March));
        assert_eq!(role(b"CS_ReconstructRaymarch_High_66f3d2a0"), None);
        assert_eq!(role(b"CS_TemporalResolve"), None);
    }

    /// The passes compile with the system's compiler and the host takes them.
    #[test]
    fn the_passes_build() {
        let Some(host) = host() else {
            return;
        };
        make(&host.device, |bytes| host.uav_buffer(bytes)).expect("the passes");
        // (Made only with its switch on, so compiled here by itself.)
        crate::host::compile(&format!("#define MARCH\n{SOURCE}"), s!("clear_march"), s!("cs_5_1")).expect("the march clamp");
    }
}
