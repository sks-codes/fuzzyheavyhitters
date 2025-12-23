
use crate::{
    aes::AES_KEY_SIZE, 
    data_structures::{
        modp::{BarrettCtx, Modp},
        ringvec::RingVec,
    },
    fuzzy_match::{
        share_types::{DistanceMetric, ShareMethod, DictionaryType}, 
        shared_range::SharedRange, shared_sketch::SketchData, sketch_helper::SketchHelper,
        share_phase::SharePhaseError,
    }, randomness::prg::PRG,
    fss::interval::IntervalFSSKey,
    channel::CommTrackingChannel,
};


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
    pub fn sketch(
        &self,
        shared_ranges: &[SharedRange],
        sketch_data: &[SketchData],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<bool, SharePhaseError> {
        // Check the type of all shared ranges
        let first_type = match &shared_ranges[0] {
            SharedRange::OKVS { .. } => "OKVS",
            SharedRange::IntervalFSS { .. } => "IntervalFSS",
            SharedRange::DistanceFSSL1 { .. } => "DistanceFSSL1",
            SharedRange::DistanceFSSL2 { .. } => "DistanceFSSL2",
            SharedRange::DistanceFSSL3 { .. } => "DistanceFSSL3",
        };
        for sr in shared_ranges.iter() {
            let sr_type = match sr {
                SharedRange::OKVS { .. } => "OKVS",
                SharedRange::IntervalFSS { .. } => "IntervalFSS",
                SharedRange::DistanceFSSL1 { .. } => "DistanceFSSL1",
                SharedRange::DistanceFSSL2 { .. } => "DistanceFSSL2",
                SharedRange::DistanceFSSL3 { .. } => "DistanceFSSL3",
            };
            if sr_type != first_type {
                return Err(SharePhaseError::InvalidRange(
                    "All shared ranges must be of the same type for sketching".to_string()
                ));
            }
        }

        let role = match &shared_ranges[0] {
            SharedRange::OKVS { role, .. } => *role,
            SharedRange::IntervalFSS { role, .. } => *role,
            SharedRange::DistanceFSSL1 { role, .. } => *role,
            SharedRange::DistanceFSSL2 { role, .. } => *role,
            SharedRange::DistanceFSSL3 { role, .. } => *role,
        };

        let mut seed = [0u8; AES_KEY_SIZE];
        if role {
            seed = rand::rng().random::<[u8; AES_KEY_SIZE]>();
            other_server_channels[0].write_bytes(&seed).expect("Failed to send seed to other server");
        } else {
            other_server_channels[0].read_bytes(&mut seed).expect("Failed to receive seed from other server");
        }

        // Now sketch based on the type
        match first_type {
            "OKVS" => {
                println!("Sketching for OKVS not implemented yet, so we skip it.");
                Ok(true)
            },
            "IntervalFSS" => self.sketch_interval_fss(shared_ranges, sketch_data, seed, other_server_channels),
            "DistanceFSSL1" => self.sketch_distance_fss::<2>(shared_ranges),
            "DistanceFSSL2" => self.sketch_distance_fss::<3>(shared_ranges),
            "DistanceFSSL3" => self.sketch_distance_fss::<4>(shared_ranges),
            _ => Err(SharePhaseError::InvalidRange(
                "Unknown shared range type for sketching".to_string()
            )),
        }
    }

    fn sketch_interval_fss(
        &self, 
        shared_ranges: &[SharedRange],
        sketch_data: &[SketchData],
        seed: [u8; AES_KEY_SIZE],
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<bool, SharePhaseError> {

    }

    pub fn get_sketch_helper(&self) -> SketchHelper {

    }

    fn get_sketch_values_interval_fss_one_dimension(
        &self,
        key: IntervalFSSKey<1>,
        role: bool,
        sketch_helper: SketchHelper,
        prg: &mut PRG,
    ) -> Vec<Modp> {
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
            let z = self.get_sketch_values_dcf(&ldcf_full_evals[level], 1 << level, sketch_helper.dpf()[level], prg);
            let z = self.get_sketch_values_dcf(&rdcf_full_evals[level], 1 << level, sketch_helper.dpf()[level], prg);
        }

        // Checking whether each two consecutive levels are consistent
        for level in 0..(self.h1 - 1) {
            let z = self.get_sketch_values_incremental_ldcf_consistency(&ldcf_full_evals[level], ldcf_full_evals[level+1], 1 << level, prg);
            let z = self.get_sketch_values_incremental_rdcf_consistency(&rdcf_full_evals[level], rdcf_full_evals[level+1], 1 << level, prg);
        }

        // Checking whether last level of ldcf is shifted by 2*delta from last level of rdcf
        let z = self.get_sketch_values_shift_dcf_consistency(&ldcf_full_evals[self.h1-1], &rdcf_full_evals[self.h1-1], 2 * delta as usize, domain_size, prg);
    }

    fn get_sketch_values_dcf(
        &self,
        evals: &[RingVec<1>],
        domain_size: usize,
        dpf_helper: &[Modp],
        prg: &mut PRG,
    ) -> Vec<Modp> {
        assert!(evals.len() == domain_size);
        assert!(dpf_helper.len() == domain_size);
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
        dpf_helper: &[Modp],
        prg: &mut PRG,
    ) -> (Modp, Modp, Modp, Modp) {
        assert!(evals.len() == domain_size);
        assert!(dpf_helper.len() == domain_size);
        // Prepare transforming to Zq
        let barrett_ctx = BarrettCtx::new(self.q);
        let case_1_evals: Vec<Modp> = evals.iter().map(|eval| {
            Modp::new(&barrett_ctx, eval[0]);
        });
        let case_2_evals = case_1_evals.iter().zip(dpf_helper.iter()).map(|(x, y)| {
            x * y
        });

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

        (z_ast, z_ast_2, z_bullet, z_bullet_2)
    }

    fn get_sketch_values_incremental_ldcf_consistency(
        &self,
        evals0: &[RingVec<1>],
        evals1: &[RingVec<1>],
        domain_size: usize,
        prg: &mut PRG,
    ) -> (Modp, Modp) {
        assert!(evals0.len() == domain_size);
        assert!(evals1.len() * 2 == domain_size);

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

        (z_ast, z_bullet)
    }

    fn get_sketch_values_incremental_rdcf_consistency(
        &self,
        evals0: &[RingVec<1>],
        evals1: &[RingVec<1>],
        domain_size: usize,
        prg: &mut PRG,
    ) -> (Modp, Modp) {
        assert!(evals0.len() == domain_size);
        assert!(evals1.len() * 2 == domain_size);

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
        assert!(evals0.len() == domain_size);
        assert!(evals1.len() == domain_size);

        // Shift evals1 to the left by shift_amount
        let evals0_modp: Vec<Modp> = evals0.iter().map(|x| 
            Modp::new(&barrett_ctx, eval[0])
        ).collect();
        let evals1_modp: Vec<Modp> = evals1.iter().map(|x| 
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