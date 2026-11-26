//! Lowering added in CCS03: its own pull requests and those it took from
//! other forks, kept out of `lower.rs` so that file stays under the asset
//! policy's size limit. A child module of `lower`, so it shares its state.

use super::*;

impl Lower<'_> {
    /// Whether a read of another lane of this lane's quad (a quad swizzle
    /// or a quad permutation) must reach helper lanes: in a pixel shader,
    /// always. At a triangle's edge the quad's other pixels are helpers,
    /// which only quad reads see; a wave read of a helper lane gives
    /// nothing defined. A derivative the title takes by hand then was the
    /// whole value, the texture read that used it took its smallest level,
    /// and triangles' edges showed as lines on car paint, glass and skin.
    /// After Belobug/VIctory-Belobug-AMD 03629f3, whose lowering this
    /// tree's does not share (the two quad sites only).
    pub(super) fn helpers_read(&self, quad: bool) -> bool {
        quad && self.s.pixel
    }

    /// `x * 2^e` for any i32 `e`, rounded once unless the result is a
    /// denormal. DXIL has no ldexp: `x` is multiplied by three powers of
    /// two that are each a normal float. Past ±375 any finite nonzero `x`
    /// is already at zero or infinity, so `e` is clamped to [-375, 378],
    /// which keeps each of q, q and e - 2q (q = e / 3) inside [-126, 127].
    pub(super) fn ldexp(&mut self, x: V, e: V) -> V {
        let c = self.cur;
        let (lo, hi) = (self.s.m.i32(-375i32 as u32), self.s.m.i32(378));
        let below = self.s.m.cmp(c, pred::SLT, e, lo);
        let e = self.s.m.select(c, below, lo, e);
        let above = self.s.m.cmp(c, pred::SGT, e, hi);
        let e = self.s.m.select(c, above, hi, e);
        let three = self.s.m.i32(3);
        let q = self.s.m.bin(c, Bin::SDiv, e, three);
        let two_q = self.s.m.bin(c, Bin::Add, q, q);
        let rest = self.s.m.bin(c, Bin::Sub, e, two_q);
        let mut r = x;
        for k in [q, q, rest] {
            // 2^k for k in [-126, 127]: the biased exponent alone.
            let (bias, at) = (self.s.m.i32(127), self.s.m.i32(23));
            let biased = self.s.m.bin(c, Bin::Add, k, bias);
            let bits = self.s.m.bin(c, Bin::Shl, biased, at);
            let factor = self.as_float(bits);
            r = self.s.m.bin(c, Bin::Mul, r, factor);
        }
        r
    }

    /// The significand (bits of a float with magnitude in [0.5, 1), the
    /// sign kept) and the exponent of the f32 with bits `x`, so that
    /// x = significand * 2^exponent, worked on the bits so that denormals
    /// are normalized whatever the host's float mode. Zero gives itself and
    /// 0; infinity and NaN give themselves and 0.
    pub(super) fn frexp(&mut self, x: V) -> (V, V) {
        let c = self.cur;
        let k = |l: &mut Self, v: u32| l.s.m.i32(v);
        let (at, ff) = (k(self, 23), k(self, 0xFF));
        let field = self.s.m.bin(c, Bin::LShr, x, at);
        let field = self.s.m.bin(c, Bin::And, field, ff);
        let frac_mask = k(self, 0x7F_FFFF);
        let frac = self.s.m.bin(c, Bin::And, x, frac_mask);
        let sign_mask = k(self, 0x8000_0000);
        let sign = self.s.m.bin(c, Bin::And, x, sign_mask);
        // A denormal's leading one at bit 31 - lz moves to bit 23.
        let lz = self.s.op(
            c,
            "unaryBits.i32",
            Attr::ReadNone,
            self.s.t.i32,
            33,
            &[frac],
        );
        let (eight, k31) = (k(self, 8), k(self, 31));
        let sh = self.s.m.bin(c, Bin::Sub, lz, eight);
        let sh = self.s.m.bin(c, Bin::And, sh, k31);
        let normalized = self.s.m.bin(c, Bin::Shl, frac, sh);
        let normalized = self.s.m.bin(c, Bin::And, normalized, frac_mask);
        let zero = k(self, 0);
        let denormal = self.s.m.cmp(c, pred::EQ, field, zero);
        let frac = self.s.m.select(c, denormal, normalized, frac);
        let half = k(self, 0x3F00_0000);
        let m = self.s.m.bin(c, Bin::Or, sign, half);
        let m = self.s.m.bin(c, Bin::Or, m, frac);
        // Exponents: field - 126, or -117 - lz for a denormal.
        let (k126, k117) = (k(self, 126), k(self, -117i32 as u32));
        let e_normal = self.s.m.bin(c, Bin::Sub, field, k126);
        let e_denormal = self.s.m.bin(c, Bin::Sub, k117, lz);
        let e = self.s.m.select(c, denormal, e_denormal, e_normal);
        // Zero, infinity and NaN.
        let magnitude = k(self, 0x7FFF_FFFF);
        let magnitude = self.s.m.bin(c, Bin::And, x, magnitude);
        let is_zero = self.s.m.cmp(c, pred::EQ, magnitude, zero);
        let special = self.s.m.cmp(c, pred::EQ, field, ff);
        let keep = self.s.m.bin(c, Bin::Or, is_zero, special);
        let m = self.s.m.select(c, keep, x, m);
        let e = self.s.m.select(c, keep, zero, e);
        (m, e)
    }

    /// Registers an immediate sampler constant and returns its slot index.
    pub(super) fn register_sampler(&mut self, words: &[u32], cmp: bool) -> Result<u32, Why> {
        let words: [u32; 4] = words
            .get(..4)
            .and_then(|w| w.try_into().ok())
            .ok_or("a sampler constant")?;
        let n = match self.samplers.iter().position(|w| *w == words) {
            Some(n) => n,
            None => {
                self.samplers.push(words);
                self.samplers.len() - 1
            }
        };
        if cmp && n < 64 {
            self.s.cmp_samplers |= 1 << n;
        }
        Ok(n as u32)
    }

    /// Resolves a control-flow phi of immediate sampler constants into either a
    /// single static sampler register (`One`) or a uniform `i32` phi index over
    /// `regs` (`Many`) for branch dispatch at the sample site.
    pub(super) fn sampler_phi_choice(
        &mut self,
        block: usize,
        args: &[Source],
        cmp: bool,
        depth: u32,
    ) -> Result<(Option<V>, Vec<u32>), Why> {
        const WHY: Why = "a descriptor chosen by control flow";
        let mut distinct: Vec<[u32; 4]> = Vec::new();
        self.collect_constant_samplers(block, args, depth, &mut distinct)?;
        if distinct.is_empty() {
            return Err(WHY);
        }
        let mut regs = Vec::with_capacity(distinct.len());
        for w in &distinct {
            regs.push(self.register_sampler(w, cmp)?);
        }
        if regs.len() == 1 {
            return Ok((None, regs));
        }
        let idx = self.sampler_index_phi(block, args, 0, &distinct, depth)?;
        Ok((Some(idx), regs))
    }

    pub(super) fn collect_constant_samplers(
        &self,
        block: usize,
        args: &[Source],
        depth: u32,
        distinct: &mut Vec<[u32; 4]>,
    ) -> Result<(), Why> {
        const WHY: Why = "a descriptor chosen by control flow";
        let p = self.x.p;
        for (&q, arg) in p.blocks[block].preds.iter().zip(args) {
            if self.block[q].is_none() {
                continue;
            }
            match arg {
                Source::Constant(w) if w.len() >= 4 => {
                    let k: [u32; 4] = w[..4].try_into().unwrap();
                    if !distinct.contains(&k) {
                        distinct.push(k);
                    }
                }
                Source::Phi {
                    block: inner,
                    args: inner_args,
                } if depth > 0 => {
                    self.collect_constant_samplers(*inner, inner_args, depth - 1, distinct)?;
                }
                _ => return Err(WHY),
            }
        }
        Ok(())
    }

    pub(super) fn sampler_index_phi(
        &mut self,
        block: usize,
        args: &[Source],
        lower: u32,
        distinct: &[[u32; 4]],
        depth: u32,
    ) -> Result<V, Why> {
        const WHY: Why = "a descriptor chosen by control flow";
        let p = self.x.p;
        let mut incoming = Vec::new();
        for (&q, arg) in p.blocks[block].preds.iter().zip(args) {
            if self.block[q].is_none() {
                continue;
            }
            let tail = self.tail[q].ok_or(WHY)?;
            let at = self.edges.get(&(q, block)).copied().unwrap_or(tail);
            let idx = match arg {
                Source::Constant(w) if w.len() >= 4 => {
                    let k: [u32; 4] = w[..4].try_into().unwrap();
                    let pos = distinct.iter().position(|d| *d == k).ok_or(WHY)? as u32;
                    self.s.m.i32(lower + pos)
                }
                Source::Phi {
                    block: inner,
                    args: inner_args,
                } if depth > 0 => {
                    self.sampler_index_phi(*inner, inner_args, lower, distinct, depth - 1)?
                }
                _ => return Err(WHY),
            };
            incoming.push((idx, at));
        }
        let join = self.block[block].ok_or(WHY)?;
        let phi = self.s.m.phi(join, self.s.t.i32);
        self.s.m.set_phi(phi, incoming);
        Ok(phi)
    }

    /// A 64-bit float source as its bits in an i64: a register pair, an
    /// inline constant (the float ones as f64 values, the integer ones
    /// sign-extended) or a literal (its high dword; the low one is 0), with
    /// `abs` and `neg` applied to the sign bit.
    pub(super) fn f64_src(&mut self, inst: &Inst, k: usize) -> Result<V, Why> {
        let Fields::Valu(v) = &inst.fields else {
            unreachable!()
        };
        if v.sdwa.is_some() || v.dpp.is_some() {
            return Err("64-bit operand modifiers");
        }
        let c = self.cur;
        let i64t = self.s.m.int(64);
        self.s.flags |= flags::INT64;
        let op = *v.src.get(k).ok_or("a missing operand")?;
        let mut x = match op {
            Operand::Const(code @ 240..=248) => {
                let bits = f64_inline(code).ok_or("a special operand")?;
                self.s.m.int_const(i64t, bits as i64)
            }
            Operand::Literal(hi) => self.s.m.int_const(i64t, (u64::from(hi) << 32) as i64),
            Operand::Vector { reg, .. } => {
                let lo = self.i32_of(inst.read(Reg::V(reg)).ok_or("an unread operand")?)?;
                let hi = self.i32_of(inst.read(Reg::V(reg + 1)).ok_or("an unread operand")?)?;
                self.join64(lo, hi)
            }
            Operand::Scalar { code, .. } if code != NULL => {
                let lo = self.i32_of(inst.read(Reg::S(code)).ok_or("an unread operand")?)?;
                let hi = self.i32_of(inst.read(Reg::S(code + 1)).ok_or("an unread operand")?)?;
                self.join64(lo, hi)
            }
            Operand::Const(code @ 128..=208) => {
                let n = crate::resource::inline(code).ok_or("a special operand")?;
                self.s.m.int_const(i64t, i64::from(n as i32))
            }
            _ => return Err("a 64-bit float operand"),
        };
        let sign = self.s.m.int_const(i64t, i64::MIN);
        if v.abs >> k & 1 != 0 {
            let keep = self.s.m.int_const(i64t, i64::MAX);
            x = self.s.m.bin(c, Bin::And, x, keep);
        }
        if v.neg >> k & 1 != 0 {
            x = self.s.m.bin(c, Bin::Xor, x, sign);
        }
        Ok(x)
    }

    /// The bits of the f32 nearest an f64 (`x`, its bits in an i64), ties
    /// to even, as `v_cvt_f32_f64` rounds: what the free `f32_bits_of_f64`
    /// (a test's reference) does on the CPU, step for step. Denormal
    /// results are kept; f64 denormals come out as zeros of their sign; a
    /// NaN stays a NaN (quiet, its sign and its payload's high bits kept);
    /// past the largest f32 is infinity.
    pub(super) fn f32_bits_of_f64(&mut self, x: V) -> V {
        let c = self.cur;
        let m = &mut self.s.m;
        let i64t = m.int(64);
        let k = |m: &mut crate::emit::bitcode::Module, v: i64| m.int_const(i64t, v);
        let (k0, k1, k23, k29, k34, k52, k63) =
            (k(m, 0), k(m, 1), k(m, 23), k(m, 29), k(m, 34), k(m, 52), k(m, 63));
        let sign = m.bin(c, Bin::LShr, x, k63);
        let field = m.bin(c, Bin::LShr, x, k52);
        let k7ff = k(m, 0x7ff);
        let exp = m.bin(c, Bin::And, field, k7ff);
        let low52 = k(m, (1 << 52) - 1);
        let mant = m.bin(c, Bin::And, x, low52);
        let one52 = k(m, 1 << 52);
        let sig = m.bin(c, Bin::Or, mant, one52);
        // The f32 exponent the value would have, unbiased by 1023, biased by 127.
        let k896 = k(m, 896);
        let e = m.bin(c, Bin::Sub, exp, k896);
        // Below the normal range the significand shifts out further (a
        // denormal result); by 63 at most, which leaves nothing of it.
        let under = m.bin(c, Bin::Sub, k1, e);
        let pos = m.cmp(c, pred::SGT, under, k0);
        let under = m.select(c, pos, under, k0);
        let far = m.cmp(c, pred::SGT, under, k34);
        let under = m.select(c, far, k34, under);
        let shift = m.bin(c, Bin::Add, k29, under);
        let q = m.bin(c, Bin::LShr, sig, shift);
        let unit = m.bin(c, Bin::Shl, k1, shift);
        let below = m.bin(c, Bin::Sub, unit, k1);
        let rest = m.bin(c, Bin::And, sig, below);
        let less = m.bin(c, Bin::Sub, shift, k1);
        let half = m.bin(c, Bin::Shl, k1, less);
        // Up past half, or at half when the kept bits are odd.
        let odd = m.bin(c, Bin::And, q, k1);
        let rest = m.bin(c, Bin::Add, rest, odd);
        let up = m.cmp(c, pred::IUGT, rest, half);
        let up = m.cast(c, Cast::ZExt, up, i64t);
        // The exponent field less the significand's leading bit, which q
        // carries for a normal result; a carry out of rounding moves it up.
        let normal = m.cmp(c, pred::SGT, e, k1);
        let lower = m.bin(c, Bin::Sub, e, k1);
        let field = m.select(c, normal, lower, k0);
        let field = m.bin(c, Bin::Shl, field, k23);
        let bits = m.bin(c, Bin::Add, field, q);
        let bits = m.bin(c, Bin::Add, bits, up);
        let k255 = k(m, 255);
        let huge = m.cmp(c, pred::SGE, e, k255);
        let inf = k(m, 0x7f80_0000);
        let bits = m.select(c, huge, inf, bits);
        let all = m.cmp(c, pred::EQ, exp, k7ff);
        let payload = m.cmp(c, pred::NE, mant, k0);
        let nan = m.bin(c, Bin::And, all, payload);
        let top = m.bin(c, Bin::LShr, mant, k29);
        let quiet = k(m, 0x7fc0_0000);
        let nan_bits = m.bin(c, Bin::Or, top, quiet);
        let bits = m.select(c, nan, nan_bits, bits);
        let k31 = k(m, 31);
        let sign = m.bin(c, Bin::Shl, sign, k31);
        let bits = m.bin(c, Bin::Or, bits, sign);
        let i32t = self.s.t.i32;
        self.s.m.cast(c, Cast::Trunc, bits, i32t)
    }
}

/// Whether a pixel program runs part of itself in primitive order (POPS):
/// it sets the ordering unit's packer register, or reads the exiting wave id.
///
/// The console's ordered section: s18 is the collision wave id (bits 9:0 this
/// wave's id, 25:16 the id the wave must wait behind, 29:28 the packer mode,
/// bit 31 set when an earlier wave may overlap). The program sets the packer
/// register, spins until `src_pops_exiting_wave_id` passes the earlier wave's
/// id, then does its read-modify-write on UAV images. The host's equivalent
/// is a rasterizer-ordered view, so the program is lowered as the first wave
/// (s18 is 0: bit 31 clear skips the wait), the packer write is dropped, and
/// every UAV it opens is marked rasterizer ordered.
/// shortcut: all UAVs of the program, not only those the section touches.
pub(super) fn enters_pops(p: &Program) -> bool {
    p.blocks.iter().flat_map(|b| &b.insts).any(|i| {
        let Fields::Sop { src, imm, .. } = &i.fields else {
            return false;
        };
        (matches!(i.name(), "s_setreg_imm32_b32" | "s_setreg_b32")
            && imm.is_some_and(|m| m & 0x3F == HW_REG_POPS_PACKER))
            || src.iter().any(|o| matches!(o, Operand::Const(POPS_EXITING_WAVE_ID)))
    })
}

/// The f64 bits of a float inline constant (codes 240-248), as a 64-bit
/// float operand reads it.
fn f64_inline(code: u8) -> Option<u64> {
    Some(match code {
        240 => 0.5f64.to_bits(),
        241 => (-0.5f64).to_bits(),
        242 => 1.0f64.to_bits(),
        243 => (-1.0f64).to_bits(),
        244 => 2.0f64.to_bits(),
        245 => (-2.0f64).to_bits(),
        246 => 4.0f64.to_bits(),
        247 => (-4.0f64).to_bits(),
        // 1 / (2 pi)
        248 => 0x3FC4_5F30_6DC9_C882,
        _ => return None,
    })
}

/// What `Lower::f32_bits_of_f64` emits, on the CPU and in the same steps:
/// the f32 nearest an f64, ties to even, NaNs quiet with their sign and
/// the high bits of their payload.
#[cfg(test)]
pub(crate) fn f32_bits_of_f64(x: u64) -> u32 {
    let sign = x >> 63;
    let exp = (x >> 52 & 0x7ff) as i64;
    let mant = x & ((1 << 52) - 1);
    let sig = mant | 1 << 52;
    let e = exp - 896;
    let under = (1 - e).clamp(0, 34);
    let shift = 29 + under as u32;
    let q = sig >> shift;
    let rest = sig & ((1 << shift) - 1);
    let half = 1u64 << (shift - 1);
    let up = u64::from(rest + (q & 1) > half);
    let field = if e > 1 { (e - 1) as u64 } else { 0 };
    let mut bits = (field << 23) + q + up;
    if e >= 255 {
        bits = 0x7f80_0000;
    }
    if exp == 0x7ff && mant != 0 {
        bits = mant >> 29 | 0x7fc0_0000;
    }
    (bits | sign << 31) as u32
}

impl Lower<'_> {
    // The image instructions with the sampler they use: lowered here, out of
    // `lower.rs` for its size (the body is victory-fork's `image`, with the
    // sampler given, so a uniform branch can choose between two).
    pub(super) fn image_with_sampler(
        &mut self,
        inst: &Inst,
        b: usize,
        i: usize,
        sampler: Option<&Source>,
    ) -> Result<(), Why> {
        let Fields::Image {
            data,
            addr,
            dmask,
            dim,
            unorm,
            a16,
            d16,
            tfe,
            lwe,
            ..
        } = &inst.fields
        else {
            unreachable!()
        };
        if *tfe || *lwe {
            return Err("an image access with tfe or lwe");
        }
        let name = inst.name();
        // A load or sample with no channel enabled returns the first.
        let dmask = if *dmask == 0 { &1 } else { dmask };
        let site = (b, i);
        let guard = self.guard(b, i)?;
        let c = self.cur;
        let (kind, coords) = match dim {
            0 => (1, 1),
            1 => (2, 2),
            2 => (4, 3),
            // Sampled (face coordinates and face, see `cube_direction`) or
            // sized only.
            3 if name.starts_with("image_sample")
                || name.starts_with("image_gather")
                || name == "image_get_resinfo"
                || (name == "image_get_lod" && self.s.pixel) =>
            {
                (5, 3)
            }
            4 => (6, 2),
            5 => (7, 3),
            _ => return Err("an image dimension (cube loads or MSAA)"),
        };
        // Address VGPRs in order.
        let mut regs = Vec::new();
        for a in addr {
            if let Operand::Vector { reg, dwords } = *a {
                regs.extend((0..dwords).map(|k| reg + k));
            }
        }
        // Full 32-bit address VGPR at `reg_idx` (offset, bias, z-compare).
        let addr_u32 = |l: &mut Self, reg_idx: usize| -> Result<V, Why> {
            let r = *regs.get(reg_idx).ok_or("an image address")?;
            let v = inst.read(Reg::V(r)).ok_or("an image address")?;
            l.i32_of(v)
        };
        // With a16, two 16-bit components per VGPR starting at `base_reg`, low half first.
        let addr_packed = |l: &mut Self, base_reg: usize, k: usize| -> Result<V, Why> {
            let x = addr_u32(l, base_reg + if *a16 { k / 2 } else { k })?;
            if !*a16 {
                return Ok(x);
            }
            let c = l.cur;
            let (sh, mask) = (l.s.m.i32(16 * (k as u32 % 2)), l.s.m.i32(0xFFFF));
            let x = l.s.m.bin(c, Bin::LShr, x, sh);
            Ok(l.s.m.bin(c, Bin::And, x, mask))
        };
        let addr_val = |l: &mut Self, k: usize| -> Result<V, Why> { addr_packed(l, 0, k) };
        let src = self
            .descriptors
            .get(&(site, false))
            .copied()
            .ok_or("an untracked image")?;
        let store = name.starts_with("image_store");
        let uav = store
            || self
                .used_kind(src)
                .is_some_and(|u| u.kind == ResourceType::Uav);
        let props = (kind | if uav { 0x1000 } else { 0 }, 0x409);
        if let Some(op) = name.strip_prefix("image_atomic_") {
            // GLC returns the old value. DXIL implements subtraction by adding
            // the two's complement of the operand at the same width.
            let code = match op {
                "add" | "sub" => 0,
                "and" => 1,
                "or" => 2,
                "xor" => 3,
                "smin" => 4,
                "smax" => 5,
                "umin" => 6,
                "umax" => 7,
                "swap" => 8,
                "cmpswap" => u32::MAX,
                _ => return Err(name),
            };
            let Fields::Image { cache, .. } = &inst.fields else {
                unreachable!()
            };
            if *d16 || *a16 {
                return Err("a 16-bit image atomic");
            }
            // Compare-exchange consumes a replacement followed by a comparand.
            let compare = code == u32::MAX;
            let wide = match (compare, *dmask) {
                (false, 1) | (true, 3) => false,
                (false, 3) | (true, 15) => true,
                _ => return Err("an image atomic data mask"),
            };
            let iu = self.s.m.undef(self.s.t.i32);
            let mut cs = vec![iu; 3];
            for (k, slot) in cs.iter_mut().enumerate().take(coords) {
                *slot = addr_val(self, k)?;
            }
            let Operand::Vector { reg, .. } = *data else {
                return Err("image atomic data");
            };
            let width = if wide { 2 } else { 1 };
            let vals = self.vgprs(inst, data, if compare { 2 * width } else { width })?;
            let (ty, component) = if wide {
                self.s.flags |= flags::INT64 | flags::ATOMIC_TYPED64 | flags::ATOMIC_HEAP64;
                (self.s.m.int(64), 0x107) // one uint64_t in an R32G32_UINT view
            } else {
                (self.s.t.i32, 0x105)
            };
            let original = if wide {
                self.join64(vals[0], vals[1])
            } else {
                vals[0]
            };
            let value = if op == "sub" {
                let zero = self.s.m.int_const(ty, 0);
                self.s.m.bin(c, Bin::Sub, zero, original)
            } else {
                original
            };
            let h = self.handle(site, src, false, (kind | 0x1000, component))?;
            let mut args = vec![h];
            let (intrinsic, opcode) = if compare {
                let comparand = if wide {
                    self.join64(vals[2], vals[3])
                } else {
                    vals[1]
                };
                args.extend(cs);
                args.extend([comparand, value]);
                let intrinsic = if wide {
                    "atomicCompareExchange.i64"
                } else {
                    "atomicCompareExchange.i32"
                };
                (intrinsic, 79)
            } else {
                args.push(self.s.m.i32(code));
                args.extend(cs);
                args.push(value);
                (
                    if wide {
                        "atomicBinOp.i64"
                    } else {
                        "atomicBinOp.i32"
                    },
                    78,
                )
            };
            let at = self.begin_guarded(guard);
            let r = self
                .s
                .op(self.cur, intrinsic, Attr::NoUnwind, ty, opcode, &args);
            if cache.glc {
                let old = self.end_guarded(at, &[(r, original)])[0];
                if wide {
                    let (lo, hi) = self.split64(old);
                    self.write(inst, Reg::V(reg), lo, None);
                    self.write(inst, Reg::V(reg + 1), hi, None);
                } else {
                    self.write(inst, Reg::V(reg), old, None);
                }
            } else {
                self.end_guarded(at, &[]);
            }
            return Ok(());
        }
        if store {
            if name != "image_store" {
                return Err("an image store to a mip level");
            }
            // shortcut: stores are taken to write all four channels of a
            // float format (the console converts by the view's format).
            // DXIL's typed stores write all four: a store of some channels
            // (their data packed in channel order) keeps the others as the
            // texel holds them, read first (victory-pc-layer, after
            // Ortomanic's fix in fxceless/victory-fork). Unlike the
            // console's masked write, that races with other lanes' stores
            // to those channels of the same texel.
            if *dmask == 0 {
                return Err("an image store of no channels");
            }
            let iu = self.s.m.undef(self.s.t.i32);
            let mut cs = vec![iu; 3];
            for (k, slot) in cs.iter_mut().enumerate().take(coords) {
                *slot = addr_val(self, k)?;
            }
            let Operand::Vector { reg, .. } = *data else {
                return Err("image store data");
            };
            let mut vals = [None; 4];
            for (k, ch) in (0..4u8).filter(|ch| dmask >> ch & 1 != 0).enumerate() {
                let k = k as u8;
                // d16: two f16 channels per VGPR, low half first.
                let r = if *d16 { k / 2 } else { k };
                let v = inst.read(Reg::V(reg + r)).ok_or("image store data")?;
                vals[usize::from(ch)] = Some(if *d16 {
                    let mut x = self.i32_of(v)?;
                    if k % 2 == 1 {
                        let k16 = self.s.m.i32(16);
                        x = self.s.m.bin(self.cur, Bin::LShr, x, k16);
                    }
                    let f32t = self.s.t.f32;
                    self.s
                        .op(self.cur, "legacyF16ToF32", Attr::ReadNone, f32t, 131, &[x])
                } else {
                    self.float_of(v)?
                });
            }
            let h = self.handle(site, src, false, props)?;
            let at = self.begin_guarded(guard);
            let vals: Vec<V> = if let Some(vals) = vals.iter().copied().collect::<Option<Vec<_>>>() {
                vals
            } else {
                // Our declarations have four channels.
                self.s.flags |= flags::TYPED_UAV_LOADS;
                let mut args = vec![h, iu];
                args.extend(&cs);
                args.extend([iu, iu, iu]);
                let old = self.s.op(self.cur, "textureLoad.f32", Attr::ReadOnly, self.s.t.res_ret, 66, &args);
                (0..4u32)
                    .map(|ch| match vals[ch as usize] {
                        Some(x) => x,
                        None => self.s.m.extract(self.cur, old, ch),
                    })
                    .collect()
            };
            let mut args = vec![h];
            args.extend(cs);
            args.extend(vals);
            args.push(self.s.m.i8(0xF));
            let void = self.s.t.void;
            self.s.op(
                self.cur,
                "textureStore.f32",
                Attr::NoUnwind,
                void,
                67,
                &args,
            );
            self.end_guarded(at, &[]);
            return Ok(());
        }
        let has_cl = name.ends_with("_cl");
        let (op, sample) = match name {
            "image_sample" | "image_sample_o" | "image_sample_cl" => (60, true),
            "image_sample_lz" | "image_sample_l" | "image_sample_lz_o" => (62, true),
            "image_sample_b" | "image_sample_b_cl" => (61, true),
            "image_sample_c_lz" => (65, true),
            "image_sample_d" | "image_sample_d_cl" => (63, true),
            "image_get_lod" if self.s.pixel => (81, true),
            // Implicit derivatives: pixel shaders only.
            "image_sample_c" if self.s.pixel => (64, true),
            "image_sample_c_cl" if self.s.pixel => (64, true),
            "image_gather4" | "image_gather4_lz" | "image_gather4_lz_o" | "image_gather4_l" | "image_gather4_l_o" => (73, true),
            "image_load" | "image_load_mip" => (66, false),
            "image_get_resinfo" => (72, false),
            _ => return Err(name),
        };
        if op == 72 {
            // Width, height, depth or layers, levels (per dmask) of the level
            // in the first address: DXIL's dimensions; a channel DXIL leaves
            // out reads 1 (UAVs: one level).
            let h = self.handle(site, src, false, props)?;
            let mip = if uav {
                self.s.m.undef(self.s.t.i32)
            } else {
                addr_val(self, 0)?
            };
            let i32t = self.s.t.i32;
            let dims = self.s.m.named_struct("dx.types.Dimensions", vec![i32t; 4]);
            let d = self
                .s
                .op(c, "getDimensions", Attr::ReadOnly, dims, 72, &[h, mip]);
            let one = self.s.m.i32(1);
            let present = match kind {
                1 => [true, false, false, true],
                2 | 5 | 6 => [true, true, false, true],
                _ => [true, true, true, true],
            };
            let Operand::Vector { reg, .. } = *data else {
                return Err("an image result");
            };
            for (k, ch) in (0..4u32).filter(|ch| dmask >> ch & 1 != 0).enumerate() {
                let x = if ch == 3 && uav || !present[ch as usize] {
                    one
                } else {
                    self.s.m.extract(c, d, ch)
                };
                self.write(inst, Reg::V(reg + k as u8), x, guard);
            }
            return Ok(());
        }
        if uav && op != 66 {
            return Err("sampling a UAV");
        }
        let h = self.handle(site, src, false, props)?;
        let fu = self.s.m.undef(self.s.t.f32);
        let iu = self.s.m.undef(self.s.t.i32);
        // A gather at a named level, done as four texel reads (below).
        let mut gathered: Option<[V; 4]> = None;
        let r = if sample {
            let ss = sampler.ok_or("an untracked sampler")?;
            // Comparison: the reference value is the first address after
            // the bias, and the sampler is a comparison sampler.
            let cmp = matches!(op, 64 | 65);
            if cmp && (*a16 || dmask.count_ones() != 1) {
                return Err("a comparison sample of 16-bit coordinates or several channels");
            }
            let mut sampler_choice: Option<(V, Vec<u32>)> = None;
            let sh = match ss {
                Source::Phi { block, args } => {
                    match self.sampler_phi_choice(*block, args, cmp, 8) {
                        Ok((None, regs)) => self.s.sampler_handle(self.cur, regs[0]),
                        Ok((Some(idx), regs)) => {
                            let first = self.s.sampler_handle(self.cur, regs[0]);
                            sampler_choice = Some((idx, regs));
                            first
                        }
                        Err(_) => {
                            self.handle(site, ss, true, (14 | if cmp { 0x8000 } else { 0 }, 0))?
                        }
                    }
                }
                _ => self.handle(site, ss, true, (14 | if cmp { 0x8000 } else { 0 }, 0))?,
            };
            // Texel offsets (`_o`) come first: x in bits 0-5, y in 8-13,
            // signed (programmable offsets: gathers only).
            let offset_word = name.ends_with("_o");
            let first = usize::from(offset_word)
                + usize::from(name.starts_with("image_sample_b"))
                + usize::from(cmp);
            // Coordinates the size divides (not the array slice); explicit
            // gradients (x then y) come before the coordinates.
            let spatial = coords - usize::from(kind == 6 || kind == 7);
            // A cube's gradients are the two of its face coordinates.
            let grads = if kind == 5 { 2 } else { spatial };
            let deriv_regs = if op == 63 {
                if *a16 {
                    grads.div_ceil(2)
                } else {
                    grads
                }
            } else {
                0
            };
            let coord_base = first + 2 * deriv_regs;
            let mut cs = vec![fu; 4];
            // With a16, sampling coordinates, gradients, LOD and clamp are f16.
            let to_float = |l: &mut Self, x: V| {
                if *a16 {
                    let f = l.s.t.f32;
                    l.s.op(l.cur, "legacyF16ToF32", Attr::ReadNone, f, 131, &[x])
                } else {
                    l.as_float(x)
                }
            };
            if kind == 5 && *unorm {
                return Err("a cube sample with texel coordinates");
            }
            for (k, slot) in cs.iter_mut().enumerate().take(coords) {
                let x = addr_packed(self, coord_base, k)?;
                *slot = to_float(self, x);
            }
            if kind == 5 {
                let dir = self.cube_direction(cs[0], cs[1], cs[2]);
                cs[..3].copy_from_slice(&dir);
            }
            if *unorm {
                // Texel coordinates: divided by the size of level 0.
                let i32t = self.s.t.i32;
                let dims = self.s.m.named_struct("dx.types.Dimensions", vec![i32t; 4]);
                let z = self.s.m.i32(0);
                let d = self
                    .s
                    .op(c, "getDimensions", Attr::ReadOnly, dims, 72, &[h, z]);
                for (k, slot) in cs.iter_mut().enumerate().take(spatial) {
                    let n = self.s.m.extract(c, d, k as u32);
                    let n = self.s.m.cast(c, Cast::UiToFp, n, self.s.t.f32);
                    *slot = self.s.m.bin(c, Bin::SDiv, *slot, n);
                }
            }
            if op == 81 {
                // The clamped then the unclamped level of detail.
                let Operand::Vector { reg, .. } = *data else {
                    return Err("an image result");
                };
                let f32t = self.s.t.f32;
                for (k, ch) in (0..2u32).filter(|ch| dmask >> ch & 1 != 0).enumerate() {
                    let clamped = self.s.m.bool(ch == 0);
                    let lod = self.s.op(
                        c,
                        "calculateLOD.f32",
                        Attr::ReadOnly,
                        f32t,
                        81,
                        &[h, sh, cs[0], cs[1], cs[2], clamped],
                    );
                    let x = self.as_int(lod);
                    self.write(inst, Reg::V(reg + k as u8), x, guard);
                }
                return Ok(());
            }
            let offsets = if kind == 5 {
                0
            } else {
                (coords - usize::from(kind == 6 || kind == 7)).min(3)
            };
            let zero = self.s.m.i32(0);
            let mut os = vec![iu; 3];
            for o in os.iter_mut().take(offsets) {
                *o = zero;
            }
            if offset_word && offsets > 0 {
                let w = addr_val(self, 0)?;
                for (k, at) in [(0, 26), (1, 18)].into_iter().take(offsets.min(2)) {
                    // Bits at..at+5 sign-extended: shift up, then down.
                    let left = self.s.m.i32(at);
                    let t = self.s.m.bin(c, Bin::Shl, w, left);
                    let k26 = self.s.m.i32(26);
                    os[k] = self.s.m.bin(c, Bin::AShr, t, k26);
                }
            }
            let mut args = vec![h, sh];
            args.extend(&cs);
            let clamp = if has_cl {
                let x = addr_packed(self, coord_base, coords)?;
                to_float(self, x)
            } else {
                fu
            };
            let call_sample = |l: &mut Self,
                                   intrinsic: &str,
                                   opcode: u32,
                                   args: &[V],
                                   gathered: &mut Option<[V; 4]>|
             -> V {
                let Some((idx, ref regs)) = sampler_choice else {
                    return l
                        .s
                        .op(c, intrinsic, Attr::ReadOnly, l.s.t.res_ret, opcode, args);
                };
                let merge = l.s.m.new_block();
                let mut cur_test = c;
                let mut per_ch: [Vec<(V, usize)>; 4] = Default::default();
                let mut last_r = args[0];
                for (i, &reg) in regs.iter().enumerate() {
                    let arm = if i + 1 == regs.len() {
                        cur_test
                    } else {
                        let arm = l.s.m.new_block();
                        let next = l.s.m.new_block();
                        let k = l.s.m.i32(i as u32);
                        let cond = l.s.m.cmp(cur_test, pred::EQ, idx, k);
                        l.s.m.terminate(cur_test, Term::CondBr(cond, arm, next));
                        cur_test = next;
                        arm
                    };
                    let mut arm_args = args.to_vec();
                    arm_args[1] = l.s.sampler_handle(arm, reg);
                    let r_i = l.s.op(
                        arm,
                        intrinsic,
                        Attr::ReadOnly,
                        l.s.t.res_ret,
                        opcode,
                        &arm_args,
                    );
                    for (ch, slot) in per_ch.iter_mut().enumerate() {
                        let v = l.s.m.extract(arm, r_i, ch as u32);
                        slot.push((v, arm));
                    }
                    l.s.m.terminate(arm, Term::Br(merge));
                    last_r = r_i;
                }
                let mut phis = [fu; 4];
                for (ch, incoming) in per_ch.into_iter().enumerate() {
                    let phi = l.s.m.phi(merge, l.s.t.f32);
                    l.s.m.set_phi(phi, incoming);
                    phis[ch] = phi;
                }
                *gathered = Some(phis);
                l.cur = merge;
                last_r
            };
            match op {
                // Outside a pixel shader there are no screen derivatives:
                // the validator refuses `sample` there ("features
                // incompatible with the shader stage": 730 compute programs,
                // among them the G-buffer resolves and the sky), and what a
                // driver does with it is its own affair.
                // shortcut: such a sample reads level 0 (a bias: that
                // level). The console takes derivatives from neighbouring
                // lanes there; compute them from the lanes if a resolve
                // shows the wrong mip.
                60 if !self.s.pixel => {
                    args.extend(&os);
                    args.push(self.s.m.f32(0));
                    call_sample(self, "sampleLevel.f32", 62, &args, &mut gathered)
                }
                60 => {
                    if offset_word {
                        // DXIL wants immediate offsets in -8..=7 here; the
                        // console's are 6-bit and may be computed. A
                        // constant word in range becomes immediates
                        // (upstream PR #634); anything else keeps the
                        // offsets computed above (upstream refuses those).
                        let w = addr_val(self, 0)?;
                        if let Some(w) = self.s.m.int_value(w) {
                            let w = w as u32;
                            let field = |k: usize| {
                                let x = (w >> (8 * k) & 0x3F) as i32;
                                if x >= 32 { x - 64 } else { x }
                            };
                            let n = offsets.min(2);
                            if (0..n).all(|k| (-8..=7).contains(&field(k))) {
                                for (k, o) in os.iter_mut().enumerate().take(n) {
                                    *o = self.s.m.i32(field(k) as u32);
                                }
                            }
                        }
                    }
                    args.extend(&os);
                    args.push(clamp);
                    call_sample(self, "sample.f32", 60, &args, &mut gathered)
                }
                61 => {
                    // With a16 the bias is an f16 in the low half of its
                    // word (the word is whole, the value is not).
                    let bias = addr_u32(self, usize::from(offset_word))?;
                    let bias = if *a16 {
                        let low = self.s.m.i32(0xFFFF);
                        let bias = self.s.m.bin(c, Bin::And, bias, low);
                        to_float(self, bias)
                    } else {
                        self.as_float(bias)
                    };
                    args.extend(&os);
                    if self.s.pixel {
                        args.extend([bias, clamp]);
                        call_sample(self, "sampleBias.f32", 61, &args, &mut gathered)
                    } else {
                        // As above: the bias is the level.
                        args.push(bias);
                        call_sample(self, "sampleLevel.f32", 62, &args, &mut gathered)
                    }
                }
                63 if kind == 5 => {
                    // A cube with explicit gradients: the host wants them
                    // as changes of the direction, the console gives the
                    // changes of the face coordinates (the face is one
                    // unit wide). The level they select is taken from the
                    // longer of the two, in texels, and sampled directly.
                    // shortcut: no anisotropic filtering for these samples
                    // (the longer axis alone decides the level).
                    let mut len = [fu; 2];
                    for (axis, slot) in len.iter_mut().enumerate() {
                        let mut sum = self.s.m.f32(0);
                        for k in 0..2 {
                            let x = addr_packed(self, first + axis * deriv_regs, k)?;
                            let x = to_float(self, x);
                            let sq = self.s.m.bin(c, Bin::Mul, x, x);
                            sum = self.s.m.bin(c, Bin::Add, sum, sq);
                        }
                        *slot = self.unary(24, sum); // square root
                    }
                    let longer = self.binary(35, len[0], len[1]); // maximum
                    let i32t = self.s.t.i32;
                    let dims = self.s.m.named_struct("dx.types.Dimensions", vec![i32t; 4]);
                    let z = self.s.m.i32(0);
                    let d = self
                        .s
                        .op(c, "getDimensions", Attr::ReadOnly, dims, 72, &[h, z]);
                    let width = self.s.m.extract(c, d, 0);
                    let width = self.s.m.cast(c, Cast::UiToFp, width, self.s.t.f32);
                    let texels = self.s.m.bin(c, Bin::Mul, longer, width);
                    // log2 = natural logarithm / ln 2; a length of 0 gives
                    // minus infinity, which the sampler clamps to level 0.
                    let ln = self.unary(23, texels);
                    let per_ln2 = self.s.m.f32(std::f32::consts::LOG2_E.to_bits());
                    let lod = self.s.m.bin(c, Bin::Mul, ln, per_ln2);
                    args.extend(&os);
                    args.push(lod);
                    call_sample(self, "sampleLevel.f32", 62, &args, &mut gathered)
                }
                63 => {
                    let mut g = vec![fu; 6];
                    for k in 0..spatial {
                        let x = addr_packed(self, first, k)?;
                        g[k] = to_float(self, x);
                        let y = addr_packed(self, first + deriv_regs, k)?;
                        g[3 + k] = to_float(self, y);
                    }
                    args.extend(&os);
                    args.extend(g);
                    args.push(clamp);
                    call_sample(self, "sampleGrad.f32", 63, &args, &mut gathered)
                }
                64 | 65 => {
                    let reference = addr_u32(self, first - 1)?;
                    let reference = self.as_float(reference);
                    args.extend(&os);
                    args.push(reference);
                    let name = if op == 64 {
                        args.push(clamp);
                        "sampleCmp.f32"
                    } else {
                        "sampleCmpLevelZero.f32"
                    };
                    call_sample(self, name, op, &args, &mut gathered)
                }
                62 => {
                    let lod = if name == "image_sample_l" {
                        let x = addr_packed(self, coord_base, coords)?;
                        to_float(self, x)
                    } else {
                        self.s.m.f32(0)
                    };
                    args.extend(&os);
                    args.push(lod);
                    call_sample(self, "sampleLevel.f32", 62, &args, &mut gathered)
                }
                _ => {
                    if name == "image_gather4" {
                        return Err("a gather with implicit level");
                    }
                    if dmask.count_ones() != 1 {
                        return Err("a gather of several channels");
                    }
                    if matches!(name, "image_gather4_l" | "image_gather4_l_o")
                        && coords == 2
                        && spatial == 2
                    {
                        // The host's gather has no level: it read level 0
                        // where the title names one (the depth pyramid its
                        // occlusion tests read: most of the world was
                        // culled against the wrong depths). The four texels
                        // are read at the level instead, in the gather's
                        // order: (u0,v1) (u1,v1) (u1,v0) (u0,v0).
                        // With an offset too (`image_gather4_l_o`): the
                        // offset is in texels of that level.
                        // The title's meshlet culling takes a 4x4 block of
                        // the pyramid as one gather and three offset ones
                        // ((2,0), (0,2), (2,2)); the three read level 0,
                        // two texels beside the first one's place, so a
                        // meshlet three or four texels wide was tested
                        // against a quarter of what it covers.
                        // shortcut: the level is rounded to a whole one and
                        // coordinates past the edge are clamped, whatever
                        // the sampler says.
                        let lod = addr_packed(self, coord_base, coords)?;
                        let lod = to_float(self, lod);
                        let (i32t, f32t) = (self.s.t.i32, self.s.t.f32);
                        let zero = self.s.m.f32(0);
                        let lod = self.binary(35, lod, zero); // maximum
                        let lod = self.unary(26, lod); // round to nearest
                        let mip = self.s.m.cast(c, Cast::FpToUi, lod, i32t);
                        let dims = self.s.m.named_struct("dx.types.Dimensions", vec![i32t; 4]);
                        let d = self
                            .s
                            .op(c, "getDimensions", Attr::ReadOnly, dims, 72, &[h, mip]);
                        let half = self.s.m.f32(0.5f32.to_bits());
                        let one = self.s.m.f32(1.0f32.to_bits());
                        let (mut lo, mut hi) = ([iu; 2], [iu; 2]);
                        for k in 0..2 {
                            let n = self.s.m.extract(c, d, k as u32);
                            let n = self.s.m.cast(c, Cast::UiToFp, n, f32t);
                            let last = self.s.m.bin(c, Bin::Sub, n, one);
                            let t = self.s.m.bin(c, Bin::Mul, cs[k], n);
                            let t = self.s.m.bin(c, Bin::Sub, t, half);
                            let first = self.unary(27, t); // round down
                            // The offset word's field (0 without one).
                            let by = self.s.m.cast(c, Cast::SiToFp, os[k], f32t);
                            let first = self.s.m.bin(c, Bin::Add, first, by);
                            let next = self.s.m.bin(c, Bin::Add, first, one);
                            for (slot, x) in [(&mut lo[k], first), (&mut hi[k], next)] {
                                let x = self.binary(35, x, zero); // maximum
                                let x = self.binary(36, x, last); // minimum
                                *slot = self.s.m.cast(c, Cast::FpToSi, x, i32t);
                            }
                        }
                        let ch = dmask.trailing_zeros();
                        let mut g = [fu; 4];
                        let mut read = None;
                        let texels = [(lo[0], hi[1]), (hi[0], hi[1]), (hi[0], lo[1]), (lo[0], lo[1])];
                        for (slot, (x, y)) in g.iter_mut().zip(texels) {
                            let r = self.s.op(
                                c,
                                "textureLoad.f32",
                                Attr::ReadOnly,
                                self.s.t.res_ret,
                                66,
                                &[h, mip, x, y, iu, iu, iu, iu],
                            );
                            *slot = self.s.m.extract(c, r, ch);
                            read = Some(r);
                        }
                        gathered = Some(g);
                        read.expect("four reads")
                    } else {
                        args.extend(&os[..2]);
                        args.push(self.s.m.i32(dmask.trailing_zeros()));
                        call_sample(self, "textureGather.f32", 73, &args, &mut gathered)
                    }
                }
            }
        } else {
            let mut cs = vec![iu; 3];
            for (k, slot) in cs.iter_mut().enumerate().take(coords) {
                *slot = addr_val(self, k)?;
            }
            let mip = if name == "image_load_mip" {
                addr_val(self, coords)?
            } else if uav {
                // Our declarations have four channels.
                self.s.flags |= flags::TYPED_UAV_LOADS;
                iu
            } else {
                self.s.m.i32(0)
            };
            let mut args = vec![h, mip];
            args.extend(&cs);
            args.extend([iu, iu, iu]);
            self.s.op(
                c,
                "textureLoad.f32",
                Attr::ReadOnly,
                self.s.t.res_ret,
                66,
                &args,
            )
        };
        let Operand::Vector { reg, .. } = *data else {
            return Err("an image result");
        };
        let channels: Vec<u32> = if op == 73 {
            vec![0, 1, 2, 3]
        } else {
            (0..4).filter(|ch| dmask >> ch & 1 != 0).collect()
        };
        let mut xs = Vec::new();
        for ch in channels {
            let x = match gathered {
                Some(g) => g[ch as usize],
                None => self.s.m.extract(c, r, ch),
            };
            xs.push(if *d16 {
                // shortcut: d16 results are taken as floats (rounded to
                // nearest even); integer formats would keep 16 bits.
                let i32t = self.s.t.i32;
                self.s
                    .op(c, "legacyF32ToF16", Attr::ReadNone, i32t, 130, &[x])
            } else {
                self.as_int(x)
            });
        }
        if *d16 {
            // Two channels per VGPR, low half first (an odd last high half 0).
            let k16 = self.s.m.i32(16);
            xs = xs
                .chunks(2)
                .map(|p| match *p {
                    [lo, hi] => {
                        let hi = self.s.m.bin(c, Bin::Shl, hi, k16);
                        self.s.m.bin(c, Bin::Or, lo, hi)
                    }
                    _ => p[0],
                })
                .collect();
        }
        for (k, x) in xs.into_iter().enumerate() {
            self.write(inst, Reg::V(reg + k as u8), x, guard);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// The CPU form of `v_cvt_f32_f64`'s lowering agrees with Rust's own
    /// f64 to f32 conversion (ties to even) on edge values and on a few
    /// million others; NaNs stay NaNs of their sign.
    #[test]
    fn f64_to_f32_rounds_to_nearest_even() {
        let check = |x: u64| {
            let got = super::f32_bits_of_f64(x);
            let want = (f64::from_bits(x) as f32).to_bits();
            if f64::from_bits(x).is_nan() {
                assert!(f32::from_bits(got).is_nan(), "{x:#018x} -> {got:#010x}");
                assert_eq!(got >> 31, (x >> 63) as u32, "{x:#018x} sign");
                assert_eq!(got & 0x0040_0000, 0x0040_0000, "{x:#018x} quiet");
            } else {
                assert_eq!(got, want, "{x:#018x} ({})", f64::from_bits(x));
            }
        };
        let mut edges = vec![
            0,
            1,
            (1 << 52) - 1,
            1 << 52,
            0x7FF0_0000_0000_0000,
            0x7FF0_0000_0000_0001,
            0x7FF8_0000_0000_0000,
            0x7FFF_FFFF_FFFF_FFFF,
            f64::MAX.to_bits(),
            f64::from(f32::MAX).to_bits(),
            f64::from(f32::MIN_POSITIVE).to_bits(),
            f64::from(f32::from_bits(1)).to_bits(),
            f64::from(f32::from_bits(0x007F_FFFF)).to_bits(),
            1.0f64.to_bits(),
            0.1f64.to_bits(),
            (1.0f64 / 3.0).to_bits(),
        ];
        // Around every f32 exponent: exact values, halfway points (to even
        // either way), just above and below them, and the largest f32 with
        // what rounds past it to infinity.
        for e in 0..2047u64 {
            for m in [
                0u64,
                1,
                1 << 28,
                (1 << 28) - 1,
                (1 << 28) + 1,
                3 << 28,
                (1 << 29) - 1,
                (1 << 52) - 1,
                0xF_FFFF_E000_0000,
                0xF_FFFF_F000_0000,
            ] {
                edges.push(e << 52 | m);
            }
        }
        for x in edges {
            check(x);
            check(x | 1 << 63);
        }
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..4_000_000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            check(s);
            // The f32 range and below it, where results round or are denormal.
            let near = (s & 0x800F_FFFF_FFFF_FFFF) | ((s >> 20) % 330 + 700) << 52;
            check(near);
        }
    }
}
