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
    data_structures::ringvec::RingVec,
    util::u128_to_bits_msb,
    fuzzy_match::shared_range::{SharedRange, ShareData},
};
use std::cmp::{max, min};
use scuttlebutt::AbstractChannel;
use rayon::prelude::*;

// Import strategies from the separate module
use super::strategies::{
    KeyValuePairStrategy, 
    KnownLInfinityStrategy, 
    UnknownLInfinityStrategy, 
    KnownLpStrategy, 
    UnknownLpStrategy
};

/// Enumeration of different distance metrics
#[derive(Debug, Clone, PartialEq)]
pub enum DistanceMetric {
    /// L-infinity distance (max of absolute differences)
    LInfinity,
    /// Lp distance with specified p value
    Lp { p: u32 },
}

/// Enumeration of dictionary types
#[derive(Debug, Clone, PartialEq)]
pub enum DictionaryType {
    /// Known dictionary case - exact values in range
    Known,
    /// Unknown dictionary case - all prefixes of values in range
    Unknown,
}

/// Enumeration of different sharing methods available
#[derive(Debug, Clone, PartialEq)]
pub enum ShareMethod {
    /// Use OKVS for sharing
    OKVS,
    /// Use FSS for sharing (Interval FSS for L-infinity, Distance FSS for Lp)
    FSS,
}


/// Configuration for the share phase
#[derive(Debug, Clone, PartialEq)]
pub struct ShareConfig {
    /// The sharing method to use
    pub method: ShareMethod,
    /// The distance metric to use
    pub metric: DistanceMetric,
    /// The dictionary type (known or unknown)
    pub dictionary_type: DictionaryType,
    /// Number of bits for representing input values (u)
    pub h1: usize,
    /// Number of bits for representing output values (v)
    pub h2: usize,
    /// Dimension of the input space
    pub d: usize,
}

/// Share phase handler
#[derive(Clone)]
pub struct SharePhase {
    pub config: ShareConfig,
}

impl SharePhase {
    /// Create a new share phase with the given configuration
    pub fn new(config: ShareConfig) -> Self {
        Self { config }
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


    pub fn sketch(
        &self,
        shared_ranges: &[SharedRange],
        delta: u128,
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
            "IntervalFSS" => self.parallel_sketch_interval_fss(shared_ranges, seed, delta, other_server_channels),
            "DistanceFSSL1" => self.sketch_distance_fss::<2>(shared_ranges),
            "DistanceFSSL2" => self.sketch_distance_fss::<3>(shared_ranges),
            "DistanceFSSL3" => self.sketch_distance_fss::<4>(shared_ranges),
            _ => Err(SharePhaseError::InvalidRange(
                "Unknown shared range type for sketching".to_string()
            )),
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

    // This function is NOT READY to be used!!!
    fn parallel_sketch_interval_fss(
        &self,
        shared_ranges: &[SharedRange],
        seed: [u8; AES_KEY_SIZE],
        delta: u128,
        other_server_channels: &mut [CommTrackingChannel],
    ) -> Result<bool, SharePhaseError> {
        // First just do full domain evaluation
        // Check dcf by shifting by one, and subtract, to reduce to dpf. Don't need to pad one to the left, since we only want to check if the whole interval is equal to each other, don't care about the payload.
        // Check interval equals delta by shifting with delta and minus. The lefter dcf does not need to pad to the left, since we already checked that the payload inside the interval is the same. 
        // In this code, we just use Aes256 in ctr mode as random oracle
        // WARNING! Only works when the input range is smaller than 60 bits

        let num_threads = other_server_channels.len();
        let thread_pool = rayon::ThreadPoolBuilder::new().num_threads(num_threads).build().unwrap();
        let domain_range = 1u128 << self.config.h1;
        let total_mod = 1u128 << self.config.h1;
        let modulus = 1u128 << self.config.h2;

        let mut z1s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut z2s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut z3s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut z4s = vec![vec![0u128; self.config.d]; shared_ranges.len()];
        let mut z5s = vec![vec![0u128; self.config.d]; shared_ranges.len()];

        thread_pool.install(|| {
            shared_ranges.par_iter()
                .zip(z1s.par_iter_mut())
                .zip(z2s.par_iter_mut())
                .zip(z3s.par_iter_mut())
                .zip(z4s.par_iter_mut())
                .zip(z5s.par_iter_mut())
                .enumerate()
                .for_each(|(i, (((((shared_range, z1_row), z2_row), z3_row), z4_row), z5_row))| {
                    match shared_range {
                        SharedRange::IntervalFSS { keys, .. } => {
                            // Create a seed for this index only, by xoring the global seed with the index
                            let mut blocks = vec![[0u8; 16]; domain_range as usize * self.config.d];
                            let mut prg = PRG::new(Some(&seed), i as u64);
                            prg.random_16byte_block(&mut blocks);
                            let rs = blocks.iter().map(|&b| {
                                let num = u128::from_le_bytes(b);
                                num % total_mod
                            }).collect::<Vec<u128>>();
                            let rs2 = rs.iter()
                                .map(|&r| (r * r) % total_mod)
                                .collect::<Vec<u128>>(); // ri^2
                            let rs3 = rs.iter().zip(rs2.iter())
                                .map(|(&r, &r2)| (r * r2) % total_mod)
                                .collect::<Vec<u128>>(); // ri^3
                            let rs4 = rs2.iter()
                                .map(|&r2| (r2 * r2) % total_mod)
                                .collect::<Vec<u128>>(); // ri^4
                            let rs6 = rs3.iter()
                                .map(|&r3| (r3 * r3) % total_mod)
                                .collect::<Vec<u128>>(); // ri^6
                            // There are d dimensions
                            for dimension in 0..self.config.d {
                                let key = &keys[dimension];
                                let ldcf_evals = key.full_domain_eval_ldcf(modulus, self.config.h1).iter()
                                    .map(|eval| eval[0])
                                    .collect::<Vec<u128>>();
                                let rdcf_evals = key.full_domain_eval_rdcf(modulus, self.config.h1).iter()
                                    .map(|eval| eval[0])
                                    .collect::<Vec<u128>>();

                                // CHECK LDCF
                                // Shift by one and subtract to turn into DPF
                                let ldcf_evals1 = (1..ldcf_evals.len())
                                    .map(|j| {
                                        (ldcf_evals[j] + modulus - ldcf_evals[j - 1]) % modulus
                                    })
                                    .collect::<Vec<u128>>();
                                let range_start = domain_range as usize * dimension;
                                let range_end = domain_range as usize * (dimension + 1);
                                z1_row[dimension] = ldcf_evals1.iter()
                                    .zip(rs[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r)| {
                                        (acc + eval * r) % total_mod
                                    }); // sum of ri * evali. This sum should be non-zero at only one position.
                                z2_row[dimension] = ldcf_evals1.iter()
                                    .zip(rs2[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r2)| {
                                        (acc + eval * r2) % total_mod
                                    }); // sum of ri^2 * evali. We should have z2 = z1^2 if only one position is non-zero, and it is 1.
                                
                                // CHECK RDCF
                                // Shift by one and subtract to turn into DPF
                                let rdcf_evals1 = (1..rdcf_evals.len())
                                    .map(|j| {
                                        (rdcf_evals[j] + modulus - rdcf_evals[j - 1]) % modulus
                                    })
                                    .collect::<Vec<u128>>();
                                z3_row[dimension] = rdcf_evals1.iter()
                                    .zip(rs3[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r3)| {
                                        (acc + eval * r3) % total_mod
                                    }); // sum of ri^3 * evali. This sum should be non-zero at only one position.
                                z4_row[dimension] = rdcf_evals1.iter()
                                    .zip(rs6[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r6)| {
                                        (acc + eval * r6) % total_mod
                                    }); // sum of ri^6 * evali. We should have z4 = z3^2 if only one position is non-zero, and it is 1.


                                // Check whether the interval is smaller than or equal to 2 * delta + 1 by shifting rdcf by 2 * delta + 1 and subtracting
                                let rdcf_ldcf_evals = (0..rdcf_evals1.len() - (2 * delta as usize + 1))
                                    .map(|j| {
                                        (rdcf_evals[j + 2 * delta as usize + 1] + modulus - ldcf_evals1[j]) % modulus
                                    })
                                    .collect::<Vec<u128>>();
                                z5_row[dimension] = rdcf_ldcf_evals.iter()
                                    .zip(rs4[range_start..range_end].iter())
                                    .fold(0u128, |acc, (&eval, &r4)| {
                                        (acc + eval * r4) % total_mod
                                    }); // sum of ri * evali. This sum should be non-zero at only one position.
                            }
                        }
                        _ => {
                            panic!("Sketching failed: Expected IntervalFSS shared range");
                        }
                    }
                })
            });

        Ok(true)
    }

    fn sketch_distance_fss<const N: usize>(
        &self,
        _shared_ranges: &[SharedRange],
    ) -> Result<bool, SharePhaseError> {
        unimplemented!()
    }
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
