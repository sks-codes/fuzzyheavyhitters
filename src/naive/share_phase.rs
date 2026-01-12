use crate::{
    data_structures::ringvec::RingVec,
    fss::dpf::DpfKey,
    fuzzy_match::share_phase_types::{DistanceMetric, ShareConfig, ShareMethod},
    util::u128_to_bits_msb,
};
use anyhow::ensure;

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
        ensure!(
            x.len() == self.share_config.d,
            "Input x must match the configured dimension"
        );
        let max_input = (1u128 << self.share_config.h1) - 1;
        for &xi in x {
            ensure!(
                xi <= max_input,
                "Input x ({}) exceeds maximum value for {}-bit input ({})",
                xi,
                self.share_config.h1,
                max_input
            );
        }

        let offsets = match self.share_config.metric {
            DistanceMetric::LInfinity => self.create_linfinity_ball(delta, self.share_config.d),
            DistanceMetric::Lp { p } => {
                let distance = delta.pow(p);
                self.create_lp_ball(delta, distance, self.share_config.d, p)
            }
        };
        let mut keys0 = Vec::new();
        let mut keys1 = Vec::new();
        let out_modulus = 1u128 << self.share_config.h2;
        let a = RingVec::new(vec![1u128], out_modulus)?;
        let b = RingVec::zero_with_len(1, out_modulus)?;
        for offset in offsets {
            let mut point = Vec::with_capacity(self.share_config.d);
            self.expand_offset_points(
                x,
                &offset,
                0,
                &mut point,
                &a,
                &b,
                out_modulus,
                &mut keys0,
                &mut keys1,
            )?;
        }
        Ok((keys0, keys1))
    }

    fn expand_offset_points(
        &self,
        x: &[u128],
        offset: &[u128],
        dim: usize,
        current_point: &mut Vec<u128>,
        a: &RingVec,
        b: &RingVec,
        out_modulus: u128,
        keys0: &mut Vec<DpfKey>,
        keys1: &mut Vec<DpfKey>,
    ) -> Result<(), anyhow::Error> {
        if dim == offset.len() {
            return self.push_dpf_keys_for_point(current_point, a, b, out_modulus, keys0, keys1);
        }

        let xi = x[dim];
        let p = offset[dim];
        if p == 0 {
            current_point.push(xi);
            let result = self.expand_offset_points(
                x,
                offset,
                dim + 1,
                current_point,
                a,
                b,
                out_modulus,
                keys0,
                keys1,
            );
            current_point.pop();
            return result;
        }

        let max_input = (1u128 << self.share_config.h1) - 1;
        let upper = xi.saturating_add(p).min(max_input);
        let lower = xi.saturating_sub(p);

        // Add the plus point
        current_point.push(upper);
        self.expand_offset_points(
            x,
            offset,
            dim + 1,
            current_point,
            a,
            b,
            out_modulus,
            keys0,
            keys1,
        )?;
        current_point.pop();

        // Add the minus point
        current_point.push(lower);
        self.expand_offset_points(
            x,
            offset,
            dim + 1,
            current_point,
            a,
            b,
            out_modulus,
            keys0,
            keys1,
        )?;
        current_point.pop();
        Ok(())
    }

    fn push_dpf_keys_for_point(
        &self,
        point: &[u128],
        a: &RingVec,
        b: &RingVec,
        out_modulus: u128,
        keys0: &mut Vec<DpfKey>,
        keys1: &mut Vec<DpfKey>,
    ) -> Result<(), anyhow::Error> {
        let point_bits = point
            .iter()
            .map(|&v| u128_to_bits_msb(v, self.share_config.h1))
            .collect::<Vec<Vec<bool>>>();
        let mut point_bits_flat = Vec::with_capacity(self.share_config.h1 * self.share_config.d);
        for i in 0..self.share_config.h1 {
            for dimension in 0..self.share_config.d {
                point_bits_flat.push(point_bits[dimension][i]);
            }
        }
        let (key0, key1) = DpfKey::gen_dpf_key(&point_bits_flat, a, b, out_modulus)?;
        keys0.push(key0);
        keys1.push(key1);
        Ok(())
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
