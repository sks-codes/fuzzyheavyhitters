use crate::{
    channel::CommTrackingChannel,
    data_structures::{
        mod2k::Mod2k,
        modp::{BarrettCtx, Modp},
        ringvec::RingVec,
    },
    fss::{
        distance::{BINOMIAL_COEFFICIENTS, DistanceFSSKey},
        interval::IntervalFSSKey,
    },
    fuzzy_match::{
        share_phase::SharePhaseError, 
        share_types::{DistanceMetric, ShareMethod}, 
        shared_range::SharedRange, 
        shared_sketch::{SketchData, TripleModp}, 
        sketch_helper::SketchHelper, 
        sketch_types::{SketchConfig, SketchValues},
    },
    randomness::prg::PRG,
};
use anyhow::{anyhow, ensure, Result};
use scuttlebutt::AbstractChannel;

pub struct SketchPhase {
    config: SketchConfig,
    #[allow(dead_code)]
    barrett_ctx: BarrettCtx,
}

impl SketchPhase {
    pub fn new<C: Into<SketchConfig>>(config: C) -> Self {
        let config = config.into();
        let barrett_ctx = BarrettCtx::new(config.q);
        Self {
            config,
            barrett_ctx,
        }
    }

    pub fn sketch<'a>(&'a self, shared_range: &SharedRange, sketch_helper: &SketchHelper, prg: &mut PRG) -> Result<Vec<SketchValues<'a>>> {
        match &self.config.method {
            ShareMethod::FSS => {
                match &self.config.metric {
                    DistanceMetric::LInfinity => self.sketch_linf(shared_range, sketch_helper, prg),
                    DistanceMetric::Lp { p } => self.sketch_lp(*p as usize, shared_range, sketch_helper, prg),
                }
            },
            _ => Err(anyhow!("Sketch not supported for this method. Only support ShareMethod::FSS")),
        }
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

    pub fn get_sketch_data<'a>(&self, prg: &mut PRG) -> Result<SketchData<'a>> {
        match &self.config.method {
            ShareMethod::FSS => {
                match &self.config.metric {
                    DistanceMetric::LInfinity => self.get_sketch_data_linf(prg),
                    DistanceMetric::Lp { p } => self.get_sketch_data_lp(prg),
                }
            }
            _ => Err(anyhow!("get_sketch_data not supported for this method. Only support ShareMethod::FSS")), 
        }
    }

    /// Verify the sketch using pre-shared Beaver triples over an MPC channel.
    /// Returns a collection of zero-tests (shares) that should all open to 0 when combined.
    pub fn verify<'a>(
        &'a self,
        sketch_value: &SketchValues<'a>,
        sketch_data: &SketchData<'a>,
        channel: &mut CommTrackingChannel,
        is_first: bool,
    ) -> Result<Vec<Modp<'a>>> {
        match (&self.config.method, &self.config.metric) {
            (ShareMethod::FSS, DistanceMetric::LInfinity) => {
                self.verify_linf(sketch_value, sketch_data, channel, is_first)
            }
            (ShareMethod::FSS, DistanceMetric::Lp { .. }) => {
                self.verify_lp(sketch_value, sketch_data, channel, is_first)
            }
            _ => Err(anyhow!("Sketch verification not supported for this configuration")),
        }
    }
}

// Helpers for the sketching phase
impl SketchPhase {
    fn sketch_linf<'a>(
        &'a self,
        shared_range: &SharedRange,
        sketch_helper: &SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<SketchValues<'a>>> {
        match shared_range {
            SharedRange::IntervalFSS { keys, role: _ } => {
                let mut sketch_values = Vec::new();
                for key in keys {
                    let sketch_value = self.sketch_linf_one_dimension(key, sketch_helper, prg)
                        .map_err(|e| anyhow!("Error when sketch linf one dimension: {}", e))?;
                    sketch_values.push(sketch_value);
                }
                Ok(sketch_values)
            }
            _ => Err(anyhow!("Wrong shared_range type, needed SharedRange::IntervalFSS")),
        }
    }

    fn sketch_lp<'a>(
        &'a self,
        p: usize,
        shared_range: &SharedRange,
        sketch_helper: &SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<SketchValues<'a>>> {
        match shared_range {
            SharedRange::DistanceFSS { keys, role: _ } => {
                let mut sketch_values = Vec::new();
                for key in keys {
                    let sketch_value = self.sketch_lp_one_dimension(key, p, sketch_helper, prg)
                        .map_err(|e| anyhow!("Error when sketch lp one dimension: {}", e))?;
                    sketch_values.push(sketch_value);
                }
                Ok(sketch_values)
            }
            _ => Err(anyhow!("Wrong shared_range type for lp sketch")),
        }
    }

    #[allow(dead_code)]
    fn sketch_linf_one_dimension<'a>(
        &'a self,
        key: &IntervalFSSKey,
        sketch_helper: &SketchHelper,
        prg: &mut PRG,
    ) -> Result<SketchValues<'a>> {
        // We do not parallelize at this level. We only parallelize through multiple key pairs
        let domain_size = 1usize << self.config.h1;
        let modulus = 1u128 << self.config.h2;
        let delta = self.config.delta;

        let ldcf_key = key.ldcf_key();
        let rdcf_key = key.rdcf_key();

        let ldcf_full_evals = ldcf_key
            .full_domain_incremental_eval(modulus, self.config.h1)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        let rdcf_full_evals = rdcf_key
            .full_domain_incremental_eval(modulus, self.config.h1)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        let ldcf_incremental_evals_mod2k: Vec<Vec<Mod2k>> = ldcf_full_evals
            .iter()
            .map(|evals| evals.iter().map(|x| Mod2k::new(x[0], modulus)).collect())
            .collect();
        let rdcf_incremental_evals_mod2k: Vec<Vec<Mod2k>> = rdcf_full_evals
            .iter()
            .map(|evals| evals.iter().map(|x| Mod2k::new(x[0], modulus)).collect())
            .collect();

        let sketch_helper_dcf = match sketch_helper {
            SketchHelper::IntervalFSS {
                sketch_helper_dcf
            } => &**sketch_helper_dcf,
            _ => return Err(anyhow!("Sketch helper for interval fss type mismatch!")),
        };

        // Sketch LDCF
        let sketch_value_ldcf = self.sketch_incremental_dcf(
            &ldcf_incremental_evals_mod2k,
            true,
            sketch_helper_dcf,
            prg,
        )?;

        // Sketch RDCF
        let sketch_value_rdcf = self.sketch_incremental_dcf(
            &rdcf_incremental_evals_mod2k,
            false,
            sketch_helper_dcf,
            prg,
        )?;

        // Checking whether last level of ldcf is shifted by 2*delta from last level of rdcf
        let ldcf_last_layer = &ldcf_incremental_evals_mod2k[self.config.h1]; // Critical point x - delta - 1
        let rdcf_last_layer = &rdcf_incremental_evals_mod2k[self.config.h1]; // Critical point x + delta
        // Convert last layers to dpf
        let ldcf_last_layer_dpf = self.shifted_dcf_to_dpf(ldcf_last_layer, domain_size, 0)
            .map_err(|e| anyhow!("Failed to convert ldcf_last_layer into dpf: {}", e))?;
        let rdcf_last_layer_dpf = self.shifted_dcf_to_dpf(rdcf_last_layer, domain_size, 0)
            .map_err(|e| anyhow!("Failed to convert rdcf_last_layer into dpf: {}", e))?;

        let subtracted = self.subtract_shifted_mod2k_to_modp(
            &rdcf_last_layer_dpf,
            &ldcf_last_layer_dpf,
            (2 * delta + 1) as usize,
            &self.barrett_ctx,
        ).map_err(|e| anyhow!("Failed to subtract shifted mod2k vectors for linf consistency: {}", e))?;
        let rs = sample_modp_vec(domain_size, &self.barrett_ctx, prg)
            .map_err(|e| anyhow!("Failed to sample random vector for linf consistency: {}", e))?;
        let z_consistency = dot_product_modp(&self.barrett_ctx, &subtracted, &rs)
            .map_err(|e| anyhow!("Failed to compute linf consistency dot product: {}", e))?;

        Ok(SketchValues::Linf {
            ldcf: Box::new(sketch_value_ldcf),
            rdcf: Box::new(sketch_value_rdcf),
            consistency: z_consistency,
        })
    }

    #[allow(dead_code)]
    fn sketch_lp_one_dimension<'a>(
        &'a self,
        key: &DistanceFSSKey,
        p: usize,
        sketch_helper: &SketchHelper,
        prg: &mut PRG,
    ) -> Result<SketchValues<'a>> {
        let domain_size = 1usize << self.config.h1;
        let modulus = 1u128 << self.config.h2;
        let _delta = self.config.delta;

        let ldcf_key0 = key.left_fss0();
        let ldcf_key1 = key.left_fss1();
        let rdcf_key0 = key.right_fss0();
        let rdcf_key1 = key.right_fss1();

        let ldcf0_full_evals = ldcf_key0
            .full_domain_incremental_eval(modulus, self.config.h1)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        let ldcf1_full_evals = ldcf_key1
            .full_domain_incremental_eval(modulus, self.config.h1)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        let rdcf0_full_evals = rdcf_key0
            .full_domain_incremental_eval(modulus, self.config.h1)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        let rdcf1_full_evals = rdcf_key1
            .full_domain_incremental_eval(modulus, self.config.h1)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;

        // Extract reference dpf from the last layer of ldcf1 and sketch it
        let reference_dcf: Vec<Mod2k> = (0..domain_size).map(|i| {
            Mod2k::new(ldcf1_full_evals[self.config.h1][i][0], modulus)
        }).collect();
        let mut reference_dpf: Vec<Mod2k> = (0..domain_size-1).map(|i| {
            reference_dcf[i] - reference_dcf[i+1]
        }).collect();
        reference_dpf.push(Mod2k::zero(modulus));
        // Now sketch it
        // Get sketch helper and sketch each ldcf, rdcf relative to the reference
        let sketch_helper_dcf = match sketch_helper {
            SketchHelper::DistanceFSS { 
                sketch_helper_dcf, ..
            } => &**sketch_helper_dcf,
            _ => return Err(anyhow!("Sketch helper for distance fss type mismatch!")),
        };

        let (reference_dpf_sketch_case0, reference_dpf_sketch_case1) = {
            let (scale0, scale1) = sketch_helper_dcf.get_helper_vector(&self.barrett_ctx, domain_size)
                .map_err(|e| anyhow!("Failed to get helper vector: {}", e))?;
            let (evals_unit_vector0, evals_unit_vector1) = self.dpf_to_unit_vector(
                &reference_dpf,
                domain_size,
                &scale0,
                &scale1,
                &self.barrett_ctx,
            )?;

            let rs = sample_modp_vec(domain_size, &self.barrett_ctx, prg)
                .map_err(|e| anyhow!("Failed to sample random vector for reference dpf sketch: {}", e))?;

            let rs2 = element_wise_product_modp(&rs, &rs)
                .map_err(|e| anyhow!("Failed to square random vector for last layer sketch: {}", e))?;

            let z_ast = dot_product_modp(&self.barrett_ctx, &rs, &evals_unit_vector0)
                .map_err(|e| anyhow!("Failed to multiply rs with evals_unit_vector case0: {}", e))?;
            let z2_ast = dot_product_modp(&self.barrett_ctx, &rs2, &evals_unit_vector0)
                .map_err(|e| anyhow!("Failed to multiply rs2 with evals_unit_vector case0: {}", e))?;

            let z_bullet= dot_product_modp(&self.barrett_ctx, &rs, &evals_unit_vector1)
                .map_err(|e| anyhow!("Failed to multiply rs with evals_unit_vector case1: {}", e))?;
            let z2_bullet = dot_product_modp(&self.barrett_ctx, &rs2, &evals_unit_vector1)
                .map_err(|e| anyhow!("Failed to multiply rs2 with evals_unit_vector case1: {}", e))?;

            (
                (z_ast, z2_ast),
                (z_bullet, z2_bullet),
            )
        };

        let sketch_value_ldcf0 = self.sketch_incremental_dcf_shift_payload(
            &ldcf0_full_evals, 
            p+1, 
            self.config.delta as isize, // reference_dpf left shift by delta
            true, 
            &reference_dpf,
            &sketch_helper.get_payload_helper_ldcf0()?,
            prg
        ).map_err(|e| anyhow!("Failed to sketch incremental ldcf0: {}", e))?;
        let sketch_value_ldcf1 = self.sketch_incremental_dcf_shift_payload(
            &ldcf1_full_evals, 
            p+1, 
            0, // no shift
            true, 
            &reference_dpf,
            &sketch_helper.get_payload_helper_ldcf1()?,
            prg
        ).map_err(|e| anyhow!("Failed to sketch incremental ldcf1: {}", e))?;
        let sketch_value_rdcf0 = self.sketch_incremental_dcf_shift_payload(
            &rdcf0_full_evals, 
            p+1, 
            -1, // reference_dpf right shift by 1
            false, 
            &reference_dpf,
            &sketch_helper.get_payload_helper_rdcf0()?,
            prg
        ).map_err(|e| anyhow!("Failed to sketch incremental rdcf0: {}", e))?;
        let sketch_value_rdcf1 = self.sketch_incremental_dcf_shift_payload(
            &rdcf1_full_evals, 
            p+1, 
            -(self.config.delta as isize + 1), // reference_dpf right shift by delta+1
            false, 
            &reference_dpf,
            &sketch_helper.get_payload_helper_rdcf1()?,
            prg
        ).map_err(|e| anyhow!("Failed to sketch incremental rdcf1: {}", e))?;

        Ok(SketchValues::Lp 
        { 
            p, 
            ldcf0: Box::new(sketch_value_ldcf0), 
            ldcf1: Box::new(sketch_value_ldcf1), 
            rdcf0: Box::new(sketch_value_rdcf0), 
            rdcf1: Box::new(sketch_value_rdcf1), 
            reference_dpf_case0: reference_dpf_sketch_case0, 
            reference_dpf_case1: reference_dpf_sketch_case1,
        }
        )
    }
}

// This is the bulk of implmentations for get_sketch_helper
impl SketchPhase {
    fn get_sketch_helper_linf(&self) -> Result<SketchHelper> {
        let barrett_ctx = BarrettCtx::new(self.config.q);
        let modulus = 1u128 << self.config.h2;
        let case_2_const_inv = Modp::one(&barrett_ctx) - Modp::new(&barrett_ctx, modulus);
        let case_2_const = case_2_const_inv.inv().unwrap();
        Ok(SketchHelper::IntervalFSS {
            sketch_helper_dcf: Box::new(SketchHelper::Dcf {
                barrett_ctx: barrett_ctx,
                inv_value: case_2_const.value(),
            }),
        })
    }

    fn get_sketch_helper_lp(&self, p: u32) -> Result<SketchHelper> {
        let modulus = 1u128 << self.config.h2;
        let case_2_const_inv = Modp::one(&self.barrett_ctx) - Modp::new(&self.barrett_ctx, modulus);
        let case_2_const = case_2_const_inv.inv().unwrap();

        let modulus = 1u128 << self.config.h2;
        let domain_size = 1usize << self.config.h1;
        let barrett_ctx = BarrettCtx::new(self.config.q);
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
            if ((i & 1) == 1) ^ (((p_usize + 1 - i) & 1) == 0) {
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
        let out_minus_left = element_wise_subtract_mod2k_vec(&out_payload, &left_payload)
            .map_err(|e| anyhow!("Failed to subtract out_payload and left_payload: {}", e))?;
        let ldcf_0_helper = element_wise_subtract_mod2k_vec(&out_minus_left, &zero_payload)
            .map_err(|e| anyhow!("Failed to compute ldcf0 helper: {}", e))?;
        let ldcf_0_helper = ldcf_0_helper // Shift left by delta+1 
            .iter()
            .map(|v| 
                shift_mod2k(v, self.config.delta as isize + 1, modulus)
                    .map_err(|e| anyhow!("Failed to shift ldcf_0_helper: {}", e))
            )
            .collect::<Result<Vec<_>>>()?;

        // Helper vectors for ldcf1: left_payload - zero_payload.
        let ldcf_1_helper = element_wise_subtract_mod2k_vec(&left_payload, &zero_payload)
            .map_err(|e| anyhow!("Failed to compute ldcf1 helper: {}", e))?;
        let ldcf_1_helper: Vec<Vec<Mod2k>> = ldcf_1_helper // Shift left by 1 because of dpf transformation
            .iter()
            .map(|v| {
                shift_mod2k(v, 1, modulus)
                    .map_err(|e| anyhow!("Failed to shift ldcf_1_helper: {}", e))
            }).collect::<Result<Vec<_>>>()?;
        println!("Length of ldcf_1_helper: {}", ldcf_1_helper.len());

        // Helper vectors for rdcf0: zero_payload - right_payload.
        let rdcf_0_helper = element_wise_subtract_mod2k_vec(&zero_payload, &right_payload)
            .map_err(|e| anyhow!("Failed to compute rdcf0 helper: {}", e))?;

        // Helper vectors for rdcf1: zero_payload - (out_payload - right_payload).
        let out_minus_right = element_wise_subtract_mod2k_vec(&out_payload, &right_payload)
            .map_err(|e| anyhow!("Failed to subtract out_payload and right_payload: {}", e))?;
        let rdcf_1_helper = element_wise_subtract_mod2k_vec(&zero_payload, &out_minus_right)
            .map_err(|e| anyhow!("Failed to compute rdcf1 helper: {}", e))?;
        let rdcf_1_helper: Vec<Vec<Mod2k>> = rdcf_1_helper
            .iter()
            .map(|v| {
                shift_mod2k(v, -(self.config.delta as isize), modulus)
                    .map_err(|e| anyhow!("Failed to shift rdcf_1_helper: {}", e))
            }).collect::<Result<Vec<_>>>()?;

        Ok(SketchHelper::DistanceFSS {
            sketch_helper_dcf: Box::new(SketchHelper::Dcf {
                barrett_ctx: barrett_ctx,
                inv_value: case_2_const.value(),
            }),
            payload_helper_ldcf0: ldcf_0_helper,
            payload_helper_ldcf1: ldcf_1_helper,
            payload_helper_rdcf0: rdcf_0_helper,
            payload_helper_rdcf1: rdcf_1_helper,
        })
    }
}


impl SketchPhase {
    fn get_sketch_data_linf<'a>(
        &self,
        prg: &mut PRG,
    ) -> Result<SketchData<'a>> {
        unimplemented!()
    }

    fn get_sketch_data_lp<'a>(
        &self, 
        prg: &mut PRG,
    ) -> Result<SketchData<'a>> {
        unimplemented!()
    }
}

impl SketchPhase {
    fn verify_linf<'a>(
        &'a self,
        sketch_value: &SketchValues<'a>,
        sketch_data: &SketchData<'a>,
        channel: &mut CommTrackingChannel,
        is_first: bool,
    ) -> Result<Vec<Modp<'a>>> {
        let (ldcf_value, rdcf_value, shift_consistency) = match sketch_value {
            SketchValues::Linf {
                ldcf,
                rdcf,
                consistency,
            } => (&**ldcf, &**rdcf, consistency),
            _ => return Err(anyhow!("SketchValues not in Linf format for verification")),
        };
        let (ldcf_data, rdcf_data) = match sketch_data {
            SketchData::Linf { ldcf, rdcf } => (&**ldcf, &**rdcf),
            _ => return Err(anyhow!("SketchData not in Linf format for verification")),
        };

        let (ldcf_last_layer, ldcf_consistency) =
            self.verify_dcf(ldcf_value, ldcf_data, channel, is_first)?;
        let (rdcf_last_layer, rdcf_consistency) =
            self.verify_dcf(rdcf_value, rdcf_data, channel, is_first)?;

        let mut checks = Vec::new();
        checks.push(ldcf_last_layer);
        checks.extend(ldcf_consistency);
        checks.push(rdcf_last_layer);
        checks.extend(rdcf_consistency);
        checks.push(*shift_consistency);

        Ok(checks)
    }

    fn verify_lp<'a>(
        &'a self,
        sketch_value: &SketchValues<'a>,
        sketch_data: &SketchData<'a>,
        channel: &mut CommTrackingChannel,
        is_first: bool,
    ) -> Result<Vec<Modp<'a>>> {
        let (
            p_value,
            ldcf0_value,
            ldcf1_value,
            rdcf0_value,
            rdcf1_value,
            reference_dpf_case0,
            reference_dpf_case1,
        ) = match sketch_value {
            SketchValues::Lp {
                p,
                ldcf0,
                ldcf1,
                rdcf0,
                rdcf1,
                reference_dpf_case0,
                reference_dpf_case1,
            } => (
                *p,
                &**ldcf0,
                &**ldcf1,
                &**rdcf0,
                &**rdcf1,
                reference_dpf_case0,
                reference_dpf_case1,
            ),
            _ => return Err(anyhow!("SketchValues not in Lp format for verification")),
        };
        let (
            p_data,
            ldcf0_data,
            ldcf1_data,
            rdcf0_data,
            rdcf1_data,
            z_ast_triple,
            z_bullet_triple,
            z_triple,
        ) = match sketch_data {
            SketchData::Lp {
                p,
                ldcf0,
                ldcf1,
                rdcf0,
                rdcf1,
                z_ast,
                z_bullet,
                z,
            } => (
                *p,
                &**ldcf0,
                &**ldcf1,
                &**rdcf0,
                &**rdcf1,
                z_ast,
                z_bullet,
                z,
            ),
            _ => return Err(anyhow!("SketchData not in Lp format for verification")),
        };

        ensure!(
            p_value == p_data,
            "Mismatch p between sketch values ({}) and sketch data ({})",
            p_value,
            p_data
        );

        // Verify reference DPF sketch
        let ref_case0_sq = self.beaver_multiply(
            reference_dpf_case0.0,
            reference_dpf_case0.0,
            z_ast_triple,
            channel,
            is_first,
        )?;
        let ref_case0 = ref_case0_sq - reference_dpf_case0.1;
        let ref_case1_sq = self.beaver_multiply(
            reference_dpf_case1.0,
            reference_dpf_case1.0,
            z_bullet_triple,
            channel,
            is_first,
        )?;
        let ref_case1 = ref_case1_sq - reference_dpf_case1.1;
        let ref_check = self.beaver_multiply(ref_case0, ref_case1, z_triple, channel, is_first)?;

        // Verify each payload component
        let mut checks = vec![ref_check];
        checks.extend(self.verify_dcf_payload(
            ldcf0_value,
            ldcf0_data,
            channel,
            is_first,
        )?);
        checks.extend(self.verify_dcf_payload(
            ldcf1_value,
            ldcf1_data,
            channel,
            is_first,
        )?);
        checks.extend(self.verify_dcf_payload(
            rdcf0_value,
            rdcf0_data,
            channel,
            is_first,
        )?);
        checks.extend(self.verify_dcf_payload(
            rdcf1_value,
            rdcf1_data,
            channel,
            is_first,
        )?);

        Ok(checks)
    }

    fn verify_dcf<'a>(
        &'a self,
        sketch_value: &SketchValues<'a>,
        sketch_data: &SketchData<'a>,
        channel: &mut CommTrackingChannel,
        is_first: bool,
    ) -> Result<(Modp<'a>, Vec<Modp<'a>>)> {
        let (last_layer_case0, last_layer_case1, consistency_values) = match sketch_value {
            SketchValues::Dcf {
                last_layer_case0,
                last_layer_case1,
                consistency,
            } => (last_layer_case0, last_layer_case1, consistency),
            _ => return Err(anyhow!("SketchValues not in Dcf format for verification")),
        };

        let (triple_z_ast, triple_z_bullet, triple_z, consistency_triples) = match sketch_data {
            SketchData::Dcf {
                z_ast,
                z_bullet,
                z,
                consistency,
            } => (z_ast, z_bullet, z, consistency),
            _ => return Err(anyhow!("SketchData not in Dcf format for verification")),
        };

        ensure!(
            consistency_values.len() == consistency_triples.len(),
            "Mismatch DCF consistency length: values {} vs triples {}",
            consistency_values.len(),
            consistency_triples.len()
        );

        let z_ast_sq = self.beaver_multiply(
            last_layer_case0.0,
            last_layer_case0.0,
            triple_z_ast,
            channel,
            is_first,
        )?;
        let z_case0 = z_ast_sq - last_layer_case0.1;
        let z_bullet_sq = self.beaver_multiply(
            last_layer_case1.0,
            last_layer_case1.0,
            triple_z_bullet,
            channel,
            is_first,
        )?;
        let z_case1 = z_bullet_sq - last_layer_case1.1;
        let last_layer_check =
            self.beaver_multiply(z_case0, z_case1, triple_z, channel, is_first)?;

        let mut consistency_checks = Vec::with_capacity(consistency_values.len());
        for (value_pair, triple) in consistency_values.iter().zip(consistency_triples.iter()) {
            let check =
                self.beaver_multiply(value_pair.0, value_pair.1, triple, channel, is_first)?;
            consistency_checks.push(check);
        }

        Ok((last_layer_check, consistency_checks))
    }

    fn verify_dcf_payload<'a>(
        &'a self,
        sketch_value: &SketchValues<'a>,
        sketch_data: &SketchData<'a>,
        channel: &mut CommTrackingChannel,
        is_first: bool,
    ) -> Result<Vec<Modp<'a>>> {
        let (length_value, last_layer_consistency, consistency_values) = match sketch_value {
            SketchValues::DcfPayload {
                length,
                last_layer_consistency,
                consistency,
            } => (*length, last_layer_consistency, consistency),
            _ => return Err(anyhow!("SketchValues not in DcfPayload format for verification")),
        };
        let (length_data, consistency_triples) = match sketch_data {
            SketchData::DcfPayload { length, consistency } => (*length, consistency),
            _ => return Err(anyhow!("SketchData not in DcfPayload format for verification")),
        };

        ensure!(
            length_value == length_data,
            "Mismatch payload length for verification: values {} vs data {}",
            length_value,
            length_data
        );
        ensure!(
            last_layer_consistency.len() == length_value,
            "Unexpected last layer payload length: {} vs declared {}",
            last_layer_consistency.len(),
            length_value
        );
        ensure!(
            consistency_values.len() == consistency_triples.len(),
            "Mismatch DCF payload consistency levels: values {} vs triples {}",
            consistency_values.len(),
            consistency_triples.len()
        );

        let mut checks = Vec::new();
        checks.extend(last_layer_consistency.iter().copied());

        for (level_idx, (value_pairs, triples)) in consistency_values
            .iter()
            .zip(consistency_triples.iter())
            .enumerate()
        {
            ensure!(
                value_pairs.len() == triples.len(),
                "Mismatch payload consistency count at level {}: values {} vs triples {}",
                level_idx + 1,
                value_pairs.len(),
                triples.len()
            );
            for (value_pair, triple) in value_pairs.iter().zip(triples.iter()) {
                checks.push(self.beaver_multiply(
                    value_pair.0,
                    value_pair.1,
                    triple,
                    channel,
                    is_first,
                )?);
            }
        }

        Ok(checks)
    }

    fn beaver_multiply<'a>(
        &'a self,
        x: Modp<'a>,
        y: Modp<'a>,
        triple: &TripleModp<'a>,
        channel: &mut CommTrackingChannel,
        is_first: bool,
    ) -> Result<Modp<'a>> {
        let (a, b, c) = *triple;
        let d = x - a;
        let e = y - b;

        // Open d and e with the counterpart
        let send_opening =
            |val: &Modp<'a>, chan: &mut CommTrackingChannel| -> Result<()> {
                let buf = val.value().to_le_bytes();
                chan.write_bytes(&buf)
                    .map_err(|er| anyhow!("Failed to send opening share: {}", er))
            };
        send_opening(&d, channel)?;
        send_opening(&e, channel)?;
        channel
            .flush()
            .map_err(|er| anyhow!("Failed to flush opening shares: {}", er))?;

        let mut buf_d = [0u8; 16];
        channel
            .read_bytes(&mut buf_d)
            .map_err(|er| anyhow!("Failed to receive opening share d: {}", er))?;
        let mut buf_e = [0u8; 16];
        channel
            .read_bytes(&mut buf_e)
            .map_err(|er| anyhow!("Failed to receive opening share e: {}", er))?;

        let d_open = d + Modp::new(&self.barrett_ctx, u128::from_le_bytes(buf_d));
        let e_open = e + Modp::new(&self.barrett_ctx, u128::from_le_bytes(buf_e));

        let mut prod_share = c + d_open * b + e_open * a;
        if is_first {
            prod_share = prod_share + d_open * e_open;
        }

        Ok(prod_share)
    }
}

impl SketchPhase {
    fn dcf_to_dpf_ringvec(
        &self,
        evals: &[RingVec],
        length: usize,
        domain_size: usize,
    ) -> Result<Vec<RingVec>> {
        // Transform from dcf payload to dpf payload by taking differences between consecutive elements
        ensure!(
            evals.len() == domain_size,
            "length mismatch: evals.len() = {}, domain_size = {}", evals.len(), domain_size,
        );
        let mut evals_dpf: Vec<RingVec> = (0..(domain_size-1))
            .map(|i| {
                evals[i].clone() - evals[i+1].clone()
            }).collect();
        let modulus = 1u128 << self.config.h2;
        evals_dpf.push(RingVec::zero_with_len(length, modulus)?);
        Ok(evals_dpf)
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

        let scale0_product = element_wise_product_modp(&evals_modp, scale0)
            .map_err(|e| anyhow!("Failed to compute element-wise product for unit vector case0: {}", e))?;
        let scale1_product = element_wise_product_modp(&evals_modp, scale1)
            .map_err(|e| anyhow!("Failed to compute element-wise product for unit vector case1: {}", e))?;

        Ok((scale0_product, scale1_product))
    }
}

impl SketchPhase {
    #[allow(dead_code)]
    fn sketch_incremental_dcf<'a>(
        &'a self,
        incremental_evals: &[Vec<Mod2k>],
        ldcf_or_rdcf: bool,
        sketch_helper: &SketchHelper,
        prg: &mut PRG,
    ) -> Result<SketchValues<'a>> {
        ensure!(
            incremental_evals.len() == self.config.h1 + 1,
            "Height mismatch: expected {}, got {}",
            self.config.h1 + 1,
            incremental_evals.len()
        );

        let domain_size = 1usize << self.config.h1;

        // Only sketch DCF property at the last layer.
        let (last_layer_sketch_case0, last_layer_sketch_case1) = {
            let last_layer = &incremental_evals[self.config.h1];
            let last_layer_dpf = self.shifted_dcf_to_dpf(last_layer, domain_size, 0)
                .map_err(|e| anyhow!("Failed to convert last_layer into dpf: {}", e))?;
            let (scale0, scale1) = sketch_helper.get_helper_vector(&self.barrett_ctx, domain_size)
                .map_err(|e| anyhow!("Failed to get helper vector: {}", e))?;

            let (evals_unit_vector0, evals_unit_vector1) = self.dpf_to_unit_vector(
                &last_layer_dpf,
                domain_size,
                &scale0,
                &scale1,
                &self.barrett_ctx,
            )?;

            let rs = sample_modp_vec(domain_size, &self.barrett_ctx, prg)
                .map_err(|e| anyhow!("Failed to sample random vector for last layer sketch: {}", e))?;

            let rs2 = element_wise_product_modp(&rs, &rs)
                .map_err(|e| anyhow!("Failed to square random vector for last layer sketch: {}", e))?;

            let z_ast = dot_product_modp(&self.barrett_ctx, &rs, &evals_unit_vector0)
                .map_err(|e| anyhow!("Failed to multiply rs with evals_unit_vector case0: {}", e))?;
            let z2_ast = dot_product_modp(&self.barrett_ctx, &rs2, &evals_unit_vector0)
                .map_err(|e| anyhow!("Failed to multiply rs2 with evals_unit_vector case0: {}", e))?;

            let z_bullet= dot_product_modp(&self.barrett_ctx, &rs, &evals_unit_vector1)
                .map_err(|e| anyhow!("Failed to multiply rs with evals_unit_vector case1: {}", e))?;
            let z2_bullet = dot_product_modp(&self.barrett_ctx, &rs2, &evals_unit_vector1)
                .map_err(|e| anyhow!("Failed to multiply rs2 with evals_unit_vector case1: {}", e))?;

            (
                (z_ast, z2_ast),
                (z_bullet, z2_bullet),
            )
        };

        // Sketch consistency between earlier levels
        let mut consistency_sketches = Vec::new();
        for level in 1..self.config.h1 {
            let evals_i_duplicated = duplicate_vector::<Mod2k>(&incremental_evals[level])
                .map_err(|e| anyhow!("Failed to duplicate vector level {}: {}", level, e))?;
            let next = &incremental_evals[level + 1];
            let (evals_subtracted_case0, evals_subtracted_case1) = if ldcf_or_rdcf {
                // LDCF
                // So we have f(x) = a if x < alpha
                // Which means the last value equals to a would be alpha-1
                // Currently on layer i, we have prefix alpha_i on this layer and prefix alpha_next on next layer
                // Either alpha_next = 2 * alpha_i or alpha_next = 2 * alpha_i + 1
                // Last value of this layer at alpha_i - 1
                // So last value of this layer duplicated is 2 * alpha_i - 1
                // Last value of next layer is either 2 * alpha_i - 1 or 2 * alpha_i
                // So either next layer left shift 1 or next layer would be equal to this layer duplicated
                let next_layer_case0 = next.clone();
                let next_layer_case1: Vec<Mod2k> = 
                    [next[1..].to_vec(), vec![next[next.len() - 1]; 1]].concat();
                let subtracted_case0 = element_wise_subtract_mod2k(&evals_i_duplicated, &next_layer_case0)
                    .map_err(|e| anyhow!("Failed to subtract evals_i_duplicated and next_layer_case0: {}", e))?;
                let subtracted_case1 = element_wise_subtract_mod2k(&evals_i_duplicated, &next_layer_case1)
                    .map_err(|e| anyhow!("Failed to subtract evals_i_duplicated and next_layer_case1: {}", e))?;
                (
                    mod2k_to_modp(&self.barrett_ctx, &subtracted_case0)
                        .map_err(|e| anyhow!("Failed to convert subtracted_case0 to modp: {}", e))?,
                    mod2k_to_modp(&self.barrett_ctx, &subtracted_case1)
                        .map_err(|e| anyhow!("Failed to convert subtracted_case1 to modp: {}", e))?,
                )
            } else {
                // RDCF
                // So we have f(x) = a if x <= alpha
                // Which means the last value equals to a would be alpha
                // Currently on layer i, we have prefix alpha_i on this layer and prefix alpha_next on next layer
                // Either alpha_next = 2 * alpha_i or alpha_next = 2 * alpha_i + 1
                // Last value of this layer at alpha_i
                // So last value of this layer duplicated is 2 * alpha_i + 1
                // Last value of next layer is either 2 * alpha_i or 2 * alpha_i + 1
                // So either next layer right shift 1 or next layer would be equal to this layer duplicated
                let next_layer_case0 = next.clone();
                let next_layer_case1: Vec<Mod2k> = 
                    [vec![next[0]; 1], next[..next.len()-1].to_vec()].concat();
                let subtracted_case0 = element_wise_subtract_mod2k(&evals_i_duplicated, &next_layer_case0)
                    .map_err(|e| anyhow!("Failed to subtract evals_i_duplicated and next_layer_case0: {}", e))?;
                let subtracted_case1 = element_wise_subtract_mod2k(&evals_i_duplicated, &next_layer_case1)
                    .map_err(|e| anyhow!("Failed to subtract evals_i_duplicated and next_layer_case1: {}", e))?;
                (
                    mod2k_to_modp(&self.barrett_ctx, &subtracted_case0)
                        .map_err(|e| anyhow!("Failed to convert subtracted_case0 to modp: {}", e))?,
                    mod2k_to_modp(&self.barrett_ctx, &subtracted_case1)
                        .map_err(|e| anyhow!("Failed to convert subtracted_case1 to modp: {}", e))?,
                )
            };
            // println!("evals_subtracted_case0: {:?}", evals_subtracted_case0);
            // println!("evals_subtracted_case1: {:?}", evals_subtracted_case1);

            let domain_size = evals_subtracted_case0.len();
            let rs = sample_modp_vec(domain_size, &self.barrett_ctx, prg)
                .map_err(|e| anyhow!("Failed to sample random vector for DCF consistency: {}", e))?;

            let z_ast = dot_product_modp(&self.barrett_ctx, &rs, &evals_subtracted_case0)
                .map_err(|e| anyhow!("Failed to compute DCF consistency z_ast at level {}: {}", level, e))?;
            let z_bullet = dot_product_modp(&self.barrett_ctx, &rs, &evals_subtracted_case1)
                .map_err(|e| anyhow!("Failed to compute DCF consistency z_bullet at level {}: {}", level, e))?;

            consistency_sketches.push((z_ast, z_bullet));
        }

        Ok(SketchValues::Dcf {
            last_layer_case0: last_layer_sketch_case0,
            last_layer_case1: last_layer_sketch_case1,
            consistency: consistency_sketches,
        })
    }

    #[allow(dead_code)]
    fn sketch_incremental_dcf_shift_payload<'a>(
        &'a self,
        incremental_evals: &[Vec<RingVec>],
        length: usize,
        shift: isize,
        ldcf_or_rdcf: bool,
        reference_dpf: &[Mod2k],
        payload_helper: &[Vec<Mod2k>],
        prg: &mut PRG,
    ) -> Result<SketchValues<'a>> {
        let barrett_ctx = &self.barrett_ctx;
        let modulus = 1u128 << self.config.h2;

        ensure!(
            payload_helper.len() == length,
            "payload helper length mismatch: payload_helper.len() = {}, length = {}", payload_helper.len(), length,
        );

        let domain_size = 1 << self.config.h1;

        // Shift and rescale last layer payloads
        let last_layer = &incremental_evals[self.config.h1];
        let last_layer_dpf = self.dcf_to_dpf_ringvec(last_layer, length, domain_size)
            .map_err(|e| anyhow!("Failed to convert last layer into dcf: {}", e))?;
        let reference_dpf_shifted = shift_mod2k(reference_dpf, shift, modulus)
            .map_err(|e| anyhow!("Failed to shift reference dpf: {}", e))?;
        let last_layer_payloads_subtracted: Vec<Vec<Modp>> = (0..length)
            .map(|i| -> Result<Vec<Modp>> {
                let rescaled: Vec<Mod2k> = reference_dpf_shifted 
                    .iter()
                    .zip(payload_helper[i].iter())
                    .map(|(&x, helper)| x * *helper)
                    .collect();
                let compare: Vec<Mod2k> = last_layer_dpf
                    .iter()
                    .map(|components| Mod2k::new(components[i], modulus))
                    .collect();
                self.subtract_shifted_mod2k_to_modp(&rescaled, &compare, 0, barrett_ctx)
                    .map_err(|e| anyhow!("Failed to subtract last layer payload for component {}: {}", i, e))
            })
            .collect::<Result<Vec<_>>>()?;

        // Sketch last layer payload consistency
        let rs = sample_modp_vec(domain_size, barrett_ctx, prg)
            .map_err(|e| anyhow!("Failed to sample random vector for payload consistency: {}", e))?;
        let mut rs_pow = vec![Modp::one(barrett_ctx); domain_size];
        let mut last_layer_consistency_sketch = Vec::with_capacity(length);
        for subtracted in &last_layer_payloads_subtracted {
            rs_pow = element_wise_product_modp(&rs, &rs_pow)
                .map_err(|e| anyhow!("Failed to build power vector for payload consistency: {}", e))?;
            let dot = dot_product_modp(barrett_ctx, &rs_pow, subtracted)
                .map_err(|e| anyhow!("Failed to compute payload last layer dot product: {}", e))?;
            last_layer_consistency_sketch.push(dot);
        }

        // Sketch consistency between earlier levels
        let mut consistency_sketches = Vec::new();
        for level in 1..self.config.h1 {
            let evals_i_duplicated = duplicate_vector::<RingVec>(&incremental_evals[level])
                .map_err(|e| anyhow!("Failed to duplicate vector level {}: {}", level, e))?;
            let next = &incremental_evals[level + 1];
            let (evals_subtracted_case0, evals_subtracted_case1) = if ldcf_or_rdcf {
                // LDCF
                // So we have f(x) = a if x < alpha
                // Which means the last value equals to a would be alpha-1
                // Currently on layer i, we have prefix alpha_i on this layer and prefix alpha_next on next layer
                // Either alpha_next = 2 * alpha_i or alpha_next = 2 * alpha_i + 1
                // Last value of this layer at alpha_i - 1
                // So last value of this layer duplicated is 2 * alpha_i - 1
                // Last value of next layer is either 2 * alpha_i - 1 or 2 * alpha_i
                // So either next layer left shift 1 or next layer would be equal to this layer duplicated
                let next_layer_case0 = next.clone();
                let next_layer_case1: Vec<RingVec> = 
                    [next[1..].to_vec(), vec![next[next.len() - 1].clone(); 1]].concat();
                let subtracted_case0 = element_wise_subtract_ringvec(&evals_i_duplicated, &next_layer_case0)
                    .map_err(|e| anyhow!("Failed to subtract evals_i_duplicated and next_layer_case0: {}", e))?;
                let subtracted_case1 = element_wise_subtract_ringvec(&evals_i_duplicated, &next_layer_case1)
                    .map_err(|e| anyhow!("Failed to subtract evals_i_duplicated and next_layer_case1: {}", e))?;
                (
                    ringvec_to_modp_vec(&self.barrett_ctx, &subtracted_case0)
                        .map_err(|e| anyhow!("Failed to convert subtracted_case0 to modp: {}", e))?,
                    ringvec_to_modp_vec(&self.barrett_ctx, &subtracted_case1)
                        .map_err(|e| anyhow!("Failed to convert subtracted_case1 to modp: {}", e))?,
                )
            } else {
                // RDCF
                // So we have f(x) = a if x <= alpha
                // Which means the last value equals to a would be alpha
                // Currently on layer i, we have prefix alpha_i on this layer and prefix alpha_next on next layer
                // Either alpha_next = 2 * alpha_i or alpha_next = 2 * alpha_i + 1
                // Last value of this layer at alpha_i
                // So last value of this layer duplicated is 2 * alpha_i + 1
                // Last value of next layer is either 2 * alpha_i or 2 * alpha_i + 1
                // So either next layer right shift 1 or next layer would be equal to this layer duplicated
                let next_layer_case0 = next.clone();
                let next_layer_case1: Vec<RingVec> = 
                    [vec![next[0].clone(); 1], next[..next.len()-1].to_vec()].concat();
                let subtracted_case0 = element_wise_subtract_ringvec(&evals_i_duplicated, &next_layer_case0)
                    .map_err(|e| anyhow!("Failed to subtract evals_i_duplicated and next_layer_case0: {}", e))?;
                let subtracted_case1 = element_wise_subtract_ringvec(&evals_i_duplicated, &next_layer_case1)
                    .map_err(|e| anyhow!("Failed to subtract evals_i_duplicated and next_layer_case1: {}", e))?;
                (
                    ringvec_to_modp_vec(&self.barrett_ctx, &subtracted_case0)
                        .map_err(|e| anyhow!("Failed to convert subtracted_case0 to modp: {}", e))?,
                    ringvec_to_modp_vec(&self.barrett_ctx, &subtracted_case1)
                        .map_err(|e| anyhow!("Failed to convert subtracted_case1 to modp: {}", e))?,
                )
            };
            // println!("evals_subtracted_case0: {:?}", evals_subtracted_case0);
            // println!("evals_subtracted_case1: {:?}", evals_subtracted_case1);

            let domain_size = evals_subtracted_case0.len();
            let rs = sample_modp_vec(domain_size, &self.barrett_ctx, prg)
                .map_err(|e| anyhow!("Failed to sample random vector for payload DCF consistency: {}", e))?;
            let mut rs_pow = vec![Modp::one(barrett_ctx); domain_size];

            let domain_size = 1 << (level + 1);
            let mut consistency_this_level = Vec::new();
            for i in 0..length {
                rs_pow = element_wise_product_modp(&rs, &rs_pow)
                    .map_err(|e| anyhow!("Failed to do element wise product between rs and rs_pow at level {} index {}: {}", level, i, e))?;
                let evals_subtracted_case0_vec: Vec<Modp> = (0..domain_size).map(|j| evals_subtracted_case0[j][i]).collect();
                let evals_subtracted_case1_vec: Vec<Modp> = (0..domain_size).map(|j| evals_subtracted_case1[j][i]).collect();
                let z_ast = dot_product_modp(&self.barrett_ctx, &rs_pow, &evals_subtracted_case0_vec)
                    .map_err(|e| anyhow!("Failed to compute DCF consistency z_ast at level {}: {}", level, e))?;
                let z_bullet = dot_product_modp(&self.barrett_ctx, &rs_pow, &evals_subtracted_case1_vec)
                    .map_err(|e| anyhow!("Failed to compute DCF consistency z_bullet at level {}: {}", level, e))?;
                consistency_this_level.push((z_ast, z_bullet));
            }

            consistency_sketches.push(consistency_this_level);
        }

        Ok(SketchValues::DcfPayload {
            length,
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
        let subtracted = self
            .subtract_shifted_mod2k_to_modp(a, b, shift, barrett_ctx)
            .map_err(|e| anyhow!("Failed to subtract shifted vectors for shift consistency: {}", e))?;

        let mut rs_u128: Vec<u128> = vec![0u128; domain_size];
        prg.random_u128s(&mut rs_u128);
        let rs: Vec<Modp> = rs_u128.iter().map(|x| Modp::new(barrett_ctx, *x)).collect();

        dot_product_modp(barrett_ctx, &rs, &subtracted)
            .map_err(|e| anyhow!("Failed to compute shift consistency dot product: {}", e))
    }

    #[allow(dead_code)]
    fn subtract_shifted_mod2k_to_modp<'a>(
        &self,
        a: &[Mod2k],
        b: &[Mod2k],
        shift: usize,
        ctx: &'a BarrettCtx,
    ) -> Result<Vec<Modp<'a>>> {
        // Only shift left!!!
        let modulus = 1u128 << self.config.h2;
        ensure!(
            shift <= a.len(),
            "shift {} exceeds input length {} in subtract_shifted_mod2k_to_modp",
            shift,
            a.len()
        );
        let shifted_a: Vec<Mod2k> = if shift > 0 {
            // Shift to the left
            [
                a[shift..].to_vec(),
                vec![Mod2k::zero(modulus); shift],
            ]
            .concat()
        } else {
            // No shift
            a.to_vec()
        };

        // println!("shifted_a: {:?}", shifted_a);

        let subtracted = element_wise_subtract_mod2k(&shifted_a, b)
            .map_err(|e| anyhow!("Failed to subtract shifted mod2k vectors: {}", e))?;

        Ok(subtracted.iter().map(|x| Modp::new(ctx, x.val())).collect())
    }
}

fn sample_modp_vec<'a>(
    size: usize,
    barrett_ctx: &'a BarrettCtx,
    prg: &mut PRG,
) -> Result<Vec<Modp<'a>>> {
    let mut rs_u128: Vec<u128> = vec![0u128; size];
    prg.random_u128s(&mut rs_u128);
    Ok(
        rs_u128
            .into_iter()
            .map(|x| Modp::new(barrett_ctx, x))
            .collect()
    )
}

fn sample_modp_triples<'a>(
    num_triples: usize, 
    barrett_ctx: &'a BarrettCtx,
    prg: &mut PRG
) -> Result<(Vec<TripleModp<'a>>, Vec<TripleModp<'a>>)> {
    let a0 = sample_modp_vec(num_triples, barrett_ctx, prg)
        .map_err(|e| anyhow!("Error sampling vector a0 for triple: {}", e))?;
    let a1 = sample_modp_vec(num_triples, barrett_ctx, prg)
        .map_err(|e| anyhow!("Error sampling vector a1 for triple: {}", e))?;
    let b0 = sample_modp_vec(num_triples, barrett_ctx, prg)
        .map_err(|e| anyhow!("Error sampling vector b0 for triple: {}", e))?;
    let b1 = sample_modp_vec(num_triples, barrett_ctx, prg)
        .map_err(|e| anyhow!("Error sampling vector b1 for triple: {}", e))?;
    let a = element_wise_sum_modp(&a0, &a1)
        .map_err(|e| anyhow!("Error getting vector a from a0 + a1: {}", e))?;
    let b = element_wise_sum_modp(&b0, &b1)
        .map_err(|e| anyhow!("Error getting vector b from b0 + b1: {}", e))?;
    // Get c = a * b, then sample c0, then get c1 from c - c0
    let c = element_wise_product_modp(&a, &b)
        .map_err(|e| anyhow!("Error getting c from a * b: {}", e))?;
    let c0 = sample_modp_vec(num_triples, barrett_ctx, prg)
        .map_err(|e| anyhow!("Error sampling vector c0 for triple: {}", e))?;
    let c1 = element_wise_subtract_modp(&c, &c0)
        .map_err(|e| anyhow!("Error getting c1 from c - c0: {}", e))?;

    Ok((
        a0.iter().zip(b0.iter()).zip(c0.iter()).map(|((a, b), c)| (*a, *b, *c)).collect(),
        a1.iter().zip(b1.iter()).zip(c1.iter()).map(|((a, b), c)| (*a, *b, *c)).collect(),
    ))
}

fn shift_mod2k(
    input: &[Mod2k],
    shift: isize,
    modulus: u128,
) -> Result<Vec<Mod2k>> {
    // CAUTION: ONLY WORKS FOR DPF!!!
    let len = input.len();
    let mut out: Vec<Mod2k> = Vec::with_capacity(len);
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
    Ok(out)
}

#[allow(unused)]
fn shift_ringvec(
    input: &[RingVec],
    length: usize,
    shift: isize,
    modulus: u128,
) -> Result<Vec<RingVec>> {
    // CAUTION: ONLY WORKS FOR DPF!!!
    let len = input.len();
    let mut out: Vec<RingVec> = Vec::with_capacity(len);
    if shift > 0 {
        let s = (shift as usize).min(len);
        if s < len {
            out.extend_from_slice(&input[s..]);
        }
        out.extend(std::iter::repeat(RingVec::zero_with_len(length, modulus)?).take(s));
    } else if shift < 0 {
        let s = (-shift as usize).min(len);
        out.extend(std::iter::repeat(RingVec::zero_with_len(length, modulus)?).take(s));
        if s < len {
            out.extend_from_slice(&input[..len - s]);
        }
    } else {
        out.extend_from_slice(input);
    }
    if out.len() > len {
        out.truncate(len);
    } else if out.len() < len {
        out.extend(std::iter::repeat(RingVec::zero_with_len(length, modulus)?).take(len - out.len()));
    }
    Ok(out)
}

fn dot_product_modp<'a>(ctx: &'a BarrettCtx, a: &[Modp<'a>], b: &[Modp<'a>]) -> Result<Modp<'a>> {
    ensure!(a.len() == b.len(), "Length mismatch in dot_product_modp: a.len() = {}, b.len() = {}", a.len(), b.len());
    Ok(
        a.iter()
            .zip(b.iter())
            .fold(Modp::zero(ctx), |acc, (x, y)| acc + (*x * *y))
    )
}

fn element_wise_sum_modp<'a>(a: &[Modp<'a>], b: &[Modp<'a>]) -> Result<Vec<Modp<'a>>> {
    ensure!(
        a.len() == b.len(),
        "length mismatch in element_wise_product_modp: a.len() = {}, b.len() = {}",
        a.len(),
        b.len()
    );
    Ok(a.iter().zip(b.iter()).map(|(x, y)| *x + *y).collect())
}


fn element_wise_product_modp<'a>(a: &[Modp<'a>], b: &[Modp<'a>]) -> Result<Vec<Modp<'a>>> {
    ensure!(
        a.len() == b.len(),
        "length mismatch in element_wise_product_modp: a.len() = {}, b.len() = {}",
        a.len(),
        b.len()
    );
    Ok(a.iter().zip(b.iter()).map(|(x, y)| *x * *y).collect())
}

#[allow(dead_code)]
fn element_wise_subtract_modp<'a>(a: &[Modp<'a>], b: &[Modp<'a>]) -> Result<Vec<Modp<'a>>> {
    ensure!(
        a.len() == b.len(),
        "length mismatch in element_wise_subtract_modp: a.len() = {}, b.len() = {}",
        a.len(),
        b.len()
    );
    Ok(a.iter().zip(b.iter()).map(|(x, y)| *x - *y).collect())
}

fn element_wise_subtract_mod2k(a: &[Mod2k], b: &[Mod2k]) -> Result<Vec<Mod2k>> {
    ensure!(
        a.len() == b.len(),
        "length mismatch in element_wise_subtract_mod2k: a.len() = {}, b.len() = {}",
        a.len(),
        b.len()
    );
    Ok(a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| x.clone() - y.clone())
        .collect())
}

fn element_wise_subtract_mod2k_vec(a: &[Vec<Mod2k>], b: &[Vec<Mod2k>]) -> Result<Vec<Vec<Mod2k>>> {
    ensure!(
        a.len() == b.len(),
        "length mismatch in element_wise_subtract_mod2k_vec: a.len() = {}, b.len() = {}",
        a.len(),
        b.len()
    );
    a.iter()
        .zip(b.iter())
        .map(|(row_a, row_b)| {
            element_wise_subtract_mod2k(row_a, row_b)
                .map_err(|e| anyhow!("Failed to subtract mod2k rows: {}", e))
        })
        .collect()
}

fn element_wise_subtract_ringvec(a: &[RingVec], b: &[RingVec]) -> Result<Vec<RingVec>> {
    ensure!(
        a.len() == b.len(),
        "length mismatch in element_wise_subtract_mod2k: a.len() = {}, b.len() = {}",
        a.len(),
        b.len()
    );
    Ok(a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| x.clone() - y.clone())
        .collect())
}


fn duplicate_vector<T: Clone>(v: &[T]) -> Result<Vec<T>> {
    let mut out = Vec::with_capacity(v.len() * 2);
    for x in v {
        out.push(x.clone());
        out.push(x.clone());
    }
    Ok(out)
}

fn mod2k_to_modp<'a>(ctx: &'a BarrettCtx, a: &[Mod2k]) -> Result<Vec<Modp<'a>>> {
    Ok(a.iter().map(|x| Modp::new(ctx, x.val())).collect())
}

fn ringvec_to_modp_vec<'a>(ctx: &'a BarrettCtx, a: &[RingVec]) -> Result<Vec<Vec<Modp<'a>>>> {
    Ok(a.iter().map(|x| 
        (0..x.len()).map(|i| Modp::new(ctx, x[i])).collect()
    ).collect())
}
