use blake3;
use rand::Rng;

use crate::{
    aes::AES_KEY_SIZE,
    channel::CommTrackingChannel,
    randomness::prg::PRG,
    okvs_f2k::RbOkvsF2k,
    fss::{
        distance::{DistanceFSSKey, DistanceFSSEval},
        interval::{IntervalFSSKey, IntervalFSSEval},
    },
    data_structures::{
        ringvec::RingVec,
        modp::mul_mod,
    },
    util::u128_to_bits_msb,
    fuzzy_match::{
        shared_range::{SharedRange, ShareData},
        shared_sketch::SketchData,
        share_okvs_strategies::{
            KeyValuePairStrategy, 
            KnownLInfinityStrategy, 
            UnknownLInfinityStrategy, 
            KnownLpStrategy, 
            UnknownLpStrategy
        },
    },
    configs::cli_config::ProtocolParameters,
};

use std::{cmp::{max, min}, convert::TryInto};
use scuttlebutt::AbstractChannel;
use rayon::prelude::*;
use rayon::ThreadPool;

/// Share phase handler
#[derive(Clone)]
pub struct SharePhase {
    config: ShareConfig,
}

impl SharePhase {
    /// Create a new share phase with the given configuration
    pub fn new<C: Into<ShareConfig>>(config: C) -> Self {
        Self {
            config: config.into(),
        }
    }

    /// Share a range [x-delta, x+delta] using the configured method
    /// Input x is a d-dimensional vector, output will be 2 SharedRange (one for each server)
    pub fn share_range(&self, x: &[u128], delta: u128) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        assert_eq!(x.len(), self.config.d, "Input x must match the configured dimension");
        // Calculate the maximum value for u bits
        let max_input = (1u128 << self.config.h1) - 1;
        
        // Validate that x is within the valid range
        for &xi in x {
            if xi > max_input {
                return Err(SharePhaseError::InvalidRange(
                    format!("Input x ({}) exceeds maximum value for {}-bit input ({})",
                            xi, self.config.h1, max_input)
                ));
            }
        }

        let left_bound = x.iter().map(|&xi| {
            if xi < delta {
                0 // Clamp to 0 if below delta
            } else {
                xi - delta
            }
        }).collect::<Vec<u128>>();

        let right_bound = x.iter().map(|&xi| {
            if xi + delta > max_input {
                max_input
            } else {
                xi + delta
            }
        }).collect::<Vec<u128>>();

        match &self.config.method {
            ShareMethod::OKVS => {
                let mut rng = rand::rng();
                let r1 = (0..self.config.d).map(|_| rng.random::<[u8; 16]>()).collect::<Vec<_>>();
                let r2 = (0..self.config.d).map(|_| rng.random::<[u8; 16]>()).collect::<Vec<_>>();
                match self.config.metric {
                    DistanceMetric::LInfinity => {
                        match self.config.dictionary_type {
                            DictionaryType::Known => self.share_with_okvs(&x, &left_bound, &right_bound, &r1, &r2, KnownLInfinityStrategy),
                            DictionaryType::Unknown => self.share_with_okvs(&x, &left_bound, &right_bound, &r1, &r2, UnknownLInfinityStrategy),
                        }
                    },
                    DistanceMetric::Lp { p } => {
                        match self.config.dictionary_type {
                            DictionaryType::Known => self.share_with_okvs(&x, &left_bound, &right_bound, &r1, &r2, KnownLpStrategy { p: p }),
                            DictionaryType::Unknown => self.share_with_okvs(&x, &left_bound, &right_bound, &r1, &r2, UnknownLpStrategy { p: p }),
                        }
                    },
                }
            }
            ShareMethod::FSS => {
                match self.config.metric {
                    DistanceMetric::LInfinity => {
                        self.share_with_interval_fss(&left_bound, &right_bound)
                    },
                    DistanceMetric::Lp { p } => {
                        if p > 3 {
                            return Err(SharePhaseError::InvalidRange("Distance FSS only supports p <= 3 for practical experiments".to_string()));
                        }
                        let modulus_mask = (1u128 << self.config.h2) - 1;
                        match p {
                            1 => self.share_with_distance_fss_l1(&x, &left_bound, &right_bound, (delta + 1) & modulus_mask),
                            2 => self.share_with_distance_fss_l2(&x, &left_bound, &right_bound, (delta * delta + 1) & modulus_mask),
                            3 => self.share_with_distance_fss_l3(&x, &left_bound, &right_bound, (((delta * delta) & modulus_mask) * delta + 1) & modulus_mask),
                            _ => return Err(SharePhaseError::InvalidRange("Distance FSS only supports p in range 1-3".to_string())),
                        }
                    },
                }
            }
        }
    }

    // Return shares of evaluation, where (y0 + y1) mod = true_result
    pub fn evaluate_at_single_dimension(
        &self,
        shared_range: &SharedRange,
        point_bits: &[bool],
        dimension: usize,
    ) -> Result<u128, SharePhaseError> {
        // Return 0 if point_bits is empty
        if point_bits.is_empty() {
            return Ok(0);
        }
        
        match shared_range {
            SharedRange::OKVS { okvs_shares, okvs_seeds, role, p: _ } => {
                let (r1, r2) = (okvs_seeds[dimension].0, okvs_seeds[dimension].1);
                let result = self.evaluate_okvs_generic(&okvs_shares[dimension], point_bits, *role, &r1, &r2)?;
                Ok(result)
            }
            SharedRange::IntervalFSS { keys, role: _ } => {
                let result = self.evaluate_interval_fss_at_single_dimension(&keys[dimension], point_bits)?;
                Ok(result)
            }
            SharedRange::DistanceFSSL1 { keys, role } => {
                let result = self.evaluate_distance_fss::<2>(&keys[dimension], point_bits, *role)?;
                Ok(result)
            }
            SharedRange::DistanceFSSL2 { keys, role } => {
                let result = self.evaluate_distance_fss::<3>(&keys[dimension], point_bits, *role)?;
                Ok(result)
            }
            SharedRange::DistanceFSSL3 { keys, role } => {
                let result = self.evaluate_distance_fss::<4>(&keys[dimension], point_bits, *role)?;
                Ok(result)
            }
        }
    }

    pub fn expand_prefix(&self, shared_range: &SharedRange, share_data: &ShareData, prefix: &[bool], dimension: usize) -> Result<(ShareData, ShareData), SharePhaseError> {
        match shared_range {
            SharedRange::OKVS { okvs_shares, okvs_seeds, role, p: _ } => {
                match share_data {
                    ShareData::OKVS { eval } => {
                        self.expand_prefix_okvs(okvs_shares, okvs_seeds, *role, prefix, eval, dimension)
                    },
                    _ => Err(SharePhaseError::InvalidShareData(
                        "Expected OKVS share data for OKVS shared range".to_string()
                    )),
                }
            },
            SharedRange::IntervalFSS { keys, role: _ } => {
                match share_data {
                    ShareData::IntervalFSS { data } => {
                        self.expand_prefix_interval_fss(keys, data, dimension)
                    },
                    _ => Err(SharePhaseError::InvalidShareData(
                        "Expected IntervalFSS share data for IntervalFSS shared range".to_string()
                    )),
                }
            },
            SharedRange::DistanceFSSL1 { keys, role } => {
                match share_data {
                    ShareData::DistanceFSSL1 { data, eval } => {
                        self.expand_prefix_distance_fss::<2>(keys, prefix, data, eval, dimension, *role)
                    },
                    _ => Err(SharePhaseError::InvalidShareData(
                        "Expected DistanceFSS share data for DistanceFSSL1 shared range".to_string()
                    )),
                }
            },
            SharedRange::DistanceFSSL2 { keys, role } => {
                match share_data {
                    ShareData::DistanceFSSL2 { data, eval } => {
                        self.expand_prefix_distance_fss::<3>(keys, prefix, data, eval, dimension, *role)
                    },
                    _ => Err(SharePhaseError::InvalidShareData(
                        "Expected DistanceFSS share data for DistanceFSSL2 shared range".to_string()
                    )),
                }
            },
            SharedRange::DistanceFSSL3 { keys, role } => {
                match share_data {
                    ShareData::DistanceFSSL3 { data, eval } => {
                        self.expand_prefix_distance_fss::<4>(keys, prefix, data, eval, dimension, *role)
                    },
                    _ => Err(SharePhaseError::InvalidShareData(
                        "Expected DistanceFSS share data for DistanceFSSL3 shared range".to_string()
                    )),
                }
            },
        }
    }

    pub fn share_data_init(&self, shared_range: &SharedRange) -> Result<ShareData, SharePhaseError> {
        let empty_prefix = vec![vec![]; self.config.d];
        let evals = (0..self.config.d).map(|dim| {
            self.evaluate_at_single_dimension(shared_range, &empty_prefix[dim], dim)
                .map_err(SharePhaseError::from)
        }).collect::<Result<Vec<u128>, _>>()?;

        let modulus = 1u128 << self.config.h2;

        match shared_range {
            SharedRange::OKVS { okvs_shares: _, okvs_seeds: _, role: _, p: _ } => {
                Ok(ShareData::OKVS {
                    eval: evals,
                })
            },
            SharedRange::IntervalFSS { keys, role: _ } => {
                Ok(ShareData::IntervalFSS {
                    data: keys.iter().map(|key| {
                        key.init_eval(modulus)
                    }).collect::<Vec<IntervalFSSEval<1>>>(),
                })
            },
            SharedRange::DistanceFSSL1 { keys, role: _ } => {
                Ok(ShareData::DistanceFSSL1 {
                    data: keys.iter().map(|key| {
                        key.init_eval(modulus)
                    }).collect::<Vec<DistanceFSSEval<2>>>(),
                    eval: evals,
                })
            },
            SharedRange::DistanceFSSL2 { keys, role: _ } => {
                Ok(ShareData::DistanceFSSL2 {
                    data: keys.iter().map(|key| {
                        key.init_eval(modulus)
                    }).collect::<Vec<DistanceFSSEval<3>>>(),
                    eval: evals,
                })
            },
            SharedRange::DistanceFSSL3 { keys, role: _ } => {
                Ok(ShareData::DistanceFSSL3 {
                    data: keys.iter().map(|key| {
                        key.init_eval(modulus)
                    }).collect::<Vec<DistanceFSSEval<4>>>(),
                    eval: evals,
                })
            },
        }
    }

    /// Generic OKVS sharing method that uses different strategies for key-value pair preparation
    fn share_with_okvs<T: KeyValuePairStrategy>(
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
        for dim in 0..self.config.d{
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
            let okvs = RbOkvsF2k::<u128>::new(
                keys.len(),
                columns,
                band_width,
                &r1[dim],
                &r2[dim],
            );

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
    fn evaluate_okvs_generic(
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

        let okvs = RbOkvsF2k::<u128>::new(
            1,
            columns,
            band_width,
            r1, 
            r2,
        );
        
        let result = okvs.decode(&okvs_share, &[key_bits.clone()]);
        if result.is_empty() {
            Ok(0) // Return 0 if decode fails
        } else {
            if !role {
                // Server 0: return the share directly
                Ok(result[0])
            } else {
                // Server 1: need to add the deterministic mask
                let key_bits_bytes: Vec<u8> = key_bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
                let hash = blake3::hash(&key_bits_bytes);
                let hash_bytes = hash.as_bytes();
                let value_mask = u128::from_le_bytes([
                    hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
                    hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
                    hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
                    hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
                ]) & modulus_mask;
                Ok((result[0] + modulus_mask + 1 - value_mask) & modulus_mask)
            }
        }
    }


    /// Share using Interval FSS method (N=1)
    fn share_with_interval_fss(
        &self,
        left_bound: &[u128],
        right_bound: &[u128],
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut keys_0 = Vec::new();
        let mut keys_1 = Vec::new();

        let modulus = 1u128 << self.config.h2;

        let left_payload = RingVec::<1>::new([1], modulus);
        let mid_payload = RingVec::<1>::new([0], modulus);
        let right_payload = RingVec::<1>::new([1], modulus);

        for (&alpha, &beta) in left_bound.iter().zip(right_bound.iter()) {
            if alpha > beta {
                return Err(SharePhaseError::InvalidRange(
                    format!("Left bound {} cannot be greater than right bound {}", alpha, beta)
                ));
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
            );

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

    fn evaluate_interval_fss_at_single_dimension(
        &self,
        fss_key: &IntervalFSSKey<1>,
        point_bits: &[bool],
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.h2;
        let result = fss_key.eval_interval_fss(point_bits, modulus);
        Ok(result[0])
    }

    /// Share using Distance FSS method for L1 distance (p=1, N=2)
    fn share_with_distance_fss_l1(
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
                return Err(SharePhaseError::InvalidRange(
                    format!("Left bound {} cannot be greater than right bound {}", alpha, beta)
                ));
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
            );
            
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
    fn share_with_distance_fss_l2(
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
                return Err(SharePhaseError::InvalidRange(
                    format!("Left bound {} cannot be greater than right bound {}", alpha, beta)
                ));
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
            );
            
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
    fn share_with_distance_fss_l3(
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
                return Err(SharePhaseError::InvalidRange(
                    format!("Left bound {} cannot be greater than right bound {}", alpha, beta)
                ));
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
            );
            
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
    fn evaluate_distance_fss<const N: usize>(
        &self,
        fss_key: &DistanceFSSKey<N>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.h2;
        let result = fss_key.eval_distance_fss(point_bits, self.config.h1, modulus);
        if !role {
            Ok(result)
        } else {
            Ok((modulus - result) % modulus)
        }
    }

    pub fn expand_prefix_okvs(
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
        eval0[dimension] = self.evaluate_okvs_generic(&okvs_shares[dimension], &new_prefix, role, &okvs_seeds[dimension].0, &okvs_seeds[dimension].1)?;
        new_prefix.pop();
        new_prefix.push(true);
        let mut eval1 = eval.to_vec();
        eval1[dimension] = self.evaluate_okvs_generic(&okvs_shares[dimension], &new_prefix, role, &okvs_seeds[dimension].0, &okvs_seeds[dimension].1)?;

        Ok((
            ShareData::OKVS {
                eval: eval0,
            },
            ShareData::OKVS {
                eval: eval1,
            },
        ))
    }

    pub fn expand_prefix_interval_fss(
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
        (data0[dimension], data1[dimension]) = interval_fss_key.expand_prefix(interval_fss_data, modulus);

        Ok((
            ShareData::IntervalFSS {
                data: data0,
            },
            ShareData::IntervalFSS {
                data: data1,
            },
        ))
    }

    fn expand_prefix_distance_fss<const N: usize>(
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
        (data0[dimension], data1[dimension]) = key.expand_prefix(prefix, &data[dimension], input_len, modulus);

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
            2 => {
                Ok((
                    ShareData::DistanceFSSL1 {
                        data: unsafe { std::mem::transmute(data0) },
                        eval: eval0,
                    },
                    ShareData::DistanceFSSL1 {
                        data: unsafe { std::mem::transmute(data1) },
                        eval: eval1,
                    },
                ))
            },
            3 => {
                Ok((
                    ShareData::DistanceFSSL2 {
                        data: unsafe { std::mem::transmute(data0) },
                        eval: eval0,
                    },
                    ShareData::DistanceFSSL2 {
                        data: unsafe { std::mem::transmute(data1) },
                        eval: eval1,
                    },
                ))
            },
            4 => {
                Ok((
                    ShareData::DistanceFSSL3 {
                        data: unsafe { std::mem::transmute(data0) },
                        eval: eval0,
                    },
                    ShareData::DistanceFSSL3 {
                        data: unsafe { std::mem::transmute(data1) },
                        eval: eval1,
                    },
                ))
            },
            _ => Err(SharePhaseError::EvaluationError("Invalid share data type for distance FSS expansion".to_string())),
        }
    }

}

fn send_u128(
    channel: &mut CommTrackingChannel,
    value: u128,
) -> Result<(), SharePhaseError> {
    let buf = value.to_le_bytes();
    channel.write_bytes(&buf).map_err(|e| SharePhaseError::EvaluationError(format!("Failed to send u128: {}", e)))?;
    Ok(())
}

fn recv_u128(
    channel: &mut CommTrackingChannel,
) -> Result<u128, SharePhaseError> {
    let mut buf = [0u8; 16];
    channel.read_bytes(&mut buf).map_err(|e| SharePhaseError::EvaluationError(format!("Failed to receive u128: {}", e)))?;
    Ok(u128::from_le_bytes(buf))
}

fn send_array(
    channel: &mut CommTrackingChannel,
    array: &Vec<Vec<u128>>,
    length: usize,
    width: usize,
) -> Result<(), SharePhaseError> {
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&array.len().to_le_bytes());
    for l in 0..length {
        for w in 0..width {
            buffer.extend_from_slice(&array[l][w].to_le_bytes());
        }
    }
    let len_buf: [u8; 8] = buffer.len().to_le_bytes().try_into().unwrap();
    channel.write_bytes(&len_buf).map_err(|e| SharePhaseError::EvaluationError(format!("Failed to send array length: {}", e)))?;
    channel.write_bytes(&buffer).map_err(|e| SharePhaseError::EvaluationError(format!("Failed to send array data: {}", e)))?;
    Ok(())
}

fn recv_array(
    channel: &mut CommTrackingChannel,
    array: &mut Vec<Vec<u128>>,
    length: usize,
    width: usize,
) -> Result<(), SharePhaseError> {
    let mut len_buf = [0u8; 8];
    channel.read_bytes(&mut len_buf).map_err(|e| SharePhaseError::EvaluationError(format!("Failed to receive array length: {}", e)))?;
    let data_len = usize::from_le_bytes(len_buf);
    let mut buffer = vec![0u8; data_len];
    channel.read_bytes(&mut buffer).map_err(|e| SharePhaseError::EvaluationError(format!("Failed to receive array data: {}", e)))?;
    let mut offset = 0;
    for l in 0..length {
        for w in 0..width {
            let mut num_buf = [0u8; 16];
            num_buf.copy_from_slice(&buffer[offset..offset + 16]);
            array[l][w] = u128::from_le_bytes(num_buf);
            offset += 16;
        }
    }
    Ok(())
}

/// Errors that can occur during the share phase
#[derive(Debug, Clone)]
pub enum SharePhaseError {
    /// Invalid range parameters
    InvalidRange(String),
    /// Invalid share data for the given shared range
    InvalidShareData(String),
    /// OKVS encoding failed
    OKVSError(String),
    /// Interval FSS generation failed  
    IntervalFSSError(String),
    /// Evaluation failed
    EvaluationError(String),
}

impl std::fmt::Display for SharePhaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SharePhaseError::InvalidRange(msg) => write!(f, "Invalid range: {}", msg),
            SharePhaseError::OKVSError(msg) => write!(f, "OKVS error: {}", msg),
            SharePhaseError::IntervalFSSError(msg) => write!(f, "Interval FSS error: {}", msg),
            SharePhaseError::EvaluationError(msg) => write!(f, "Evaluation error: {}", msg),
            SharePhaseError::InvalidShareData(msg) => write!(f, "Invalid share data: {}", msg),
        }
    }
}

impl std::error::Error for SharePhaseError {}

impl From<crate::okvs_f2k::OkvsError> for SharePhaseError {
    fn from(err: crate::okvs_f2k::OkvsError) -> Self {
        SharePhaseError::OKVSError(err.to_string())
    }
}
