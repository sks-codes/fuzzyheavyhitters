use crate::{
    aes::AES_KEY_SIZE,
    channel::CommTrackingChannel,
    data_structures::{
        mod2k::Mod2k,
        modp::{BarrettCtx, Modp},
    },
    fss::{
        distance::{DistanceFSSKey, BINOMIAL_COEFFICIENTS},
        interval::IntervalFSSKey,
    },
    fuzzy_match::{
        share_phase::SharePhaseError,
        share_types::{DistanceMetric, ShareMethod},
        shared_range::SharedRange,
        shared_sketch::SketchData,
        sketch_helper::SketchHelper,
        sketch_types::{SketchConfig, SketchValues},
    },
    randomness::prg::PRG,
};
use anyhow::{anyhow, ensure, Result};

pub struct Sketch {
    pub(super) config: SketchConfig,
    #[allow(dead_code)]
    barrett_ctx: BarrettCtx,
}

impl Sketch {
    pub fn new<C: Into<SketchConfig>>(config: C) -> Self {
        let config = config.into();
        let barrett_ctx = BarrettCtx::new(config.q);
        Self {
            config,
            barrett_ctx,
        }
    }

    #[allow(dead_code)]
    fn sketch_interval_fss(
        &self,
        _shared_ranges: &[SharedRange],
        _sketch_data: &[SketchData],
        _seed: [u8; AES_KEY_SIZE],
        _other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<bool> {
        Ok(true)
    }

    #[allow(dead_code)]
    fn sketch_distance_fss<const N: usize>(
        &self,
        _shared_ranges: &[SharedRange],
    ) -> Result<bool, SharePhaseError> {
        unimplemented!()
    }

    pub fn get_sketch_helper(&self) -> Result<SketchHelper> {
        match (&self.config.method, &self.config.metric) {
            (ShareMethod::FSS, DistanceMetric::LInfinity) => self.get_sketch_helper_linf(),
            (ShareMethod::FSS, DistanceMetric::Lp { p }) => match p {
                1 | 2 | 3 => self.get_sketch_helper_lp(*p),
                _ => Err(anyhow!(
                    "Currently only support sketching Lp with p = 1, 2, 3."
                )),
            },
            _ => Err(anyhow!("Unsupported sketch helper configuration")),
        }
    }

    pub fn sketch(&self) -> Result<Vec<Vec<Modp<'_>>>> {
        unimplemented!()
    }

    #[allow(dead_code)]
    fn sketch_interval_fss_one_dimension(
        &self,
        key: IntervalFSSKey<1>,
        _role: bool,
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<Vec<Modp<'_>>>> {
        // We do not parallelize at this level. We only parallelize through multiple key pairs
        let domain_size = 1usize << self.config.h1;
        let modulus = 1u128 << self.config.h2;
        let delta = self.config.delta;

        let ldcf_key = key.ldcf_key();
        let rdcf_key = key.rdcf_key();

        let ldcf_full_evals = ldcf_key.full_domain_incremental_eval(modulus, domain_size);
        let rdcf_full_evals = rdcf_key.full_domain_incremental_eval(modulus, domain_size);

        let ldcf_incremental_evals_mod2k: Vec<Vec<Mod2k>> = ldcf_full_evals
            .iter()
            .map(|evals| evals.iter().map(|x| Mod2k::new(x[0], modulus)).collect())
            .collect();
        let rdcf_incremental_evals_mod2k: Vec<Vec<Mod2k>> = rdcf_full_evals
            .iter()
            .map(|evals| evals.iter().map(|x| Mod2k::new(x[0], modulus)).collect())
            .collect();

        // Checking whether each level is ldcf
        let (sketch_helper_ldcf, sketch_helper_rdcf) = match sketch_helper {
            SketchHelper::IntervalFSS {
                sketch_helper_ldcf,
                sketch_helper_rdcf,
            } => (sketch_helper_ldcf, sketch_helper_rdcf),
            _ => {
                return Err(anyhow!("Sketch helper for interval fss type mismatch!"));
            }
        };
        let z_ldcf = self.sketch_incremental_dcf(
            &ldcf_incremental_evals_mod2k,
            self.config.h1,
            true,
            *sketch_helper_ldcf,
            prg,
        )?;
        let z_rdcf = self.sketch_incremental_dcf(
            &rdcf_incremental_evals_mod2k,
            self.config.h1,
            false,
            *sketch_helper_rdcf,
            prg,
        )?;

        // Checking whether last level of ldcf is shifted by 2*delta from last level of rdcf
        let z_shift_inc = self.sketch_shift_consistency(
            &ldcf_incremental_evals_mod2k[self.config.h1 - 1],
            &rdcf_incremental_evals_mod2k[self.config.h1 - 1],
            2 * delta as usize,
            domain_size,
            prg,
        )?;

        let ctx = &self.barrett_ctx;
        let flatten_pairs = |vals: &[(Modp, Modp)]| -> Vec<Modp> {
            vals.iter()
                .flat_map(|(a, b)| [Modp::new(ctx, a.value()), Modp::new(ctx, b.value())])
                .collect()
        };

        let ld_pairs = match z_ldcf {
            SketchValues::Dcf {
                last_layer,
                mut consistency,
            } => {
                let mut all = Vec::with_capacity(consistency.len() + 1);
                all.push(last_layer);
                all.append(&mut consistency);
                flatten_pairs(&all)
            }
            _ => return Err(anyhow!("Unexpected sketch helper result for LDCF")),
        };
        let rd_pairs = match z_rdcf {
            SketchValues::Dcf {
                last_layer,
                mut consistency,
            } => {
                let mut all = Vec::with_capacity(consistency.len() + 1);
                all.push(last_layer);
                all.append(&mut consistency);
                flatten_pairs(&all)
            }
            _ => return Err(anyhow!("Unexpected sketch helper result for RDCF")),
        };

        Ok(vec![ld_pairs, rd_pairs, vec![z_shift_inc]])
    }

    #[allow(dead_code)]
    fn sketch_distance_fss_lp_one_dimension(
        &self,
        key: DistanceFSSKey<2>,
        role: bool,
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<Modp<'_>>> {
        let _ = (key, role, sketch_helper, prg);
        Ok(Vec::new())
    }

    #[allow(dead_code)]
    fn shifted_dcf_to_dpf(
        &self,
        evals: &[Mod2k],
        domain_size: usize,
        shift: usize,
    ) -> Result<Vec<Mod2k>> {
        // The transformation from dcf to dpf, turns a vector of form: a, a, ..., a, b, ..., b
        // Into a vector of form: 0, 0, ..., 0, a-b, 0, ..., 0
        // Where the index of the entry being a-b is equal to the index of the last entry being a in the original vector
        // Push 0 in the end to keep the same length as the original vector
        ensure!(
            evals.len() == domain_size,
            "length mismatch between dcf sketch vector and domain_size"
        );
        let mut evals_dpf: Vec<Mod2k> = (0..(domain_size - 1))
            .map(|i| evals[i] - evals[i + 1])
            .collect();

        let modulus = 1u128 << self.config.h2;
        evals_dpf.push(Mod2k::zero(modulus));
        self.shifted_dpf_to_dpf(&evals_dpf, domain_size, shift)
    }

    #[allow(dead_code)]
    fn shifted_dpf_to_dpf(
        &self,
        evals: &[Mod2k],
        domain_size: usize,
        shift: usize,
    ) -> Result<Vec<Mod2k>> {
        ensure!(
            evals.len() == domain_size,
            "length mismatch between dpf sketch vector and domain_size"
        );
        let modulus = 1u128 << self.config.h2;
        let shift = shift.min(domain_size);
        let mut shifted = vec![Mod2k::zero(modulus); domain_size];
        if shift < domain_size {
            shifted[shift..].clone_from_slice(&evals[..domain_size - shift]);
        }
        Ok(shifted)
    }

    #[allow(dead_code)]
    fn dpf_to_unit_vector<'a>(
        &self,
        evals: &[Mod2k],
        domain_size: usize,
        scale0: &[Modp<'a>],
        scale1: &[Modp<'a>],
        barrett_ctx: &'a BarrettCtx,
    ) -> Result<(Vec<Modp<'a>>, Vec<Modp<'a>>)> {
        // From a dpf in Z2k, there would always be two cases of payload in the non-zero entry
        // Either the entries differ by payload, or the entries differ by payload - 2^k.
        ensure!(
            scale0.len() == domain_size && scale1.len() == domain_size,
            "helper vector length mismatch"
        );
        let evals_modp: Vec<Modp<'a>> = evals
            .iter()
            .map(|x| Modp::new(barrett_ctx, x.val()))
            .collect();

        Ok((
            element_wise_product_modp(&evals_modp, scale0),
            element_wise_product_modp(&evals_modp, scale1),
        ))
    }

    #[allow(dead_code)]
    fn sketch_incremental_dcf<'a>(
        &'a self,
        incremental_evals: &[Vec<Mod2k>],
        height: usize,
        ldcf_or_rdcf: bool,
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Result<SketchValues<'a>> {
        ensure!(
            incremental_evals.len() == height,
            "Height mismatch: expected {}, got {}",
            height,
            incremental_evals.len()
        );
        ensure!(height > 0, "Tree height must be positive.");

        let barrett_ctx = &self.barrett_ctx;
        let incremental_evals_dpf =
            self.incremental_dcf_to_incremental_dpf(incremental_evals, height)?;
        let domain_size = incremental_evals_dpf
            .last()
            .map(|v| v.len())
            .unwrap_or_default();

        // Only sketch DCF property at the last layer.
        let last_layer_sketch = {
            let (scale0, scale1) = sketch_helper.get_helper_vector(barrett_ctx, domain_size);
            let (evals_unit_vector0, evals_unit_vector1) = self.dpf_to_unit_vector(
                &incremental_evals_dpf[height - 1],
                domain_size,
                &scale0,
                &scale1,
                barrett_ctx,
            )?;

            let z0 = dot_product_modp(barrett_ctx, &evals_unit_vector0, &evals_unit_vector0);
            let z1 = dot_product_modp(barrett_ctx, &evals_unit_vector1, &evals_unit_vector1);
            (z0, z1)
        };

        // Sketch consistency between earlier levels
        let mut consistency_sketches = Vec::new();
        for level in 0..height.saturating_sub(1) {
            let evals_i_duplicated = duplicate_vector_mod2k(&incremental_evals_dpf[level]);
            let next = &incremental_evals_dpf[level + 1];
            let (evals_subtracted_case0, evals_subtracted_case1) = if ldcf_or_rdcf {
                // LDCF
                (
                    self.subtract_shifted_mod2k_to_modp(&evals_i_duplicated, next, 0, barrett_ctx), // Recheck later
                    self.subtract_shifted_mod2k_to_modp(&evals_i_duplicated, next, 1, barrett_ctx), // Recheck later
                )
            } else {
                // RDCF
                (
                    self.subtract_shifted_mod2k_to_modp(next, &evals_i_duplicated, 0, barrett_ctx), // Recheck later
                    self.subtract_shifted_mod2k_to_modp(next, &evals_i_duplicated, 1, barrett_ctx), // Recheck later
                )
            };

            let domain_size = evals_subtracted_case0.len();
            let rs = sample_modp_vec(domain_size, barrett_ctx, prg);

            let z_ast = dot_product_modp(barrett_ctx, &rs, &evals_subtracted_case0);
            let z_bullet = dot_product_modp(barrett_ctx, &rs, &evals_subtracted_case1);
            consistency_sketches.push((z_ast, z_bullet));
        }

        Ok(SketchValues::Dcf {
            last_layer: last_layer_sketch,
            consistency: consistency_sketches,
        })
    }

    #[allow(dead_code)]
    fn sketch_incremental_dcf_shift_payload<'a>(
        &'a self,
        incremental_evals: &[Vec<Vec<Mod2k>>],
        length: usize,
        height: usize,
        shift: isize,
        ldcf_or_rdcf: bool,
        sketch_helper: SketchHelper,
        payload_helper: &[Vec<Mod2k>],
        prg: &mut PRG,
    ) -> Result<SketchValues<'a>> {
        ensure!(
            incremental_evals.len() == height,
            "Height mismatch: expected {}, got {}",
            height,
            incremental_evals.len()
        );
        ensure!(height > 0, "Tree height must be positive.");

        let barrett_ctx = &self.barrett_ctx;
        let modulus = 1u128 << self.config.h2;
        let incremental_evals_dpf = self.incremental_dcf_payload_to_incremmental_dpf_payload(
            incremental_evals,
            length,
            height,
        )?;

        let last_layer = incremental_evals_dpf
            .get(height - 1)
            .ok_or_else(|| anyhow!("missing last layer for payload sketch"))?;
        let domain_size = last_layer.len();

        ensure!(
            payload_helper.len() >= length.saturating_sub(1),
            "payload helper length mismatch"
        );
        ensure!(
            last_layer.iter().all(|row| row.len() >= length),
            "payload length mismatch in last layer"
        );
        for helper in payload_helper.iter().take(length.saturating_sub(1)) {
            ensure!(
                helper.len() == domain_size,
                "payload helper vector length mismatch"
            );
        }

        // Shift and rescale last layer payloads
        let last_layer_shifted = shift_mod2k_vec_payload(last_layer, length, shift, modulus);
        let last_layer_payloads_subtracted: Vec<Vec<Modp>> = (0..length.saturating_sub(1))
            .map(|i| {
                let rescaled: Vec<Mod2k> = last_layer_shifted
                    .iter()
                    .zip(payload_helper[i].iter())
                    .map(|(components, helper)| components[i] * *helper)
                    .collect();
                let compare: Vec<Mod2k> = last_layer_shifted
                    .iter()
                    .map(|components| components[i])
                    .collect();
                self.subtract_shifted_mod2k_to_modp(&rescaled, &compare, 0, barrett_ctx)
            })
            .collect();

        // Sketch last layer payload consistency (power sums)
        let rs = sample_modp_vec(domain_size, barrett_ctx, prg);
        let mut rs_pow = vec![Modp::one(barrett_ctx); domain_size];
        let mut last_layer_consistency_sketch = Vec::with_capacity(length.saturating_sub(1));
        for subtracted in &last_layer_payloads_subtracted {
            rs_pow = element_wise_product_modp(&rs, &rs_pow);
            last_layer_consistency_sketch.push(dot_product_modp(barrett_ctx, &rs_pow, subtracted));
        }

        // Sketch to check whether the last layer is DCF (use first component as base)
        if !matches!(sketch_helper, SketchHelper::Dcf { .. }) {
            return Err(anyhow!("Sketch helper for payload sketch must be Dcf"));
        }
        let base_last_layer: Vec<Mod2k> = last_layer
            .iter()
            .map(|row| *row.get(0).unwrap_or(&Mod2k::zero(modulus)))
            .collect();
        let (scale0, scale1) = sketch_helper.get_helper_vector(barrett_ctx, domain_size);
        let (evals_unit_vector0, evals_unit_vector1) =
            self.dpf_to_unit_vector(&base_last_layer, domain_size, &scale0, &scale1, barrett_ctx)?;

        let rs_last = sample_modp_vec(domain_size, barrett_ctx, prg);
        let last_layer_sketch = vec![(
            dot_product_modp(barrett_ctx, &rs_last, &evals_unit_vector0),
            dot_product_modp(barrett_ctx, &rs_last, &evals_unit_vector1),
        )];

        // Sketch consistency between levels for each payload component
        let mut consistency_sketches: Vec<Vec<(Modp, Modp)>> = Vec::new();
        for level in 0..height.saturating_sub(1) {
            let level_evals = &incremental_evals_dpf[level];
            let next_evals = &incremental_evals_dpf[level + 1];
            let level_domain_size = level_evals.len();
            let rs_level = sample_modp_vec(level_domain_size, barrett_ctx, prg);

            let mut this_level = Vec::with_capacity(length);
            for component_idx in 0..length {
                let evals_i: Vec<Mod2k> = level_evals
                    .iter()
                    .map(|row| *row.get(component_idx).unwrap_or(&Mod2k::zero(modulus)))
                    .collect();
                let evals_next: Vec<Mod2k> = next_evals
                    .iter()
                    .map(|row| *row.get(component_idx).unwrap_or(&Mod2k::zero(modulus)))
                    .collect();
                let evals_i_duplicated = duplicate_vector_mod2k(&evals_i);

                let (evals_subtracted_case0, evals_subtracted_case1) = if ldcf_or_rdcf {
                    // LDCF
                    (
                        self.subtract_shifted_mod2k_to_modp(
                            &evals_next,
                            &evals_i_duplicated,
                            0,
                            barrett_ctx,
                        ),
                        self.subtract_shifted_mod2k_to_modp(
                            &evals_next,
                            &evals_i_duplicated,
                            1,
                            barrett_ctx,
                        ),
                    )
                } else {
                    // RDCF
                    (
                        self.subtract_shifted_mod2k_to_modp(
                            &evals_i_duplicated,
                            &evals_next,
                            0,
                            barrett_ctx,
                        ),
                        self.subtract_shifted_mod2k_to_modp(
                            &evals_i_duplicated,
                            &evals_next,
                            1,
                            barrett_ctx,
                        ),
                    )
                };

                let z_ast = dot_product_modp(barrett_ctx, &rs_level, &evals_subtracted_case0);
                let z_bullet = dot_product_modp(barrett_ctx, &rs_level, &evals_subtracted_case1);
                this_level.push((z_ast, z_bullet));
            }
            consistency_sketches.push(this_level);
        }

        Ok(SketchValues::DcfPayload {
            length,
            last_layer: last_layer_sketch,
            last_layer_consistency: last_layer_consistency_sketch,
            consistency: consistency_sketches,
        })
    }

    #[allow(dead_code)]
    fn sketch_shift_consistency(
        &self,
        a: &[Mod2k],
        b: &[Mod2k],
        shift: usize,
        domain_size: usize,
        prg: &mut PRG,
    ) -> Result<Modp<'_>> {
        ensure!(
            a.len() == domain_size && b.len() == domain_size,
            "length mismatch for shift consistency sketch"
        );
        let barrett_ctx = &self.barrett_ctx;
        let subtracted = self.subtract_shifted_mod2k_to_modp(a, b, shift, barrett_ctx);

        let mut rs_u128: Vec<u128> = vec![0u128; domain_size];
        prg.random_u128s(&mut rs_u128);
        let rs: Vec<Modp> = rs_u128.iter().map(|x| Modp::new(barrett_ctx, *x)).collect();

        Ok(dot_product_modp(barrett_ctx, &rs, &subtracted))
    }

    #[allow(dead_code)]
    fn subtract_shifted_mod2k_to_modp<'a>(
        &self,
        a: &[Mod2k],
        b: &[Mod2k],
        shift: usize,
        ctx: &'a BarrettCtx,
    ) -> Vec<Modp<'a>> {
        // Only shift left!!!
        let modulus = 1u128 << self.config.h2;
        let shifted_a: Vec<Mod2k> = if shift > 0 {
            // Shift to the left
            [
                a[..a.len() - shift].to_vec(),
                vec![Mod2k::zero(modulus); shift],
            ]
            .concat()
        } else {
            // No shift
            a.to_vec()
        };

        let subtracted = element_wise_subtract_mod2k(&shifted_a, b);

        subtracted.iter().map(|x| Modp::new(ctx, x.val())).collect()
    }
}

impl Sketch {
    fn incremental_dcf_to_incremental_dpf(
        &self,
        incremental_evals: &[Vec<Mod2k>],
        height: usize,
    ) -> Result<Vec<Vec<Mod2k>>> {
        // Transform every single level into dpf in Z2k first
        // Assume originally, each level contains vectors of form a, a, ..., a, b, b, ..., b
        // The non-zero index in each level is exactly the last a-index in each original level
        let mut incremental_evals_dpf: Vec<Vec<Mod2k>> = Vec::with_capacity(height);
        let modulus = 1u128 << self.config.h2;
        for level in 0..height {
            let evals = &incremental_evals[level];
            let domain_size = 1usize << level;
            ensure!(
                evals.len() == domain_size,
                "length mismatch between dcf sketch vector and domain_size"
            );
            let mut evals_dpf: Vec<Mod2k> = (0..domain_size.saturating_sub(1))
                .map(|i| evals[i] - evals[i + 1])
                .collect();
            evals_dpf.push(Mod2k::zero(modulus));
            incremental_evals_dpf.push(evals_dpf);
        }

        Ok(incremental_evals_dpf)
    }

    fn incremental_dcf_payload_to_incremmental_dpf_payload(
        &self,
        incremental_evals: &[Vec<Vec<Mod2k>>],
        length: usize,
        height: usize,
    ) -> Result<Vec<Vec<Vec<Mod2k>>>> {
        let mut incremental_evals_dpf: Vec<Vec<Vec<Mod2k>>> = Vec::with_capacity(height);
        let modulus = 1u128 << self.config.h2;
        for level in 0..height {
            let evals = &incremental_evals[level];
            let domain_size = 1usize << level;
            let mut evals_dpf: Vec<Vec<Mod2k>> = (0..domain_size.saturating_sub(1))
                .map(|i| (0..length).map(|j| evals[i][j] - evals[i + 1][j]).collect())
                .collect();
            evals_dpf.push(vec![Mod2k::zero(modulus); length]);
            incremental_evals_dpf.push(evals_dpf);
        }

        Ok(incremental_evals_dpf)
    }
}

fn sample_modp_vec<'a>(
    domain_size: usize,
    barrett_ctx: &'a BarrettCtx,
    prg: &mut PRG,
) -> Vec<Modp<'a>> {
    let mut rs_u128: Vec<u128> = vec![0u128; domain_size];
    prg.random_u128s(&mut rs_u128);
    rs_u128
        .into_iter()
        .map(|x| Modp::new(barrett_ctx, x))
        .collect()
}

fn shift_mod2k_vec(input: &[Mod2k], shift: isize, modulus: u128) -> Vec<Mod2k> {
    // CAUTION: ONLY WORKS FOR DPF!!!
    let len = input.len();
    let mut out = Vec::with_capacity(len);
    if shift > 0 {
        let s = (shift as usize).min(len);
        if s < len {
            out.extend_from_slice(&input[s..]);
        }
        out.extend(std::iter::repeat(Mod2k::zero(modulus)).take(s));
    } else if shift < 0 {
        let s = (-shift as usize).min(len);
        out.extend(std::iter::repeat(Mod2k::zero(modulus)).take(s));
        if s < len {
            out.extend_from_slice(&input[..len - s]);
        }
    } else {
        out.extend_from_slice(input);
    }
    if out.len() > len {
        out.truncate(len);
    } else if out.len() < len {
        out.extend(std::iter::repeat(Mod2k::zero(modulus)).take(len - out.len()));
    }
    out
}

fn shift_mod2k_vec_payload(
    input: &[Vec<Mod2k>],
    length: usize,
    shift: isize,
    modulus: u128,
) -> Vec<Vec<Mod2k>> {
    // CAUTION: ONLY WORKS FOR DPF!!!
    let len = input.len();
    let mut out: Vec<Vec<Mod2k>> = Vec::with_capacity(len);
    if shift > 0 {
        let s = (shift as usize).min(len);
        if s < len {
            out.extend_from_slice(&input[s..]);
        }
        out.extend(std::iter::repeat(vec![Mod2k::zero(modulus); length]).take(s));
    } else if shift < 0 {
        let s = (-shift as usize).min(len);
        out.extend(std::iter::repeat(vec![Mod2k::zero(modulus); length]).take(s));
        if s < len {
            out.extend_from_slice(&input[..len - s]);
        }
    } else {
        out.extend_from_slice(input);
    }
    if out.len() > len {
        out.truncate(len);
    } else if out.len() < len {
        out.extend(std::iter::repeat(vec![Mod2k::zero(modulus); length]).take(len - out.len()));
    }
    out
}

// This is the bulk of implmentations for get_sketch_helper
impl Sketch {
    fn get_sketch_helper_linf(&self) -> Result<SketchHelper> {
        let barrett_ctx = BarrettCtx::new(self.config.q);
        let modulus = 1u128 << self.config.h2;
        let case_2_const_inv = Modp::one(&barrett_ctx) - Modp::new(&barrett_ctx, modulus);
        let case_2_const = case_2_const_inv.inv().unwrap();
        Ok(SketchHelper::IntervalFSS {
            sketch_helper_ldcf: Box::new(SketchHelper::Dcf {
                barrett_ctx: barrett_ctx,
                inv_value: case_2_const.value(),
            }),
            sketch_helper_rdcf: Box::new(SketchHelper::Dcf {
                barrett_ctx: barrett_ctx,
                inv_value: case_2_const.value(),
            }),
        })
    }

    fn get_sketch_helper_lp(&self, p: u32) -> Result<SketchHelper> {
        let modulus = 1u128 << self.config.h2;
        let domain_size = 1usize << self.config.h1;
        let _delta = self.config.delta;
        let barrett_ctx = BarrettCtx::new(self.config.q);
        let modulus_modp = Modp::new(&barrett_ctx, modulus);
        let p_usize = p as usize;

        // zero_payload
        let zero_payload: Vec<Vec<Mod2k>> = (0..=p_usize)
            .map(|_| (0..domain_size).map(|_| Mod2k::zero(modulus)).collect())
            .collect();

        // Base vectors for degree-1 (p = 1) polynomial payloads.
        let mut pow_vec: Vec<Vec<Mod2k>> = (0..=p_usize)
            .map(|i| {
                (0..domain_size)
                    .map(|x| {
                        Mod2k::new(x as u128, modulus).pow(i as u128)
                            * BINOMIAL_COEFFICIENTS[p_usize][i]
                    })
                    .collect()
            })
            .collect();
        for i in 0..=p_usize {
            if (i & 1) == 1 {
                pow_vec[i] = pow_vec[i]
                    .iter()
                    .map(|x| Mod2k::zero(modulus) - *x)
                    .collect();
            }
        }

        // right_payload corresponds to (1, -x).
        let right_payload = pow_vec.clone();

        for i in 0..=p_usize {
            if ((i & 1) == 1) ^ (((p_usize + 1 - i) & 1) == 1) {
                pow_vec[i] = pow_vec[i]
                    .iter()
                    .map(|x| Mod2k::zero(modulus) - *x)
                    .collect();
            }
        }

        // left_payload is the negation of right_payload.
        let left_payload = pow_vec.clone();

        // out_payload carries the threshold term on the last coordinate.
        let mut out_payload = zero_payload.clone();
        out_payload[p_usize] = (0..domain_size)
            .map(|_| Mod2k::new(self.config.delta, modulus).pow(p as u128) + 1u128)
            .collect();

        // Helper vectors for ldcf0: (out_payload - left_payload) - zero_payload.
        let ldcf_0_helper = element_wise_subtract_mod2k_vec(
            &element_wise_subtract_mod2k_vec(&out_payload, &left_payload),
            &zero_payload,
        );

        // Helper vectors for ldcf1: left_payload - zero_payload.
        let ldcf_1_helper = element_wise_subtract_mod2k_vec(&left_payload, &zero_payload);

        // Helper vectors for rdcf0: zero_payload - right_payload.
        let rdcf_0_helper = element_wise_subtract_mod2k_vec(&zero_payload, &right_payload);

        // Helper vectors for rdcf1: zero_payload - (out_payload - right_payload).
        let rdcf_1_helper = element_wise_subtract_mod2k_vec(
            &zero_payload,
            &element_wise_subtract_mod2k_vec(&out_payload, &right_payload),
        );

        let to_case_helpers = |v: &[Vec<Mod2k>]| -> (Vec<Vec<u128>>, Vec<Vec<u128>>) {
            let mut case1_full = Vec::with_capacity(v.len());
            let mut case2_full = Vec::with_capacity(v.len());
            for component in v {
                let mut case1 = Vec::with_capacity(component.len());
                let mut case2 = Vec::with_capacity(component.len());
                for val in component {
                    let as_modp = Modp::new(&barrett_ctx, val.val());
                    let inv_case1 = as_modp
                        .inv()
                        .expect("helper value should be invertible in Modp");
                    let inv_case2 = (as_modp - modulus_modp)
                        .inv()
                        .expect("shifted helper value should be invertible in Modp");
                    case1.push(inv_case1.value());
                    case2.push(inv_case2.value());
                }
                case1_full.push(case1);
                case2_full.push(case2);
            }
            (case1_full, case2_full)
        };

        let (ldcf_0_case1, ldcf_0_case2) = to_case_helpers(&ldcf_0_helper);
        let (ldcf_1_case1, ldcf_1_case2) = to_case_helpers(&ldcf_1_helper);
        let (rdcf_0_case1, rdcf_0_case2) = to_case_helpers(&rdcf_0_helper);
        let (rdcf_1_case1, rdcf_1_case2) = to_case_helpers(&rdcf_1_helper);

        Ok(SketchHelper::DistanceFSSPayload {
            sketch_helper_ldcf0: Box::new(SketchHelper::DcfPayload {
                barrett_ctx,
                cases0_full: ldcf_0_case1,
                cases1_full: ldcf_0_case2,
            }),
            sketch_helper_ldcf1: Box::new(SketchHelper::DcfPayload {
                barrett_ctx,
                cases0_full: ldcf_1_case1,
                cases1_full: ldcf_1_case2,
            }),
            sketch_helper_rdcf0: Box::new(SketchHelper::DcfPayload {
                barrett_ctx,
                cases0_full: rdcf_0_case1,
                cases1_full: rdcf_0_case2,
            }),
            sketch_helper_rdcf1: Box::new(SketchHelper::DcfPayload {
                barrett_ctx,
                cases0_full: rdcf_1_case1,
                cases1_full: rdcf_1_case2,
            }),
        })
    }
}

#[allow(dead_code)]
fn dot_product_modp<'a>(ctx: &'a BarrettCtx, a: &[Modp<'a>], b: &[Modp<'a>]) -> Modp<'a> {
    assert!(a.len() == b.len());
    a.iter()
        .zip(b.iter())
        .fold(Modp::zero(ctx), |acc, (x, y)| acc + (*x * *y))
}

#[allow(dead_code)]
fn element_wise_product_modp<'a>(a: &[Modp<'a>], b: &[Modp<'a>]) -> Vec<Modp<'a>> {
    assert!(a.len() == b.len());
    a.iter().zip(b.iter()).map(|(x, y)| *x * *y).collect()
}

#[allow(dead_code)]
fn element_wise_subtract_modp<'a>(a: &[Modp<'a>], b: &[Modp<'a>]) -> Vec<Modp<'a>> {
    assert!(a.len() == b.len());
    a.iter().zip(b.iter()).map(|(x, y)| *x - *y).collect()
}

fn element_wise_subtract_mod2k(a: &[Mod2k], b: &[Mod2k]) -> Vec<Mod2k> {
    assert!(a.len() == b.len());
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.clone() - y.clone())
        .collect()
}

fn element_wise_subtract_mod2k_vec(a: &[Vec<Mod2k>], b: &[Vec<Mod2k>]) -> Vec<Vec<Mod2k>> {
    assert!(a.len() == b.len());
    a.iter()
        .zip(b.iter())
        .map(|(row_a, row_b)| element_wise_subtract_mod2k(row_a, row_b))
        .collect()
}

#[allow(dead_code)]
fn duplicate_vector_mod2k(a: &[Mod2k]) -> Vec<Mod2k> {
    a.iter().flat_map(|x| [x.clone(), x.clone()]).collect()
}
