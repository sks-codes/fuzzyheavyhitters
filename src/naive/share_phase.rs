use crate::{
    data_structures::ringvec::RingVec,
    fss::dpf::DpfKey,
    fuzzy_match::share_phase_types::{DistanceMetric, ShareConfig, ShareMethod},
    util::u128_to_bits_msb,
};

pub struct SharePhaseNaive {
    share_config: ShareConfig,
}

impl SharePhaseNaive {
    pub fn new(share_config: ShareConfig) -> Self {
        match share_config.method {
            ShareMethod::FSS => {}
            _ => panic!("Unsupported share method for naive share phase"),
        }
        Self { share_config }
    }

    pub fn share_range(
        &self,
        x: &[u128],
        delta: u128,
    ) -> Result<(Vec<DpfKey>, Vec<DpfKey>), anyhow::Error> {
        let points = match self.share_config.metric {
            DistanceMetric::LInfinity => self.create_linfinity_ball(delta, self.share_config.d),
            DistanceMetric::Lp { p } => {
                let distance = delta.pow(p);
                self.create_lp_ball(delta, distance, self.share_config.d, p)
            }
        };
        let mut keys0 = Vec::new();
        let mut keys1 = Vec::new();
        let in_modulus = 1 << self.share_config.h1;
        let out_modulus = 1 << self.share_config.h2;
        let a = RingVec::zero_with_len(1, out_modulus)?;
        let b = RingVec::new(vec![1u128], out_modulus)?;
        for point in points {
            let positive_point = point
                .iter()
                .zip(x.iter())
                .map(|(&p, &xi)| if p + xi < in_modulus { p + xi } else { 0u128 })
                .collect::<Vec<u128>>();
            let positive_point_bits = positive_point
                .iter()
                .map(|&v| u128_to_bits_msb(v, self.share_config.h1))
                .collect::<Vec<Vec<bool>>>();
            let mut positive_point_bits_flat = Vec::new();
            for i in 0..self.share_config.h1 {
                for dimension in 0..self.share_config.d {
                    positive_point_bits_flat.push(positive_point_bits[dimension][i]);
                }
            }
            let (key0, key1) = DpfKey::gen_dpf_key(&positive_point_bits_flat, &a, &b, out_modulus)?;
            keys0.push(key0);
            keys1.push(key1);

            let negative_point = point
                .iter()
                .zip(x.iter())
                .map(|(&p, &xi)| if xi >= p { xi - p } else { 0u128 })
                .collect::<Vec<u128>>();
            if negative_point == positive_point {
                continue;
            }
            let negative_point_bits = negative_point
                .iter()
                .map(|&v| u128_to_bits_msb(v, self.share_config.h1))
                .collect::<Vec<Vec<bool>>>();
            let mut negative_point_bits_flat = Vec::new();
            for i in 0..self.share_config.h1 {
                for dimension in 0..self.share_config.d {
                    negative_point_bits_flat.push(negative_point_bits[dimension][i]);
                }
            }
            let (key0, key1) = DpfKey::gen_dpf_key(&negative_point_bits_flat, &a, &b, out_modulus)?;
            keys0.push(key0);
            keys1.push(key1);
        }
        Ok((keys0, keys1))
    }

    fn create_linfinity_ball(&self, delta: u128, d: usize) -> Vec<Vec<u128>> {
        if d == 1 {
            return (0..=delta).map(|i| vec![i as u128]).collect();
        }
        let recurse_ball = self.create_linfinity_ball(delta, d - 1);
        let mut ball = Vec::new();
        for point in recurse_ball {
            let mut new_point = point.clone();
            new_point.push(0);
            for i in 0..=delta {
                new_point[d - 1] = i as u128;
                ball.push(new_point.clone());
            }
        }
        ball
    }

    fn create_lp_ball(&self, delta: u128, distance: u128, d: usize, p: u32) -> Vec<Vec<u128>> {
        // Note that we already calculated distance = delta^p
        // We do NOT take care of overflow here!
        if d == 1 {
            return (0..=delta).map(|i| vec![i as u128]).collect();
        }
        let dist = (0..=delta)
            .map(|i| (i as u128).pow(p))
            .collect::<Vec<u128>>();
        let recurse_ball = self.create_lp_ball(delta, distance, d - 1, p);
        let mut ball = Vec::new();
        for point in recurse_ball {
            let mut new_point = point.clone();
            let sum_so_far = new_point
                .iter()
                .fold(0u128, |acc, &val| acc + dist[val as usize]);
            new_point.push(0);
            for i in 0..=delta {
                let new_sum = sum_so_far + dist[i as usize];
                if new_sum <= distance {
                    new_point[d - 1] = i as u128;
                    ball.push(new_point.clone());
                } else {
                    break;
                }
            }
        }
        ball
    }
}
