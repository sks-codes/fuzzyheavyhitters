use crate::{
    aes::AES_KEY_SIZE, channel::CommTrackingChannel, data_structures::{
        modp::{BarrettCtx, Modp},
        ringvec::RingVec,
    }, fss::{distance::DistanceFSSKey, interval::IntervalFSSKey}, fuzzy_match::{
        share_phase::SharePhaseError, share_types::{DictionaryType, DistanceMetric, ShareMethod}, shared_range::SharedRange, shared_sketch::SketchData, sketch_helper::SketchHelper
    }, randomness::prg::PRG
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

    pub fn get_sketch_helper(&self) -> SketchHelper {
        unimplemented!()
    }

    fn get_sketch_values_interval_fss_one_dimension(
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

        // Checking whether each level is dcf
        for level in 0..self.h1 {
            let z_ldcf = 
                self.get_sketch_values_dcf(&ldcf_full_evals[level], 1 << level, &dpf_helper[level], prg)
                    .map_err(|e| anyhow!("Failed to get sketch values for LDCF at level {level}: {e}"))?;
            ensure!(z_ldcf.len() == 4, "Sketching LDCF should return 4 values, got {}", z_ldcf.len());
            let z_rdcf = 
                self.get_sketch_values_dcf(&rdcf_full_evals[level], 1 << level, &dpf_helper[level], prg)
                    .map_err(|e| anyhow!("Failed to get sketch values for RDCF at level {level}: {e}"))?;
            ensure!(z_rdcf.len() == 4, "Sketching RDCF should return 4 values, got {}", z_rdcf.len());
        }

        // Checking whether each two consecutive levels are consistent
        for level in 0..(self.h1 - 1) {
            let z_ldcf_inc = self.get_sketch_values_incremental_ldcf_consistency(
                &ldcf_full_evals[level], 
                &ldcf_full_evals[level+1], 
                1 << level, 
                prg)
                .map_err(|e| anyhow!("Failed to get sketch values incremental LDCF consistency at level {level}: {e}"))?;
            let z_rdcf_inc = self.get_sketch_values_incremental_rdcf_consistency(
                &rdcf_full_evals[level], 
                &rdcf_full_evals[level+1], 
                1 << level, 
                prg)
                .map_err(|e| anyhow!("Failed to get sketch values incremental RDCF consistency at level {level}: {e}"))?;
        }

        // Checking whether last level of ldcf is shifted by 2*delta from last level of rdcf
        let z_shift_inc = self.get_sketch_values_shift_dcf_consistency(&ldcf_full_evals[self.h1-1], &rdcf_full_evals[self.h1-1], 2 * delta as usize, domain_size, prg);

        Ok(vec![z_ldcf, z_rdcf, z_ldcf_inc, z_rdcf_inc, z_shift_inc])
    }

    fn get_sketch_values_distance_fss_l1_one_dimension(
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

        
    }

    fn shifted_dcf_with_scale_to_unit_vector(
        &self,
        evals: &[Modp],
        domain_size: usize,
        shift: usize,
        scale0: &[u128],
        scale1: &[u128],
    ) -> Result<Vec<Modp>> {
        ensure!(evals.len() == domain_size, "length mismatch between dcf sketch vector and domain_size");
        ensure!(scale0.len() == domain_size, "length mismatch between scale0 vector and domain_size");
        ensure!(scale1.len() == domain_size, "length mismatch between scale1 vector and domain_size");
        let evals_dpf: Vec<Modp> = (0..(domain_size-1)).map(
            |i| evals[i] - evals[i+1]
        ).collect();
        self.shifted_dpf_with_scale_to_unit_vector(evals_dpf, domain_size, shift, scale0, scale1, prg)
    }

    fn shifted_dpf_with_scale_to_unit_vector(
        &self,
        evals: &[Modp],
        domain_size: usize,
        shift: usize,
        scale0: &[u128],
        scale1: &[u128],
    ) -> Result<Vec<Modp>> {
        let barrett_ctx = BarrettCtx::new(self.q);
        let abs_shift = shift.abs() as usize;
        let shifted_evals = if shift < 0 {
            [&evals[abs_shift..], vec![Modp::zero(&barrett_ctx); abs_shift].as_slice()].concat()
        } else {
            [vec![Modp::zero(&barrett_ctx); abs_shift].as_slice(), &evals[..domain_size-abs_shift]].concat()
        };
        self.dpf_with_scale_to_unit_vector(shifted_evals, domain_size, scale0, scale1, prg)
    }

    fn dpf_with_scale_to_unit_vector(
        &self,
        evals: &[Modp],
        domain_size: usize,
        scale0: &[u128],
        scale1: &[u128],
    ) -> Result<(Vec<Modp>, Vec<Modp>)> {
        let barrett_ctx = BarrettCtx::new(self.q);
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

    fn dpf_to_unit_vector(
        &self,
        evals: &[Modp],
        domain_size: usize,
        dpf_helper: &[u128],
    ) -> Result<(Vec<Modp>, Vec<Modp>)> {
        let barrett_ctx = BarrettCtx::new(self.q);
        let dpf_helper_modp: Vec<Modp> = dpf_helper.iter().map(|x| Modp::new(&barrett_ctx, x)).collect();

        (
            evals.clone(),
            element_wise_product_modp(evals, dpf_helper_modp)
        )
    }

    fn get_sketch_values_shift_dpf_with_scale(
        &self,
        evals: &[RingVec<1>],
        domain_size: usize,
        shift: i32,
        scale0: &[u128],
        scale1: &[u128],
        prg: &mut PRG
    ) -> Result<Vec<Modp>> {
        ensure!(evals.len() == domain_size, "length mismatch between dpf sketch vector and domain_size");
        ensure!(scale0.len() == domain_size, "length mismatch between scale0 vector and domain_size");
        ensure!(scale1.len() == domain_size, "length mismatch between scale1 vector and domain_size");
        // Shift the dpf vector correspondingly
        let barrett_ctx = BarrettCtx::new(self.q);
        let evals_modp: Vec<Modp> = evals.iter().map(|eval| {
            Modp::new(&barrett_ctx, eval[0])
        }).collect();
        let abs_shift = shift.abs() as usize;
        let shifted_evals_modp = if shift < 0 {
            [&evals_modp[abs_shift..], vec![Modp::zero(&barrett_ctx); abs_shift].as_slice()].concat()
        } else {
            [vec![Modp::zero(&barrett_ctx); abs_shift].as_slice(), &evals_modp[..domain_size-abs_shift]].concat()
        };

        let scaled_shifted_evals_modp = 
    }

    fn get_sketch_values_dcf(
        &self,
        evals: &[RingVec<1>],
        domain_size: usize,
        dpf_helper: &[u128],
        prg: &mut PRG,
    ) -> Result<Vec<Modp>> {
        ensure!(evals.len() == domain_size, "length mismatch between dcf sketch vector and domain_size");
        ensure!(dpf_helper.len() == domain_size, "length mismatch between dpf_helper and domain_size");
        // Transform to DPF
        let evals_dpf: Vec<RingVec<1>> = (0..(domain_size-1)).map(
            |i| evals[i] - evals[i+1]
        ).collect();
        let modulus = 1u128 << self.h2;
        evals_dpf.push(RingVec::<1>::zero(modulus));
        self.get_sketch_values_dpf(evals_dpf, domain_size, dpf_helper, prg)
    }

    fn get_sketch_values_dpf(
        &self,
        evals: &[RingVec<1>],
        domain_size: usize,
        dpf_helper: &[u128],
        prg: &mut PRG,
    ) -> Result<Vec<Modp>> {
        ensure!(evals.len() == domain_size, "length mismatch between dpf sketch vector and domain_size");
        ensure!(dpf_helper.len() == domain_size, "length mismatch between dpf_helper and domain_size");
        // Prepare transforming to Zq
        let barrett_ctx = BarrettCtx::new(self.q);
        let case_1_evals: Vec<Modp> = evals.iter().map(|eval| {
            Modp::new(&barrett_ctx, eval[0]);
        }).collect();
        let dpf_helper_modp: Vec<Modp> = dpf_helper.iter().map(|x| Modp::new(&barrett_ctx, x)).collect();
        let case_2_evals = case_1_evals.iter().zip(dpf_helper_modp.iter()).map(|(x, y)| {
            x * y
        }).collect();

        // Generate random r0, r1, ...
        let mut rs_u128: Vec<u128> = vec![0u128; domain_size];
        prg.random_u128s(&mut rs_u128);
        let rs: Vec<Modp> = rs_u128.iter().map(|x| Modp::new(&barrett_ctx, x)).collect();

        // Sketch values for the first case
        let z_ast = dot_product_modp(&case_1_evals, &rs); // r0 * case_1[0] + r1 * case_1[1] + ...
        let rs2: Vec<Modp> = element_wise_product_modp(&rs, &rs); // r0^2, r1^2, ... 
        let z_ast_2 = dot_product_modp(&case_1_evals, &rs2); // r0^2 * case_1[0] + r1^2 * case_1[1] + ...

        // Sketch values for the second case
        let rs3: Vec<Modp> = element_wise_product_modp(&rs, &rs2); // r0^3, r1^3, ...
        let z_bullet = dot_product_modp(&case_2_evals, &rs3); // r0^3 * case_2[0] + r1^3 * case_2[1] + ...
        let rs6: Vec<Modp> = element_wise_product_modp(&rs3, &rs3); // r0^6, r1^6, ...
        let z_bullet_2 = dot_product_modp(&case_2_evals, &rs6); // r0^6 * case_2[0] + r1^6 * case_2[1] + ...

        Ok(vec!([z_ast, z_ast_2, z_bullet, z_bullet_2]))
    }

    fn get_sketch_values_incremental_ldcf_consistency(
        &self,
        evals0: &[RingVec<1>],
        evals1: &[RingVec<1>],
        domain_size: usize,
        prg: &mut PRG,
    ) -> Vec<Modp> {
        ensure!(evals0.len() == domain_size, "Length mismatch in incremental ldcf consistency, length evals0 = {}", evals0.len());
        ensure!(evals1.len() == domain_size * 2, "Length mismatch in increment LDCF consistency");

        // Prepare duplicating vector
        let barrett_ctx = BarrettCtx::new(self.q);
        let duplicated_evals: Vec<Modp> = evals0.iter().flat_map(|eval| 
            [Modp::new(&barrett_ctx, eval[0]), Modp::new(&barrett_ctx, eval[0])]
        ).collect();
        let case1_evals: Vec<Modp> = evals1.iter().map(|eval|
            Modp::new(&barrett_ctx, eval[0])
        ).collect();
        let case2_evals: Vec<Modp> = case1_evals[1..].to_vec().extend([Modp::zero(&barrett_ctx)]);

        // Generate random r0, r1, ...
        let mut rs_u128: Vec<u128> = vec![0u128; domain_size * 2];
        prg.random_u128s(&mut rs_u128);
        let rs: Vec<Modp> = rs_u128.iter().map(|x| Modp::new(&barrett_ctx, x)).collect();

        // Sketch value for the first case
        let case1_dif = element_wise_subtract_modp(case1_evals, &duplicated_evals);
        let z_ast = dot_product_modp(case1_dif, rs);

        // Sketch value for the second case
        let case2_dif = element_wise_subtract_modp(case2_evals, &duplicated_evals);
        let z_bullet= dot_product_modp(case2_dif, rs);

        Ok(vec!([z_ast, z_bullet]))
    }

    fn get_sketch_values_incremental_rdcf_consistency(
        &self,
        evals0: &[RingVec<1>],
        evals1: &[RingVec<1>],
        domain_size: usize,
        prg: &mut PRG,
    ) -> (Modp, Modp) {
        ensure!(evals0.len() == domain_size, "Length mismatch in incremental ldcf consistency, length evals0 = {}", evals0.len());
        ensure!(evals1.len() == domain_size * 2, "Length mismatch in incremental ldcf consistency, length evals1 = {}", evals1.len());

        // Prepare duplicating vector
        let barrett_ctx = BarrettCtx::new(self.q);
        let case1_evals: Vec<Modp> = evals0.iter().flat_map(|eval| 
            [Modp::new(&barrett_ctx, eval[0]), Modp::new(&barrett_ctx, eval[0])]
        ).collect();
        let case2_evals: Vec<Modp> = case1_evals[1..].to_vec().extend([Modp::zero(&barrett_ctx)]);

        let other_evals: Vec<Modp> = evals1.iter().map(|eval| 
            Modp::new(&barrett_ctx, eval[0])
        ).collect();

        // Generate random r0, r1, ...
        let mut rs_u128: Vec<u128> = vec![0u128; domain_size * 2];
        prg.random_u128s(&mut rs_u128);
        let rs: Vec<Modp> = rs_u128.iter().map(|x| Modp::new(&barrett_ctx, x)).collect();

        // Sketch value for the first case
        let case1_dif = element_wise_subtract_modp(case1_evals, &other_evals);
        let z_ast = dot_product_modp(case1_dif, rs);

        // Sketch value for the second case
        let case2_dif = element_wise_subtract_modp(case2_evals, &other_evals);
        let z_bullet= dot_product_modp(case2_dif, rs);

        (z_ast, z_bullet)
    }

    fn get_sketch_values_shift_dcf_consistency(
        &self, 
        evals0: &[RingVec<1>],
        evals1: &[RingVec<1>],
        shift: usize,
        domain_size: usize,
        prg: &mut PRG,
    ) -> Modp {
        ensure!(evals0.len() == domain_size, "Length mismatch in incremental rdcf consistency, length evals0 = {}", evals0.len());
        ensure!(evals1.len() == domain_size * 2, "Length mismatch in incremental rdcf consistency, length evals1 = {}", evals1.len());

        let barrett_ctx = BarrettCtx::new(self.q);
        // Shift evals1 to the left by shift_amount
        let evals0_modp: Vec<Modp> = evals0.iter().map(|eval| 
            Modp::new(&barrett_ctx, eval[0])
        ).collect();
        let evals1_modp: Vec<Modp> = evals1.iter().map(|eval| 
            Modp::new(&barrett_ctx, eval[0])
        ).collect();
        let evals1_modp_shifted = evals1_modp[shift..].to_vec().extend(vec![Modp::zero(&barrett_ctx); shift]);

        // Generate random r0, r1, ...
        let mut rs_u128: Vec<u128> = vec![0u128; domain_size];
        prg.random_u128s(&mut rs_u128);
        let rs: Vec<Modp> = rs_u128.iter().map(|x| Modp::new(&barrett_ctx, x)).collect();

        // Sketch value is simply random linear combination
        let dif = element_wise_subtract_modp(evals0_modp, evals1_modp_shifted);
        let z = dot_product_modp(rs, dif);

        z    
    }

    fn sketch_distance_fss<const N: usize>(
        &self,
        _shared_ranges: &[SharedRange],
    ) -> Result<bool, SharePhaseError> {
        unimplemented!()
    }

    fn difference(
        &self,
        evals: &[u128],
        target_length: usize,
    ) -> Vec<u128> {

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