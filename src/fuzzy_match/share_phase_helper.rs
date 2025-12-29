use blake3;

use std::cmp::{max, min};

use crate::{
    data_structures::ringvec::RingVec,
    fss::{
        distance::{DistanceFSSEval, DistanceFSSKey},
        interval::{IntervalFSSEval, IntervalFSSKey},
    },
    fuzzy_match::{
        share_okvs_strategies::KeyValuePairStrategy,
        shared_range::{ShareData, SharedRange},
    },
    okvs_f2k::RbOkvsF2k,
    util::u128_to_bits_msb,
};

use super::share_phase::{SharePhase, SharePhaseError};
use super::share_types::DistanceMetric;

impl SharePhase {
    /// Generic OKVS sharing method that uses different strategies for key-value pair preparation
    pub(super) fn share_with_okvs<T: KeyValuePairStrategy>(
        &self,
        x: &[u128], // The original d-dimensional vector
        left_bound: &Vec<u128>,
        right_bound: &Vec<u128>,
        r1: &[[u8; 16]],
        r2: &[[u8; 16]],
        strategy: T,
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut okvs_shares_0 = Vec::new();
        let mut okvs_shares_1 = Vec::new();

        // Create separate OKVS for each dimension
        for dim in 0..self.config.d {
            let left = left_bound[dim];
            let right = right_bound[dim];
            let x_i = x[dim];

            // Use the strategy to prepare key-value pairs
            let (keys, values_0, values_1) = strategy.prepare_key_value_pairs(
                dim,
                left,
                right,
                x_i,
                self.config.h1,
                self.config.h2,
            );

            if keys.is_empty() {
                // If no keys for this dimension, create empty OKVS
                okvs_shares_0.push(Vec::new());
                okvs_shares_1.push(Vec::new());
                continue;
            }

            let columns = max((keys.len() as f64 * 1.1) as usize, 60);
            let band_width = min(columns, 100);
            let okvs = RbOkvsF2k::<u128>::new(keys.len(), columns, band_width, &r1[dim], &r2[dim]);

            let encoding_0 = okvs.encode(&keys, &values_0)?;
            let encoding_1 = okvs.encode(&keys, &values_1)?;

            okvs_shares_0.push(encoding_0);
            okvs_shares_1.push(encoding_1);
        }

        Ok((
            SharedRange::OKVS {
                okvs_shares: okvs_shares_0,
                okvs_seeds: (0..self.config.d).map(|dim| (r1[dim], r2[dim])).collect(),
                role: false, // Server 0
                p: match &self.config.metric {
                    DistanceMetric::LInfinity => None,
                    DistanceMetric::Lp { p } => Some(*p),
                },
            },
            SharedRange::OKVS {
                okvs_shares: okvs_shares_1,
                okvs_seeds: (0..self.config.d).map(|dim| (r1[dim], r2[dim])).collect(),
                role: true, // Server 1
                p: match &self.config.metric {
                    DistanceMetric::LInfinity => None,
                    DistanceMetric::Lp { p } => Some(*p),
                },
            },
        ))
    }

    /// Generic OKVS evaluation method for both L-infinity and Lp distance cases
    pub(super) fn evaluate_okvs_generic(
        &self,
        okvs_share: &Vec<u128>,
        point_bits: &[bool],
        role: bool,
        r1: &[u8; 16],
        r2: &[u8; 16],
    ) -> Result<u128, SharePhaseError> {
        let key_bits = point_bits.to_vec();
        let modulus_mask = (1u128 << self.config.h2) - 1;
        let columns = okvs_share.len();
        let band_width = min(columns, 100);

        let okvs = RbOkvsF2k::<u128>::new(1, columns, band_width, r1, r2);

        let result = okvs.decode(&okvs_share, &[key_bits.clone()]);
        if result.is_empty() {
            Ok(0) // Return 0 if decode fails
        } else {
            if !role {
                // Server 0: return the share directly
                Ok(result[0])
            } else {
                // Server 1: need to add the deterministic mask
                let key_bits_bytes: Vec<u8> = key_bits
                    .iter()
                    .map(|&b| if b { 1u8 } else { 0u8 })
                    .collect();
                let hash = blake3::hash(&key_bits_bytes);
                let hash_bytes = hash.as_bytes();
                let value_mask = u128::from_le_bytes([
                    hash_bytes[0],
                    hash_bytes[1],
                    hash_bytes[2],
                    hash_bytes[3],
                    hash_bytes[4],
                    hash_bytes[5],
                    hash_bytes[6],
                    hash_bytes[7],
                    hash_bytes[8],
                    hash_bytes[9],
                    hash_bytes[10],
                    hash_bytes[11],
                    hash_bytes[12],
                    hash_bytes[13],
                    hash_bytes[14],
                    hash_bytes[15],
                ]) & modulus_mask;
                Ok((result[0] + modulus_mask + 1 - value_mask) & modulus_mask)
            }
        }
    }

    /// Share using Interval FSS method (N=1)
    pub(super) fn share_with_interval_fss(
        &self,
        left_bound: &[u128],
        right_bound: &[u128],
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut keys_0 = Vec::new();
        let mut keys_1 = Vec::new();

        let modulus = 1u128 << self.config.h2;

        let left_payload = RingVec::new(vec![1], modulus)
            .map_err(|e| SharePhaseError::InvalidRange(e.to_string()))?;
        let mid_payload = RingVec::new(vec![0], modulus)
            .map_err(|e| SharePhaseError::InvalidRange(e.to_string()))?;
        let right_payload = RingVec::new(vec![1], modulus)
            .map_err(|e| SharePhaseError::InvalidRange(e.to_string()))?;

        for (&alpha, &beta) in left_bound.iter().zip(right_bound.iter()) {
            if alpha > beta {
                return Err(SharePhaseError::InvalidRange(format!(
                    "Left bound {} cannot be greater than right bound {}",
                    alpha, beta
                )));
            }

            // Convert bounds to bits representation
            let alpha_bits = u128_to_bits_msb(alpha, self.config.h1);
            let beta_bits = u128_to_bits_msb(beta, self.config.h1);

            let (fss_key_0, fss_key_1) = IntervalFSSKey::<1>::gen_interval_fss_key(
                &alpha_bits,
                &beta_bits,
                &left_payload,
                &mid_payload,
                &right_payload,
                modulus,
            )
            .map_err(|e| SharePhaseError::IntervalFSSError(e.to_string()))?;

            keys_0.push(fss_key_0);
            keys_1.push(fss_key_1);
        }

        Ok((
            SharedRange::IntervalFSS {
                keys: keys_0,
                role: false, // Server 0
            },
            SharedRange::IntervalFSS {
                keys: keys_1,
                role: true, // Server 1
            },
        ))
    }

    pub(super) fn evaluate_interval_fss_at_single_dimension(
        &self,
        fss_key: &IntervalFSSKey<1>,
        point_bits: &[bool],
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.h2;
        let result = fss_key
            .eval_interval_fss(point_bits, modulus)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        Ok(result[0])
    }

    /// Share using Distance FSS method for L1 distance (p=1, N=2)
    pub(super) fn share_with_distance_fss_l1(
        &self,
        x: &[u128], // The original d-dimensional vector
        left_bound: &[u128],
        right_bound: &[u128],
        max_distance: u128,
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut keys_0 = Vec::new();
        let mut keys_1 = Vec::new();

        let modulus = 1u128 << self.config.h2;

        for ((&alpha, &beta), &center) in left_bound.iter().zip(right_bound.iter()).zip(x.iter()) {
            if alpha > beta || alpha > center || beta < center {
                return Err(SharePhaseError::InvalidRange(format!(
                    "Left bound {} cannot be greater than right bound {}",
                    alpha, beta
                )));
            }

            // Convert bounds to bits representation
            let alpha_bits = u128_to_bits_msb(alpha, self.config.h1);
            let beta_bits = u128_to_bits_msb(beta, self.config.h1);
            let center_bits = u128_to_bits_msb(center, self.config.h1);

            // Create Distance FSS keys for both servers
            let (fss_key_0, fss_key_1) = DistanceFSSKey::<2>::gen_distance_fss_key(
                center,
                &center_bits,
                &alpha_bits,
                &beta_bits,
                max_distance,
                modulus,
            )
            .map_err(|e| SharePhaseError::IntervalFSSError(e.to_string()))?;

            keys_0.push(fss_key_0);
            keys_1.push(fss_key_1);
        }

        Ok((
            SharedRange::DistanceFSSL1 {
                keys: keys_0,
                role: false,
            },
            SharedRange::DistanceFSSL1 {
                keys: keys_1,
                role: true,
            },
        ))
    }

    /// Share using Distance FSS method for L2 distance (p=2, N=3)
    pub(super) fn share_with_distance_fss_l2(
        &self,
        x: &[u128], // The original d-dimensional vector
        left_bound: &[u128],
        right_bound: &[u128],
        max_distance: u128,
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut keys_0 = Vec::new();
        let mut keys_1 = Vec::new();

        let modulus = 1u128 << self.config.h2;

        for ((&alpha, &beta), &center) in left_bound.iter().zip(right_bound.iter()).zip(x.iter()) {
            if alpha > beta || alpha > center || beta < center {
                return Err(SharePhaseError::InvalidRange(format!(
                    "Left bound {} cannot be greater than right bound {}",
                    alpha, beta
                )));
            }

            // Convert bounds to bits representation
            let alpha_bits = u128_to_bits_msb(alpha, self.config.h1);
            let beta_bits = u128_to_bits_msb(beta, self.config.h1);
            let center_bits = u128_to_bits_msb(center, self.config.h1);

            // Create Distance FSS keys for both servers
            let (fss_key_0, fss_key_1) = DistanceFSSKey::<3>::gen_distance_fss_key(
                center,
                &center_bits,
                &alpha_bits,
                &beta_bits,
                max_distance,
                modulus,
            )
            .map_err(|e| SharePhaseError::IntervalFSSError(e.to_string()))?;

            keys_0.push(fss_key_0);
            keys_1.push(fss_key_1);
        }

        Ok((
            SharedRange::DistanceFSSL2 {
                keys: keys_0,
                role: false,
            },
            SharedRange::DistanceFSSL2 {
                keys: keys_1,
                role: true,
            },
        ))
    }

    /// Share using Distance FSS method for L3 distance (p=3, N=4)
    pub(super) fn share_with_distance_fss_l3(
        &self,
        x: &[u128], // The original d-dimensional vector
        left_bound: &[u128],
        right_bound: &[u128],
        max_distance: u128,
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut keys_0 = Vec::new();
        let mut keys_1 = Vec::new();

        let modulus = 1u128 << self.config.h2;

        for ((&alpha, &beta), &center) in left_bound.iter().zip(right_bound.iter()).zip(x.iter()) {
            if alpha > beta || alpha > center || beta < center {
                return Err(SharePhaseError::InvalidRange(format!(
                    "Left bound {} cannot be greater than right bound {}",
                    alpha, beta
                )));
            }

            // Convert bounds to bits representation
            let alpha_bits = u128_to_bits_msb(alpha, self.config.h1);
            let beta_bits = u128_to_bits_msb(beta, self.config.h1);
            let center_bits = u128_to_bits_msb(center, self.config.h1);

            // Create Distance FSS keys for both servers
            let (fss_key_0, fss_key_1) = DistanceFSSKey::<4>::gen_distance_fss_key(
                center,
                &center_bits,
                &alpha_bits,
                &beta_bits,
                max_distance,
                modulus,
            )
            .map_err(|e| SharePhaseError::IntervalFSSError(e.to_string()))?;

            keys_0.push(fss_key_0);
            keys_1.push(fss_key_1);
        }

        Ok((
            SharedRange::DistanceFSSL3 {
                keys: keys_0,
                role: false,
            },
            SharedRange::DistanceFSSL3 {
                keys: keys_1,
                role: true,
            },
        ))
    }

    /// Generic method for evaluating Distance FSS
    pub(super) fn evaluate_distance_fss<const N: usize>(
        &self,
        fss_key: &DistanceFSSKey<N>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.h2;
        let result = fss_key
            .eval_distance_fss(point_bits, self.config.h1, modulus)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;
        if !role {
            Ok(result)
        } else {
            Ok((modulus - result) % modulus)
        }
    }

    pub(super) fn expand_prefix_okvs(
        &self,
        okvs_shares: &Vec<Vec<u128>>,
        okvs_seeds: &Vec<([u8; 16], [u8; 16])>,
        role: bool,
        prefix: &[bool],
        eval: &[u128],
        dimension: usize,
    ) -> Result<(ShareData, ShareData), SharePhaseError> {
        let mut new_prefix = prefix.to_vec();
        new_prefix.push(false);
        let mut eval0 = eval.to_vec();
        eval0[dimension] = self.evaluate_okvs_generic(
            &okvs_shares[dimension],
            &new_prefix,
            role,
            &okvs_seeds[dimension].0,
            &okvs_seeds[dimension].1,
        )?;
        new_prefix.pop();
        new_prefix.push(true);
        let mut eval1 = eval.to_vec();
        eval1[dimension] = self.evaluate_okvs_generic(
            &okvs_shares[dimension],
            &new_prefix,
            role,
            &okvs_seeds[dimension].0,
            &okvs_seeds[dimension].1,
        )?;

        Ok((
            ShareData::OKVS { eval: eval0 },
            ShareData::OKVS { eval: eval1 },
        ))
    }

    pub(super) fn expand_prefix_interval_fss(
        &self,
        keys: &Vec<IntervalFSSKey<1>>,
        data: &[IntervalFSSEval<1>],
        dimension: usize,
    ) -> Result<(ShareData, ShareData), SharePhaseError> {
        let modulus = 1u128 << self.config.h2;
        let interval_fss_key = &keys[dimension];
        let interval_fss_data = &data[dimension];

        let mut data0 = data.to_vec();
        let mut data1 = data.to_vec();
        (data0[dimension], data1[dimension]) =
            interval_fss_key
                .expand_prefix(interval_fss_data, modulus)
                .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;

        Ok((
            ShareData::IntervalFSS { data: data0 },
            ShareData::IntervalFSS { data: data1 },
        ))
    }

    pub(super) fn expand_prefix_distance_fss<const N: usize>(
        &self,
        keys: &Vec<DistanceFSSKey<N>>,
        prefix: &[bool],
        data: &[DistanceFSSEval<N>],
        eval: &[u128],
        dimension: usize,
        role: bool,
    ) -> Result<(ShareData, ShareData), SharePhaseError> {
        let modulus = 1u128 << self.config.h2;
        let input_len = self.config.h1;
        let key = keys[dimension].clone();

        let mut data0 = data.to_vec();
        let mut data1 = data.to_vec();
        (data0[dimension], data1[dimension]) = key
            .expand_prefix(prefix, &data[dimension], input_len, modulus)
            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))?;

        let mut eval0 = eval.to_vec();
        eval0[dimension] = if !role {
            data0[dimension].result
        } else {
            (modulus - data0[dimension].result) % modulus
        };

        let mut eval1 = eval.to_vec();
        eval1[dimension] = if !role {
            data1[dimension].result
        } else {
            (modulus - data1[dimension].result) % modulus
        };

        match N {
            2 => Ok((
                ShareData::DistanceFSSL1 {
                    data: unsafe { std::mem::transmute(data0) },
                    eval: eval0,
                },
                ShareData::DistanceFSSL1 {
                    data: unsafe { std::mem::transmute(data1) },
                    eval: eval1,
                },
            )),
            3 => Ok((
                ShareData::DistanceFSSL2 {
                    data: unsafe { std::mem::transmute(data0) },
                    eval: eval0,
                },
                ShareData::DistanceFSSL2 {
                    data: unsafe { std::mem::transmute(data1) },
                    eval: eval1,
                },
            )),
            4 => Ok((
                ShareData::DistanceFSSL3 {
                    data: unsafe { std::mem::transmute(data0) },
                    eval: eval0,
                },
                ShareData::DistanceFSSL3 {
                    data: unsafe { std::mem::transmute(data1) },
                    eval: eval1,
                },
            )),
            _ => Err(SharePhaseError::EvaluationError(
                "Invalid share data type for distance FSS expansion".to_string(),
            )),
        }
    }
}
