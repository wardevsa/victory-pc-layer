//! Console wave64 programs on hosts with 32-lane waves: what the two host waves that make
//! up one console wave hold alike.
//!
//! Such a host runs a console wave of 64 lanes as two waves of 32 (`exchange` in the
//! lowering). A first-lane read, a lane read or a branch the whole wave takes together then
//! needs the other half's lanes, which it gets through LDS and group barriers; barriers only
//! fit in blocks every lane runs. Much of what the console compiler reads across lanes is the
//! same in all 64 lanes of a console wave, though: made of scalars, constants and the console
//! wave's own number in the group (`thread id >> 6`), or loaded from an address made of those
//! once nothing writes there any more. Read in a block that the lanes of a console wave run or
//! skip together, such a value gives each host wave the console's answer among its own lanes.
//!
//! [`Coherence`] finds the values every lane of a console wave holds alike ("coherent") and
//! the blocks the lanes of a console wave run together. It is the greatest fixpoint: every
//! value and block starts coherent and is taken back where a rule fails, until nothing
//! changes, so that values carried around loops are judged by the whole loop.
//! shortcut: memory other than LDS (buffers, images, constants) is not trusted to read the
//! same in both host waves; only LDS that no instruction writes past a barrier every lane
//! runs is.

use std::collections::HashSet;

use crate::gfx10::{Fields, NULL, Operand};

/// EXEC's dwords by their operand codes.
const EXEC_LO: u8 = 126;
const EXEC_HI: u8 = 127;
use crate::io::{Input, Io};
use crate::ir::{Inst, Program, Reg, Value};
use crate::structure::{Atom, Bool, LaneExit, Structured};

/// What the two host waves of a console wave hold alike ([`coherence`]).
#[derive(Debug, Default)]
pub(super) struct Coherence {
    /// Values (VGPR dwords, and the lane bits of compare and carry results) that every
    /// lane of a console wave holding them holds alike.
    values: HashSet<Value>,
    /// Per block: whether the lanes of a console wave run it all together or not at all.
    blocks: Vec<bool>,
}

impl Coherence {
    /// Whether every lane of a console wave that holds `v` holds the same.
    pub(super) fn value(&self, v: Value) -> bool {
        self.values.contains(&v)
    }

    /// Whether the lanes of a console wave run block `b` all together or not at all.
    pub(super) fn block(&self, b: usize) -> bool {
        self.blocks.get(b).copied().unwrap_or(false)
    }

    /// Whether a lane condition is the same in every lane of a console wave.
    pub(super) fn bool(&self, st: &Structured, b: Bool) -> bool {
        bool_coherent(st, &self.values, b)
    }

    /// Whether the lanes of a console wave run instruction `i` of block `b` all together or
    /// not at all: they run the block so, and its guard (a `v_cmpx` before it in the block)
    /// is alike in all of them.
    pub(super) fn runs_together(&self, st: &Structured, b: usize, i: usize) -> bool {
        self.block(b)
            && st
                .guards
                .get(b)
                .and_then(|g| g.get(i))
                .is_none_or(|g| g.is_none_or(|g| self.bool(st, g)))
    }
}

fn atom_coherent(st: &Structured, values: &HashSet<Value>, a: Atom, depth: u32) -> bool {
    match a {
        Atom::Uniform(_) => true,
        // the lane's bit of a compare or carry result
        Atom::Lane(v) => values.contains(&v),
        // a mask joined at a block entry: as coherent as every mask that comes in
        Atom::Phi(v) => {
            depth < 8
                && st.phi_masks.get(&v).is_some_and(|ins| {
                    ins.iter().all(|&b| {
                        st.bools
                            .support(b)
                            .into_iter()
                            .all(|a| atom_coherent(st, values, a, depth + 1))
                    })
                })
        }
        // a quad, a constant other than all or none, a mask the structuring does not follow:
        // they differ between lanes
        Atom::Quad(_) | Atom::Opaque(_) | Atom::Constant(_) => false,
    }
}

fn bool_coherent(st: &Structured, values: &HashSet<Value>, b: Bool) -> bool {
    st.bools
        .support(b)
        .into_iter()
        .all(|a| atom_coherent(st, values, a, 0))
}

/// Whether `inst` writes LDS: stores and atomics, and buffer loads into LDS; not reads,
/// swizzles or permutes.
fn writes_lds(inst: &Inst) -> bool {
    let n = inst.name();
    match inst.fields {
        Fields::Ds { gds: false, .. } => {
            !(n.starts_with("ds_read")
                || n.starts_with("ds_swizzle")
                || n.contains("permute")
                || n == "ds_nop")
        }
        Fields::Buffer { lds, .. } => lds,
        _ => false,
    }
}

/// The thread id components (by their input value) that every lane of a console wave holds
/// alike, and those whose bits from 6 up are: the lanes of a console wave are 64 threads in a
/// row of the group, x first.
fn thread_ids(io: &Io, threads: [u32; 3]) -> (HashSet<Value>, HashSet<Value>) {
    let [x, y, z] = threads;
    let (mut whole, mut high) = (HashSet::new(), HashSet::new());
    for &(v, input) in &io.inputs {
        let Input::ThreadIdInGroup(c) = input else {
            continue;
        };
        let alike = match c {
            // x: the wave's 64 threads hold 64 consecutive ids (bits 6 up alike) when a row
            // holds whole waves, or when there is only one row
            0 => {
                if x % 64 == 0 || y * z == 1 {
                    high.insert(v);
                }
                false
            }
            1 => x % 64 == 0 || y == 1,
            2 => (x * y) % 64 == 0 || z == 1,
            _ => false,
        };
        if alike {
            whole.insert(v);
        }
    }
    (whole, high)
}

/// The value of the register an operand names, as `inst` reads it (its first dword).
fn read(inst: &Inst, op: &Operand) -> Option<Value> {
    match *op {
        Operand::Vector { reg, .. } => inst.read(Reg::V(reg)),
        Operand::Scalar { code, .. } if code != NULL => inst.read(Reg::S(code)),
        _ => None,
    }
}

/// A constant operand's value.
fn constant(op: &Operand) -> Option<u32> {
    match *op {
        Operand::Const(c) => crate::resource::inline(c),
        Operand::Literal(x) => Some(x),
        _ => None,
    }
}

/// Finds what the two host waves of each console wave hold alike in `p` (a compute
/// program of `threads` threads; see the module notes).
pub(super) fn coherence(p: &Program, st: &Structured, io: &Io, threads: [u32; 3]) -> Coherence {
    let n = p.blocks.len();
    let regions = super::lower::lane_regions(st).unwrap_or_default();
    let (ids_alike, ids_high) = thread_ids(io, threads);
    // `all`: blocks every lane runs (no lane branch has them in its region)
    let mut all = vec![true; n];
    for (_, region) in &regions {
        for b in (0..n).filter(|&b| region[b]) {
            all[b] = false;
        }
    }
    let lds_stable = lds_stable(p, &all);
    let is_mask = |v: Value| st.masks.contains_key(&v) || st.highs.contains_key(&v);
    let order = crate::ir::reverse_postorder(&p.blocks);

    // start: everything coherent that can be; then take back until nothing changes
    let mut values: HashSet<Value> = HashSet::new();
    for block in &p.blocks {
        values.extend(
            block
                .phis
                .iter()
                .filter(|f| matches!(f.reg, Reg::V(_)))
                .map(|f| f.value),
        );
        for inst in &block.insts {
            values.extend(inst.writes.iter().filter(|w| w.0 != Reg::Scc).map(|w| w.1));
        }
    }
    values.extend(ids_alike.iter().copied());
    let mut blocks = vec![true; n];
    loop {
        let mut changed = false;
        // blocks: every lane branch whose region holds the block decides alike, and what
        // holds in every lane that enters it (a `v_cmpx` before a jump) is alike
        for b in 0..n {
            let ok = regions.iter().all(|(d, region)| {
                !region[b]
                    || matches!(st.exits[*d], LaneExit::Branch { when, .. }
                        if bool_coherent(st, &values, when))
            }) && st
                .reach
                .get(b)
                .is_some_and(|&r| bool_coherent(st, &values, r));
            if blocks[b] && !ok {
                blocks[b] = false;
                changed = true;
            }
        }
        let mut drop: Vec<Value> = Vec::new();
        for &b in &order {
            let block = &p.blocks[b];
            // a join: its lanes came the same way when no lane branch that split them
            // decides differently in a console wave
            let joined = block.preds.iter().all(|&q| blocks[q]);
            for phi in block.phis.iter().filter(|f| matches!(f.reg, Reg::V(_))) {
                if values.contains(&phi.value)
                    && !(joined && phi.args.iter().all(|a| values.contains(a)))
                {
                    drop.push(phi.value);
                }
            }
            for (i, inst) in block.insts.iter().enumerate() {
                let written: Vec<Value> = inst
                    .writes
                    .iter()
                    .filter(|w| w.0 != Reg::Scc && values.contains(&w.1))
                    .map(|w| w.1)
                    .collect();
                if written.is_empty() {
                    continue;
                }
                let ok = written_alike(inst, st, &values, &ids_high, &is_mask, b, i, &lds_stable);
                if !ok {
                    drop.extend(written);
                }
            }
        }
        for v in drop {
            changed |= values.remove(&v);
        }
        if !changed {
            break;
        }
    }
    Coherence { values, blocks }
}

/// Whether what `inst` (instruction `i` of block `b`) writes is alike in every lane of a
/// console wave, given that `values` are.
#[allow(clippy::too_many_arguments)]
fn written_alike(
    inst: &Inst,
    st: &Structured,
    values: &HashSet<Value>,
    ids_high: &HashSet<Value>,
    is_mask: &impl Fn(Value) -> bool,
    b: usize,
    i: usize,
    lds_stable: &[Vec<bool>],
) -> bool {
    let name = inst.name();
    // a scalar operand is one value for the wave; a lane mask is a bit per lane. Every
    // scalar the instruction reads counts, the implicit ones too (VCC of a `v_cndmask` or a
    // carry in).
    // (a mask's high dword by its low dword's lanes)
    let scalar_alike = |x: Value| {
        let low = st.highs.get(&x).copied().unwrap_or(x);
        !is_mask(x)
            || st
                .masks
                .get(&low)
                .is_some_and(|&m| bool_coherent(st, values, m))
    };
    // (EXEC is the lanes that run, which the guard and the block rules judge)
    let scalars_alike = inst
        .reads
        .iter()
        .all(|&(r, x)| !matches!(r, Reg::S(c) if c != EXEC_LO && c != EXEC_HI) || scalar_alike(x));
    let operand_alike = |op: &Operand| match *op {
        Operand::Vector { reg, .. } => {
            let read: Vec<Value> = inst
                .reads
                .iter()
                .filter(|r| matches!(r.0, Reg::V(x) if x >= reg))
                .map(|r| r.1)
                .collect();
            !read.is_empty() && read.iter().all(|x| values.contains(x))
        }
        Operand::Scalar { code, .. } if code != NULL => {
            inst.read(Reg::S(code)).is_none_or(scalar_alike)
        }
        // an attribute's interpolation differs from lane to lane
        Operand::Attr { .. } => false,
        _ => true,
    };
    // lanes outside the guard keep the old value: alike if the guard decides alike in a
    // console wave and the old value is alike
    if let Some(g) = st.guards[b][i] {
        if !bool_coherent(st, values, g) {
            return false;
        }
        let old_alike = inst.writes.iter().all(|&(r, _)| {
            !matches!(r, Reg::V(_)) || inst.read(r).is_none_or(|x| values.contains(&x))
        });
        if !old_alike {
            return false;
        }
    }
    match &inst.fields {
        Fields::Valu(v) => {
            if v.dpp.is_some()
                || name.starts_with("v_mbcnt")
                || name.contains("lane")
                || name.starts_with("v_interp")
                || !scalars_alike
            {
                return false;
            }
            // the console wave's number: a thread id's bits from 6 up
            let shifted = |src: &Operand, by: u32| {
                by >= 6 && read(inst, src).is_some_and(|x| ids_high.contains(&x))
            };
            match (name, v.src.as_slice()) {
                ("v_lshrrev_b32", [by, src])
                    if constant(by).is_some_and(|k| shifted(src, k & 31)) =>
                {
                    return true;
                }
                ("v_bfe_u32", [src, off, _width])
                    if constant(off).is_some_and(|k| shifted(src, k & 31)) =>
                {
                    return operand_alike(&v.src[2]);
                }
                _ => {}
            }
            v.src.iter().all(operand_alike)
        }
        // an LDS read at an address alike in the console wave, where LDS no longer changes
        // (not `ds_read_addtid`, whose address is the lane's number)
        Fields::Ds {
            addr, gds: false, ..
        } if name.starts_with("ds_read") && !name.contains("addtid") => {
            lds_stable[b][i] && scalars_alike && addr.as_ref().is_none_or(operand_alike)
        }
        _ => false,
    }
}

/// Per instruction: whether LDS no longer changes where it stands. A group barrier that every
/// lane runs dominates it, and no instruction that writes LDS can run after that barrier, so
/// every wave of the group wrote all it writes before any wave got there.
fn lds_stable(p: &Program, all: &[bool]) -> Vec<Vec<bool>> {
    let n = p.blocks.len();
    let order = crate::ir::reverse_postorder(&p.blocks);
    let mut index = vec![usize::MAX; n];
    for (k, &b) in order.iter().enumerate() {
        index[b] = k;
    }
    let idom = crate::ir::idoms(&p.blocks, &order, &index);
    let dominates = |a: usize, mut b: usize| loop {
        if a == b {
            return true;
        }
        let d = idom.get(b).copied().unwrap_or(usize::MAX);
        if d == b || d == usize::MAX {
            return false;
        }
        b = d;
    };
    // blocks reachable from the end of each block (successors and on)
    let reach_from = |b: usize| {
        let mut seen = vec![false; n];
        let mut stack = p.blocks[b].exit.successors();
        while let Some(x) = stack.pop() {
            if !std::mem::replace(&mut seen[x], true) {
                stack.extend(p.blocks[x].exit.successors());
            }
        }
        seen
    };
    let writes_in = |b: usize, from: usize| p.blocks[b].insts[from..].iter().any(writes_lds);
    // the barriers past which nothing writes LDS
    let mut quiet: Vec<(usize, usize)> = Vec::new();
    for (b, block) in p.blocks.iter().enumerate() {
        if !all.get(b).copied().unwrap_or(false) {
            continue;
        }
        for (i, inst) in block.insts.iter().enumerate() {
            if inst.name() != "s_barrier" {
                continue;
            }
            let later = reach_from(b);
            if !writes_in(b, i + 1) && !(0..n).any(|x| later[x] && writes_in(x, 0)) {
                quiet.push((b, i));
            }
        }
    }
    (0..n)
        .map(|b| {
            (0..p.blocks[b].insts.len())
                .map(|i| {
                    quiet
                        .iter()
                        .any(|&(qb, qi)| if qb == b { qi < i } else { dominates(qb, b) })
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
pub(super) mod tests {
    use std::collections::HashSet;

    use crate::native::{
        ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
    };
    use crate::{ShaderError, io, ir, resource, structure};

    /// The sums before each console wave, in a group of `threads` threads (in x) of console
    /// waves of 64, the way the console compiler writes a group-wide prefix: lane 63 of
    /// console wave `w` puts `s0 + 16 w` into LDS dword `w`; after a group barrier, the
    /// lanes of wave `w > 0` loop over the waves before theirs, keeping the running sum in a
    /// scalar that `v_readfirstlane` takes from a lane still in the loop, and leave when the
    /// loop reaches their wave. Thread `t` stores its sum to UAV dword `t`. The wave number
    /// is `t >> shift` (6: the console wave's number).
    pub(in super::super) fn group_prefix(shift: u32, threads: u32) -> NativeProgram {
        let body: [u32; 38] = [
            0x2C0E_0080 | shift, // v_lshrrev_b32 v7, shift, v0       (the wave number)
            0xD765_0009,
            0x0001_00C1, // v_mbcnt_lo_u32_b32 v9, -1, 0
            0xD766_0009,
            0x0002_12C1, // v_mbcnt_hi_u32_b32 v9, -1, v9     (the console lane)
            0x3410_0E82, // v_lshlrev_b32 v8, 2, v7
            0x340C_0E84, // v_lshlrev_b32 v6, 4, v7
            0x4A0C_0C00, // v_add_nc_u32 v6, s0, v6           (the wave's total)
            0xBE94_047E, // s_mov_b64 s[20:21], exec
            0x7DA4_12BF, // v_cmpx_eq_u32 exec, 63, v9
            0xD834_0000,
            0x0000_0608, // ds_write_b32 v8, v6
            0xBEFE_0414, // s_mov_b64 exec, s[20:21]
            0xBF8A_0000, // s_barrier
            0x7E14_0280, // v_mov_b32 v10, 0
            0xBE96_047E, // s_mov_b64 s[22:23], exec
            0x7DAA_0E80, // v_cmpx_ne_u32 exec, 0, v7
            0xBF88_0010, // s_cbranch_execz RESTORE
            0xBE98_047E, // s_mov_b64 s[24:25], exec              (the lanes still looping)
            0xBE92_0380, // s_mov_b32 s18, 0
            0xBE91_0380, // s_mov_b32 s17, 0                      (the running sum)
            0xBF82_0002, // s_branch HEAD
            // LATCH:
            0x87FE_181A, // s_and_b64 exec, s[26:27], s[24:25]
            0x7E22_050A, // v_readfirstlane_b32 s17, v10
            // HEAD:
            0x8F10_8212, // s_lshl_b32 s16, s18, 2
            0x8112_8112, // s_add_i32 s18, s18, 1
            0x7E16_0210, // v_mov_b32 v11, s16
            0xD8D8_0000,
            0x0B00_000B, // ds_read_b32 v11, v11
            0x4A14_1611, // v_add_nc_u32 v10, s17, v11
            0xBE9A_047E, // s_mov_b64 s[26:27], exec
            0x7DA4_0E12, // v_cmpx_eq_u32 exec, s18, v7
            0x8A98_7E18, // s_andn2_b64 s[24:25], s[24:25], exec
            0xBF85_FFF4, // s_cbranch_scc1 LATCH
            // RESTORE:
            0xBEFE_0416, // s_mov_b64 exec, s[22:23]
            0x3418_0082, // v_lshlrev_b32 v12, 2, v0
            0xE070_1000,
            0x8002_0A0C, // buffer_store_dword v10, v12, s[8:11], 0 offen
        ];
        let mut words = vec![
            0xBE85_0380, // s_mov_b32 s5, 0
            0x3D << 26 | 0x02 << 18 | 8 << 6 | 2,
            125 << 25, // s_load_dwordx4 s[8:11], s[4:5], 0
        ];
        words.extend_from_slice(&body);
        words.push(0xBF81_0000); // s_endpgm
        let lds = (threads / 64).max(1) * 4;
        NativeProgram {
            stage: HwStage::Cs,
            wave64: true,
            vgprs: 16,
            layout: vec![
                RootParam {
                    root: RootType::Constants,
                    table_offset_dwords: 0,
                    sgprs: 4,
                    base_register: 0,
                    user_data: 0,
                    slot: 0,
                },
                RootParam {
                    root: RootType::Table,
                    table_offset_dwords: 0,
                    sgprs: 1,
                    base_register: 0,
                    user_data: 4,
                    slot: 1,
                },
            ],
            used: vec![UsedResource {
                kind: ResourceType::Uav,
                root: RootType::Table,
                table_offset_dwords: 0,
                lower_bound: 0,
                stride: 0,
                dwords: 4,
                user_data: 4,
                slot: 1,
            }],
            io: StageIo::Cs(ComputeIo {
                // 5 user SGPRs (s0..s4), LDS in blocks of 512 bytes
                rsrc2: 5 << 1 | lds.div_ceil(512) << 15,
                threads: [threads, 1, 1],
            }),
            code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
        }
    }

    /// Lowers `native` for a host of 32-lane waves.
    pub(in super::super) fn lower32(
        native: &NativeProgram,
    ) -> Result<super::super::lower::Lowered, ShaderError> {
        let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
        let st = structure::structure_wide(&p, &HashSet::new()).expect("structure");
        let res =
            resource::track(&p, &native.layout, native.stage, Some(&st)).expect("resource::track");
        let io = io::raise(&p, native).expect("io::raise");
        let lower = super::super::lower::TEST_HOST_WAVE.replace(Some(32));
        let lowered = super::super::lower::lower(&super::super::lower::Parts {
            native,
            p: &p,
            st: &st,
            io: &io,
            res: &res,
            inputs: Vec::new(),
            outputs: Vec::new(),
            prims: Vec::new(),
            mesh_stride: 0,
        });
        super::super::lower::TEST_HOST_WAVE.set(lower);
        lowered
    }

    /// The program decodes as written: every instruction is the one its comment names, and
    /// the branches land where the comments say.
    #[test]
    fn the_group_prefix_program_decodes_as_written() {
        let native = group_prefix(6, 256);
        let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
        let text: Vec<String> = p
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter().map(|i| i.machine.text(true)))
            .collect();
        let want = [
            "s_mov_b32 s5, 0",
            "s_load_dwordx4 s[8:11], s[4:5], null",
            "v_lshrrev_b32 v7, 6, v0",
            "v_mbcnt_lo_u32_b32 v9, -1, 0",
            "v_mbcnt_hi_u32_b32 v9, -1, v9",
            "v_lshlrev_b32 v8, 2, v7",
            "v_lshlrev_b32 v6, 4, v7",
            "v_add_nc_u32 v6, s0, v6",
            "s_mov_b64 s[20:21], exec",
            "v_cmpx_eq_u32 exec, 63, v9",
            "ds_write_b32 v8, v6",
            "s_mov_b64 exec, s[20:21]",
            "s_barrier",
            "v_mov_b32 v10, 0",
            "s_mov_b64 s[22:23], exec",
            "v_cmpx_ne_u32 exec, 0, v7",
            "s_cbranch_execz label_0094",
            "s_mov_b64 s[24:25], exec",
            "s_mov_b32 s18, 0",
            "s_mov_b32 s17, 0",
            "s_branch label_006C",
            "s_and_b64 exec, s[26:27], s[24:25]",
            "v_readfirstlane_b32 s17, v10",
            "s_lshl_b32 s16, s18, 2",
            "s_add_i32 s18, s18, 1",
            "v_mov_b32 v11, s16",
            "ds_read_b32 v11, v11",
            "v_add_nc_u32 v10, s17, v11",
            "s_mov_b64 s[26:27], exec",
            "v_cmpx_eq_u32 exec, s18, v7",
            "s_andn2_b64 s[24:25], s[24:25], exec",
            "s_cbranch_scc1 label_0064",
            "s_mov_b64 exec, s[22:23]",
            "v_lshlrev_b32 v12, 2, v0",
            "buffer_store_dword v10, v12, s[8:11], 0 offen",
            "s_endpgm",
        ];
        assert_eq!(text, want);
    }

    /// On a host of 32-lane waves, the loop's first-lane read needs nothing from the other
    /// host wave: the value and the loop's lanes are the same in all 64 lanes of a console
    /// wave. Before, it was refused ("a first-lane read where not every lane runs").
    #[test]
    fn a_first_lane_read_alike_in_the_console_wave_needs_no_exchange() {
        let native = group_prefix(6, 256);
        super::super::lower::take_needs();
        let lowered = lower32(&native);
        assert!(lowered.is_ok(), "{:?}", lowered.err());
    }

    /// The same with the loop going by `t >> 5`, which differs between the two halves of a
    /// console wave: still refused, the halves would each read their own first lane.
    #[test]
    fn a_first_lane_read_that_differs_between_halves_is_still_refused() {
        let native = group_prefix(5, 256);
        super::super::lower::take_needs();
        let lowered = lower32(&native);
        assert!(
            matches!(&lowered, Err(ShaderError::Unsupported { what, .. })
                if *what == "a first-lane read where not every lane runs"),
            "{:?}",
            lowered.as_ref().err()
        );
        super::super::lower::take_needs();
    }

    /// A group whose rows hold a part of a console wave (32 threads in x, 8 rows): `t >> 6`
    /// is not taken as the console wave's number, and the read is refused as before.
    #[test]
    fn rows_shorter_than_a_console_wave_keep_the_exchange() {
        let mut native = group_prefix(6, 256);
        if let StageIo::Cs(cs) = &mut native.io {
            cs.threads = [32, 8, 1];
        }
        super::super::lower::take_needs();
        let lowered = lower32(&native);
        assert!(lowered.is_err());
        super::super::lower::take_needs();
    }
}
