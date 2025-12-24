use crate::{
    aes::AES_KEY_SIZE, channel::CommTrackingChannel, 
    data_structures::{
        modp::{BarrettCtx, Modp},
        mod2k::Mod2k,
        ringvec::RingVec,
    }, 
    fss::{distance::DistanceFSSKey, interval::IntervalFSSKey}, 
    fuzzy_match::{
        share_phase::SharePhaseError, share_types::{DictionaryType, DistanceMetric, ShareMethod}, shared_range::SharedRange, shared_sketch::SketchData, sketch_helper::SketchHelper
    }, randomness::prg::PRG,
};
use anyhow::{anyhow, ensure, Result};

pub struct Sketch {
    h1: usize, // FSS phase input bit length
    h2: usize, // FSS phase output bit length
    q: u128, // sketching values will be in Zq
    delta: u128, // Distance threshold
    d: usize, // Number of dimensions
    method: ShareMethod, // The sharing method used. Only can sketch for FSS now
    metric: DistanceMetric, // Distance metric. Can support sketching both Linf and Lp
    dictionary_type: DictionaryType, // Known or Unknown
}

impl Sketch {
    fn sketch_interval_fss(
        &self, 
        shared_ranges: &[SharedRange],
        sketch_data: &[SketchData],
        seed: [u8; AES_KEY_SIZE],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<bool> {
        Ok(true)
    }

    fn sketch_distance_fss<const N: usize>(
        &self,
        _shared_ranges: &[SharedRange],
    ) -> Result<bool, SharePhaseError> {
        unimplemented!()
    }


    pub fn get_sketch_helper(&self) -> SketchHelper {
        unimplemented!()
    }

    fn sketch_interval_fss_one_dimension(
        &self,
        key: IntervalFSSKey<1>,
        role: bool,
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<Vec<Modp>>> {
        let dpf_helper = match sketch_helper {
            SketchHelper::Linf { dpf } => dpf,
            _ => {
                return Err(anyhow!("Sketch helper for interval fss must be Linf"));
            },
        };

        // We do not parallelize at this level. We only parallelize through multiple key pairs
        let domain_size = 1usize << self.h1;
        let modulus = 1u128 << self.h2;
        let delta = self.delta;
        let barrett_ctx = BarrettCtx::new(self.q);
        
        let ldcf_key = key.ldcf_key();
        let rdcf_key = key.rdcf_key();

        let ldcf_full_evals = ldcf_key.full_domain_incremental_eval(modulus, domain_size);
        let rdcf_full_evals = rdcf_key.full_domain_incremental_eval(modulus, domain_size);

        let ldcf_incremental_evals_mod2k: Vec<Mod2k> = ldcf_full_evals.iter().map(|evals| {
            evals.iter().map(|x| Mod2k::new(x[0], modulus))
        }).collect();
        let rdcf_incremental_evals_mod2k: Vec<Mod2k> = rdcf_full_evals.iter().map(|evals| {
            evals.iter().map(|x| Mod2k::new(x[0], modulus))
        }).collect();

        // Checking whether each level is ldcf
        let (sketch_helper_ldcf, sketch_helper_rdcf) = match sketch_helper {
            SketchHelper::IntervalFSS { sketch_helper_ldcf, sketch_helper_rdcf } => (sketch_helper_ldcf, sketch_helper_rdcf),
            _ => {
                return Err(anyhow!("Sketch helper for interval fss type mismatch!"));
            }
        };
        let z0 = self.sketch_ldcf(&ldcf_incremental_evals_mod2k, self.h1, sketch_helper_ldcf, prg);
        let z1 = self.sketch_rdcf(&rdcf_incremental_evals_mod2k, self.h1, sketch_helper_rdcf, prg);

        // Checking whether last level of ldcf is shifted by 2*delta from last level of rdcf
        let z_shift_inc = self.get_sketch_values_shift_dcf_consistency(&ldcf_full_evals[self.h1-1], &rdcf_full_evals[self.h1-1], 2 * delta as usize, domain_size, prg);

        Ok(vec![z_ldcf, z_rdcf, z_ldcf_inc, z_rdcf_inc, z_shift_inc])
    }

    fn sketch_distance_fss_l1_one_dimension(
        &self, 
        key: DistanceFSSKey<2>,
        role: bool, 
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<Modp>> {
        let domain_size = 1usize << self.h1;
        let modulus = 1u128 << self.h2;
        let delta = self.delta;

        let ldcf_key0 = key.left_fss0();       
        let ldcf_key1 = key.left_fss1();
        let rdcf_key0 = key.right_fss0();       
        let rdcf_key1 = key.right_fss1();
        
        let ldcf0_full_evals = ldcf_key0.full_domain_incremental_eval(modulus, domain_size);
        let ldcf1_full_evals = ldcf_key1.full_domain_incremental_eval(modulus, domain_size);
        let rdcf0_full_evals = rdcf_key0.full_domain_incremental_eval(modulus, domain_size);
        let rdcf1_full_evals = rdcf_key1.full_domain_incremental_eval(modulus, domain_size);


        let ldcf0_incremental_evals_mod2k: Vec<Mod2k> = ldcf0_full_evals.iter().map(|evals| {
            evals.iter().map(|x| Mod2k::new(x[0], modulus))
        }).collect();
        let ldcf1_incremental_evals_mod2k: Vec<Mod2k> = rdcf1_full_evals.iter().map(|evals| {
            evals.iter().map(|x| Mod2k::new(x[0], modulus))
        }).collect();
        let rdcf0_incremental_evals_mod2k: Vec<Mod2k> = rdcf0_full_evals.iter().map(|evals| {
            evals.iter().map(|x| Mod2k::new(x[0], modulus))
        }).collect();
        let rdcf1_incremental_evals_mod2k: Vec<Mod2k> = rdcf1_full_evals.iter().map(|evals| {
            evals.iter().map(|x| Mod2k::new(x[0], modulus))
        }).collect();

        // Transform every incremental evals into dpf

        // Sketch each dpf

        // Sketch last layer consistency between dpfs (shifted by delta, equal, ...)
    }

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
        ensure!(evals.len() == domain_size, "length mismatch between dcf sketch vector and domain_size");
        let mut evals_dpf: Vec<Mod2k> = (0..(domain_size-1)).map(
            |i| evals[i] - evals[i+1]
        ).collect();

        let modulus = 1u128 << self.h2;
        evals_dpf.push(Mod2k::zero(modulus));
        self.shifted_dpf_to_dpf(evals_dpf, domain_size, shift)
    }

    fn shifted_dpf_to_dpf(
        &self,
        evals: &[Mod2k],
        domain_size: usize,
        shift: usize,
    ) -> Result<Vec<Mod2k>> {
        let modulus = 1u128 << self.h2;
        let shifted_evals = if shift < 0 {
            [&evals[abs_shift..], vec![Mod2k::zero(modulus); abs_shift].as_slice()].concat()
        } else {
            [vec![Mod2k::zero(modulus); abs_shift].as_slice(), &evals[..domain_size-abs_shift]].concat()
        };
        shifted_evals
    }

    fn dpf_to_unit_vector(
        &self,
        evals: &[Mod2k],
        domain_size: usize,
        scale0: &[u128],
        scale1: &[u128],
    ) -> Result<(Vec<Modp>, Vec<Modp>)> {
        // From a dpf in Z2k, there would always be two cases of payload in the non-zero entry
        // Either the entries differ by payload, or the entries differ by payload - 2^k.
        let barrett_ctx = BarrettCtx::new(self.q);
        let evals_modp: Vec<Modp> = evals.iter().map(|x| {
            Modp::new(&barrett_ctx, x.val())
        }).collect();
        let scale0_modp: Vec<Modp> = scale0.iter().map(|x| {
            Modp::new(&barrett_ctx, x)
        }).collect();
        let scale1_modp: Vec<Modp> = scale1.iter().map(|x| {
            Modp::new(&barrett_ctx, x)
        }).collect();

        (
            element_wise_product_modp(&evals, &scale0_modp),
            element_wise_product_modp(&evals, &scale1_modp),
        )
    }

    fn sketch_unit_vector(
        &self,
        evals: &[Modp],
        domain_size: usize,
        prg: &mut PRG,
    ) -> Result<(Modp, Modp)> {
        ensure!(evals.len() == domain_size, "length mismatch between dpf sketch vector and domain_size");
        // Prepare transforming to Zq
        let barrett_ctx = BarrettCtx::new(self.q);

        // Generate random r0, r1, ...
        let mut rs_u128: Vec<u128> = vec![0u128; domain_size];
        prg.random_u128s(&mut rs_u128);
        let rs: Vec<Modp> = rs_u128.iter().map(|x| Modp::new(&barrett_ctx, x)).collect();

        // Sketch values for the first case
        let z = dot_product_modp(&evals, &rs); // r0 * case_1[0] + r1 * case_1[1] + ...
        let rs2: Vec<Modp> = element_wise_product_modp(&rs, &rs); // r0^2, r1^2, ... 
        let z2 = dot_product_modp(&evals, &rs2); // r0^2 * case_1[0] + r1^2 * case_1[1] + ...

        Ok((z, z2))
    }

    fn incremental_dcf_to_incremental_dpf (
        &self, 
        incremental_evals: &[Vec<Mod2k>],
        height: usize,
    ) -> Result<Vec<Vec<Mod2k>>> {
        // Transform every single level into dpf in Z2k first
        // Assume originally, each level contains vectors of form a, a, ..., a, b, b, ..., b
        // The non-zero index in each level is exactly the last a-index in each original level
        let incremental_evals_dpf = Vec::new();
        for level in 0..height {
            let evals = incremental_evals[level];
            let domain_size = 1usize << level;
            let evals_dpf = self.shifted_dcf_to_dpf(evals, domain_size, 0)
                .map_err("Failed to transform dcf at level {level} of ldcf into dpf.")?;
            incremental_evals_dpf.push(evals_dpf);
        }

        Ok(incremental_evals_dpf)
    }

    fn sketch_incremental_dpf(
        &self,
        incremental_evals: &[Vec<Mod2k>],
        height: usize,
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<(Modp, Modp)>> {
        ensure!(incremental_evals.len() == height, "Height mismatch, expected {height}, get {incremental_evals.len()}");
        // Check whether sketch helper is for ldcf
        match sketch_helper {
            SketchHelper::Ldcf { .. } => {},
            _ => {
                return Err(anyhow!("Mismatch in sketch_helper type"))
            },
        }

        // Sketch each level dpf
        for level in 0..height {
            let (scale0, scale1) = sketch_helper.get_helper_vector(level);
            let (evals_unit_vector0, evals_unit_vector1) = self.dpf_to_unit_vector(
                incremental_evals[level],
                1 << level,
                scale0,
                scale1,
            );

        }

        // Sketch consistency between levels 
        for level in 0..height-1 {
            let evals_i_duplicated = duplicate_vector(incremental_evals[level]);
            let barrett_ctx = sketch_helper.barrett_ctx();
            let evals_subtracted_case0 = subtract_shifted_mod2k_to_modp(evals_i_duplicated, incremental_evals_dpf[level+1], 0, &barrett_ctx); // Recheck later
            let evals_subtracted_case1 = subtract_shifted_mod2k_to_modp(evals_i_duplicated, incremental_evals_dpf[level+1], 1, &barrett_ctx); // Recheck later

            // Generate random r0, r1, ...
            let domain_size = 1 << level;
            let mut rs_u128: Vec<u128> = vec![0u128; domain_size * 2];
            prg.random_u128s(&mut rs_u128);
            let rs: Vec<Modp> = rs_u128.iter().map(|x| Modp::new(&barrett_ctx, x)).collect();
            
            let z_ast = dot_product_modp(rs, evals_subtracted_case0);
            let z_bullet = dot_product_modp(rs, evals_subtracted_case1);
        }
    }

    fn sketch_rdcf(
        &self,
        incremental_evals: &[Vec<Mod2k>],
        height: usize,
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<(Modp, Modp)>> {

    }

    fn sketch_ldcf_payload(
        &self,
        incremental_evals: &[Vec<Mod2k>],
        height: usize,
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<(Modp, Modp)>> {

    }

    fn sketch_rdcf_payload(
        &self,
        incremental_evals: &[Vec<Mod2k>],
        height: usize,
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Result<Vec<(Modp, Modp)>> {

    }

    fn subtract_shifted_mod2k_to_modp<'a>(&self, a: &[Mod2k], b: &[Mod2k], shift: usize, ctx: &'a BarrettCtx) -> Vec<Modp<'a>> {
        // Only shift left!!!
        let modulus = 1u128 << self.h2;
        let shifted_a = if shift > 0 { // Shift to the left
            [a[..a.len() - shift], vec![Mod2k::zero(modulus); ab_shift]].concat()
        } else { // No shift
            a
        };

        let subtracted = element_wise_subtract_mod2k(a_shifted, b);

        subtracted.iter().map(|x| Modp::new(ctx, x.val())).collect()
    }
}

fn dot_product_modp<'a>(a: &[Modp], b: &[Modp]) -> Modp<'a> {
    assert!(a.len() == b.len());
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn element_wise_product_modp<'a>(a: &[Modp], b: &[Modp]) -> Vec<Modp<'a>> {
    assert!(a.len() == b.len());
    a.iter().zip(b.iter()).map(|(x, y)| x * y).collect();
}

fn element_wise_subtract_modp<'a>(a: &[Modp], b: &[Modp]) -> Vec<Modp<'a>> {
    assert!(a.len() == b.len());
    a.iter().zip(b.iter()).map(|x, y| x - y).collect();
}

fn element_wise_subtract_mod2k(a: &[Mod2k], b: &[Mod2k]) -> Vec<Mod2k> {
    assert!(a.len() == b.len());
    a.iter().zip(b.iter()).map(|x, y| x - y).collect();
}

fn duplicate_vector_mod2k(a: &[Mod2k]) -> Vec<Mod2k> {
    a.iter().flat_map(|x| [x, x]).collect()
}
