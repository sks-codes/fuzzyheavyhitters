use crate::{
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
        sketch_helper::SketchHelper,
        sketch_types::{SketchConfig, SketchValues},
    },
    randomness::prg::PRG,
};
use anyhow::{anyhow, ensure, Result};

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

    pub fn get_payload_helper(&self) -> Result<Vec<Vec<Mod2k>>> {
        unimplemented!()
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
        let rs = sample_modp_vec(domain_size, &self.barrett_ctx, prg);
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
            .full_domain_incremental_eval(modulus, domain_size)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        let ldcf1_full_evals = ldcf_key1
            .full_domain_incremental_eval(modulus, domain_size)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        let rdcf0_full_evals = rdcf_key0
            .full_domain_incremental_eval(modulus, domain_size)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        let rdcf1_full_evals = rdcf_key1
            .full_domain_incremental_eval(modulus, domain_size)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;

        let ldcf0_incremental_evals_mod2k: Vec<Vec<Vec<Mod2k>>> = ldcf0_full_evals
            .iter()
            .map(|evals| evals.iter().map(|x| 
                (0..x.len()).map(|i| Mod2k::new(x[i], modulus)).collect()
            ).collect())
            .collect();
        let ldcf1_incremental_evals_mod2k: Vec<Vec<Vec<Mod2k>>> = ldcf1_full_evals
            .iter()
            .map(|evals| evals.iter().map(|x| 
                (0..x.len()).map(|i| Mod2k::new(x[i], modulus)).collect()
            ).collect())
            .collect();
        let rdcf0_incremental_evals_mod2k: Vec<Vec<Vec<Mod2k>>> = rdcf0_full_evals
            .iter()
            .map(|evals| evals.iter().map(|x| 
                (0..x.len()).map(|i| Mod2k::new(x[i], modulus)).collect()
            ).collect())
            .collect();
        let rdcf1_incremental_evals_mod2k: Vec<Vec<Vec<Mod2k>>> = rdcf1_full_evals
            .iter()
            .map(|evals| evals.iter().map(|x| 
                (0..x.len()).map(|i| Mod2k::new(x[i], modulus)).collect()
            ).collect())
            .collect();

        // Extract reference dpf from the last layer of ldcf1 and sketch it
        let reference_dcf: Vec<Mod2k> = (0..domain_size).map(|i| {
            ldcf1_incremental_evals_mod2k[self.config.h1 - 1][i][0]
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

        let (scale0, scale1) = sketch_helper_dcf.get_helper_vector(&self.barrett_ctx, domain_size)
            .map_err(|e| anyhow!("Failed to get helper vector: {}", e))?;
        let (evals_unit_vector0, evals_unit_vector1) = self.dpf_to_unit_vector(
            &reference_dpf,
            domain_size,
            &scale0,
            &scale1,
            &self.barrett_ctx,
        )?;

        let z0 = dot_product_modp(&self.barrett_ctx, &evals_unit_vector0, &evals_unit_vector0)
            .map_err(|e| anyhow!("Failed to compute lp z0 dot product: {}", e))?;
        let z1 = dot_product_modp(&self.barrett_ctx, &evals_unit_vector1, &evals_unit_vector1)
            .map_err(|e| anyhow!("Failed to compute lp z1 dot product: {}", e))?;

        let sketch_value_ldcf0 = self.sketch_incremental_dcf_shift_payload(
            &ldcf0_incremental_evals_mod2k, 
            p+1, 
            self.config.h1, 
            -(self.config.delta as isize), // right shift by delta
            true, 
            &reference_dpf,
            &sketch_helper.get_payload_helper_ldcf0()?,
            prg
        ).map_err(|e| anyhow!("Failed to sketch incremental ldcf0: {}", e))?;
        let sketch_value_ldcf1 = self.sketch_incremental_dcf_shift_payload(
            &ldcf1_incremental_evals_mod2k, 
            p+1, 
            self.config.h1, 
            0, // no shift
            true, 
            &reference_dpf,
            &sketch_helper.get_payload_helper_ldcf1()?,
            prg
        ).map_err(|e| anyhow!("Failed to sketch incremental ldcf1: {}", e))?;
        let sketch_value_rdcf0 = self.sketch_incremental_dcf_shift_payload(
            &rdcf0_incremental_evals_mod2k, 
            p+1, 
            self.config.h1, 
            1, // left shift by 1
            true, 
            &reference_dpf,
            &sketch_helper.get_payload_helper_rdcf0()?,
            prg
        ).map_err(|e| anyhow!("Failed to sketch incremental rdcf0: {}", e))?;
        let sketch_value_rdcf1 = self.sketch_incremental_dcf_shift_payload(
            &rdcf1_incremental_evals_mod2k, 
            p+1, 
            self.config.h1, 
            self.config.delta as isize + 1, // left shift by delta+1
            true, 
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
            reference_dpf: (z0, z1) 
        }
        )
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

            let rs = sample_modp_vec(domain_size, &self.barrett_ctx, prg);

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
            let evals_i_duplicated = duplicate_vector_mod2k(&incremental_evals[level]);
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
            let rs = sample_modp_vec(domain_size, &self.barrett_ctx, prg);

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
        incremental_evals: &[Vec<Vec<Mod2k>>],
        length: usize,
        height: usize,
        shift: isize,
        ldcf_or_rdcf: bool,
        reference_dpf: &[Mod2k],
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
                let rescaled: Vec<Mod2k> = reference_dpf 
                    .iter()
                    .zip(payload_helper.iter())
                    .map(|(&x, helper)| x * helper[i])
                    .collect();
                let compare: Vec<Mod2k> = last_layer_shifted
                    .iter()
                    .map(|components| components[i])
                    .collect();
                self.subtract_shifted_mod2k_to_modp(&rescaled, &compare, 0, barrett_ctx)
                    .map_err(|e| anyhow!("Failed to subtract last layer payload for component {}: {}", i, e))
            })
            .collect::<Result<Vec<_>>>()?;

        // Sketch last layer payload consistency (power sums)
        let rs = sample_modp_vec(domain_size, barrett_ctx, prg);
        let mut rs_pow = vec![Modp::one(barrett_ctx); domain_size];
        let mut last_layer_consistency_sketch = Vec::with_capacity(length.saturating_sub(1));
        for subtracted in &last_layer_payloads_subtracted {
            rs_pow = element_wise_product_modp(&rs, &rs_pow)
                .map_err(|e| anyhow!("Failed to build power vector for payload consistency: {}", e))?;
            let dot = dot_product_modp(barrett_ctx, &rs_pow, subtracted)
                .map_err(|e| anyhow!("Failed to compute payload last layer dot product: {}", e))?;
            last_layer_consistency_sketch.push(dot);
        }

        // Sketch consistency between levels for each payload component
        let mut consistency_sketches: Vec<Vec<(Modp, Modp)>> = Vec::new();
        for level in 1..height-1 {
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
                        ).map_err(|e| anyhow!("Failed to subtract payload consistency case0 at level {} component {}: {}", level, component_idx, e))?,
                        self.subtract_shifted_mod2k_to_modp(
                            &evals_next,
                            &evals_i_duplicated,
                            1,
                            barrett_ctx,
                        ).map_err(|e| anyhow!("Failed to subtract payload consistency case1 at level {} component {}: {}", level, component_idx, e))?,
                    )
                } else {
                    // RDCF
                    (
                        self.subtract_shifted_mod2k_to_modp(
                            &evals_i_duplicated,
                            &evals_next,
                            0,
                            barrett_ctx,
                        ).map_err(|e| anyhow!("Failed to subtract payload consistency case0 at level {} component {}: {}", level, component_idx, e))?,
                        self.subtract_shifted_mod2k_to_modp(
                            &evals_i_duplicated,
                            &evals_next,
                            1,
                            barrett_ctx,
                        ).map_err(|e| anyhow!("Failed to subtract payload consistency case1 at level {} component {}: {}", level, component_idx, e))?,
                    )
                };

                let z_ast = dot_product_modp(barrett_ctx, &rs_level, &evals_subtracted_case0)
                    .map_err(|e| anyhow!("Failed to compute payload consistency z_ast at level {} component {}: {}", level, component_idx, e))?;
                let z_bullet = dot_product_modp(barrett_ctx, &rs_level, &evals_subtracted_case1)
                    .map_err(|e| anyhow!("Failed to compute payload consistency z_bullet at level {} component {}: {}", level, component_idx, e))?;
                this_level.push((z_ast, z_bullet));
            }
            consistency_sketches.push(this_level);
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

impl SketchPhase {
    fn incremental_dcf_to_incremental_dpf(
        &self,
        incremental_evals: &[Vec<Mod2k>],
        height: usize,
    ) -> Result<Vec<Vec<Mod2k>>> {
        // Transform every single level into dpf in Z2k first
        // Assume originally, each level contains vectors of form a, a, ..., a, b, b, ..., b
        // The non-zero index in each level is exactly the last a-index in each original level
        let mut incremental_evals_dpf: Vec<Vec<Mod2k>> = Vec::with_capacity(height + 1);
        let modulus = 1u128 << self.config.h2;
        for level in 0..=height {
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
        let barrett_ctx = BarrettCtx::new(self.config.q);
        let modulus = 1u128 << self.config.h2;
        let case_2_const_inv = Modp::one(&barrett_ctx) - Modp::new(&barrett_ctx, modulus);
        let case_2_const = case_2_const_inv.inv().unwrap();

        let modulus = 1u128 << self.config.h2;
        let domain_size = 1usize << self.config.h1;
        let _delta = self.config.delta;
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
        let out_minus_left = element_wise_subtract_mod2k_vec(&out_payload, &left_payload)
            .map_err(|e| anyhow!("Failed to subtract out_payload and left_payload: {}", e))?;
        let ldcf_0_helper = element_wise_subtract_mod2k_vec(&out_minus_left, &zero_payload)
            .map_err(|e| anyhow!("Failed to compute ldcf0 helper: {}", e))?;

        // Helper vectors for ldcf1: left_payload - zero_payload.
        let ldcf_1_helper = element_wise_subtract_mod2k_vec(&left_payload, &zero_payload)
            .map_err(|e| anyhow!("Failed to compute ldcf1 helper: {}", e))?;

        // Helper vectors for rdcf0: zero_payload - right_payload.
        let rdcf_0_helper = element_wise_subtract_mod2k_vec(&zero_payload, &right_payload)
            .map_err(|e| anyhow!("Failed to compute rdcf0 helper: {}", e))?;

        // Helper vectors for rdcf1: zero_payload - (out_payload - right_payload).
        let out_minus_right = element_wise_subtract_mod2k_vec(&out_payload, &right_payload)
            .map_err(|e| anyhow!("Failed to subtract out_payload and right_payload: {}", e))?;
        let rdcf_1_helper = element_wise_subtract_mod2k_vec(&zero_payload, &out_minus_right)
            .map_err(|e| anyhow!("Failed to compute rdcf1 helper: {}", e))?;


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

fn dot_product_modp<'a>(ctx: &'a BarrettCtx, a: &[Modp<'a>], b: &[Modp<'a>]) -> Result<Modp<'a>> {
    ensure!(a.len() == b.len(), "Length mismatch in dot_product_modp: a.len() = {}, b.len() = {}", a.len(), b.len());
    Ok(
        a.iter()
            .zip(b.iter())
            .fold(Modp::zero(ctx), |acc, (x, y)| acc + (*x * *y))
    )
}

#[allow(dead_code)]
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

fn sum_modp<'a>(ctx: &'a BarrettCtx, a: &[Modp<'a>]) -> Result<Modp<'a>> {
    Ok(a.iter().fold(Modp::zero(ctx), |acc, x| acc + *x))
}

#[allow(dead_code)]
fn duplicate_vector_mod2k(a: &[Mod2k]) -> Vec<Mod2k> {
    a.iter().flat_map(|x| [x.clone(), x.clone()]).collect()
}

fn mod2k_to_modp<'a>(ctx: &'a BarrettCtx, a: &[Mod2k]) -> Result<Vec<Modp<'a>>> {
    Ok(a.iter().map(|x| Modp::new(ctx, x.val())).collect())
}