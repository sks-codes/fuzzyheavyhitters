//! Share Phase Implementation
//! 
//! This module provides functionality for sharing either OKVS (Oblivious Key-Value Store)
//! or Interval FSS (Function Secret Sharing) based on user input and threshold.
//! 
//! The share phase allows a user with an input x (u-bit string) and threshold delta
//! to share either:
//! 1. An OKVS that encodes all key-value pairs where keys are in range [x-delta, x+delta] 
//!    and values are all 1
//! 2. An interval FSS with left=1, mid=0, right=1 for the range [x-delta, x+delta]

use std::sync::Arc;
use std::cmp::max;
use std::collections::HashSet;
use rand::Rng;
use blake3;

use crate::fss;
use crate::okvs_f2k::{self, RbOkvsF2k};
use crate::fss::{
    interval::IntervalFSSKey,
    distance::DistanceFSSKey,
};
use crate::data_structures::payload::RingVec;
use crate::util::u128_to_bits;
use serde::{Deserialize, Serialize};

// Import strategies from the separate module
use super::strategies::{
    KeyValuePairStrategy, 
    KnownLInfinityStrategy, 
    UnknownLInfinityStrategy, 
    KnownLpStrategy, 
    UnknownLpStrategy
};

/// Enumeration of different distance metrics
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DistanceMetric {
    /// L-infinity distance (max of absolute differences)
    LInfinity,
    /// Lp distance with specified p value
    Lp { p: u32 },
}

/// Enumeration of dictionary types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DictionaryType {
    /// Known dictionary case - exact values in range
    Known,
    /// Unknown dictionary case - all prefixes of values in range
    Unknown,
}

/// Enumeration of different sharing methods available
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareMethod {
    /// Use OKVS for sharing
    OKVS,
    /// Use FSS for sharing (Interval FSS for L-infinity, Distance FSS for Lp)
    FSS,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareData {
    OKVS {
        r1: [u8; 16],
        r2: [u8; 16],
    },
    FSS,
}

/// Configuration for the share phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareConfig {
    /// The sharing method to use
    pub method: ShareMethod,
    /// The distance metric to use
    pub metric: DistanceMetric,
    /// The dictionary type (known or unknown)
    pub dictionary_type: DictionaryType,
    /// Number of bits for representing input values (u)
    pub input_bit_length: usize,
    /// Number of bits for representing output values (v)
    pub output_bit_length: usize,
    /// Dimension of the input space
    pub dimension: usize,
    /// Data specific to the sharing method
    pub data: ShareData,
}

/// Represents the shared data for a range around input x
#[derive(Debug, Clone)]
pub enum SharedRange {
    OKVS {
        okvs_shares: Vec<Vec<u128>>, // One OKVS encoding per dimension
        role: bool, // True for server 1, false for server 0
        p: Option<u32>,
    },
    IntervalFSS {
        keys: Vec<IntervalFSSKey<1>>, // One key per dimension
        role: bool,
    },
    DistanceFSSL1 {
        keys: Vec<DistanceFSSKey<2>>, // N = p+1 = 2
        role: bool,
    },
    DistanceFSSL2 {
        keys: Vec<DistanceFSSKey<3>>, // N = p+1 = 3
        role: bool,
    },
    DistanceFSSL3 {
        keys: Vec<DistanceFSSKey<4>>, // N = p+1 = 4
        role: bool,
    },
}

impl SharedRange {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            SharedRange::OKVS { okvs_shares, role, p } => {
                out.push(0u8); // tag for OKVS
                out.push(*role as u8);
                match p {
                    Some(val) => {
                        out.push(1u8);
                        out.extend_from_slice(&val.to_le_bytes());
                    },
                    None => {
                        out.push(0u8);
                    }
                }
                out.extend_from_slice(&(okvs_shares.len() as u32).to_le_bytes());
                for dim in okvs_shares {
                    out.extend_from_slice(&(dim.len() as u32).to_le_bytes());
                    for v in dim {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
            SharedRange::IntervalFSS { keys, role } => {
                out.push(1u8); // tag for IntervalFSS
                out.push(*role as u8);
                out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
                for k in keys {
                    out.extend_from_slice(&k.to_bytes());
                }
            }
            SharedRange::DistanceFSSL1 { keys, role } => {
                out.push(2u8); // tag for DistanceFSSL1
                out.push(*role as u8);
                out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
                for k in keys {
                    out.extend_from_slice(&k.to_bytes());
                }
            }
            SharedRange::DistanceFSSL2 { keys, role } => {
                out.push(3u8); // tag for DistanceFSSL2
                out.push(*role as u8);
                out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
                for k in keys {
                    out.extend_from_slice(&k.to_bytes());
                }
            }
            SharedRange::DistanceFSSL3 { keys, role } => {
                out.push(4u8); // tag for DistanceFSSL3
                out.push(*role as u8);
                out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
                for k in keys {
                    out.extend_from_slice(&k.to_bytes());
                }
            }
        }
        out
    }

    /// Returns (SharedRange, rest)
    pub fn from_bytes(mut bytes: &[u8], modulus: u128) -> Result<(Self, &[u8]), String> {
        if bytes.is_empty() { return Err("Empty bytes for SharedRange".to_string()); }
        let tag = bytes[0];
        bytes = &bytes[1..];
        match tag {
            0 => {
                // OKVS
                if bytes.len() < 2 { return Err("Too short for OKVS header".to_string()); }
                let role = bytes[0] != 0;
                let has_p = bytes[1];
                bytes = &bytes[2..];
                let p = if has_p == 1 {
                    if bytes.len() < 4 { return Err("Too short for OKVS p value".to_string()); }
                    let mut arr = [0u8; 4];
                    arr.copy_from_slice(&bytes[..4]);
                    bytes = &bytes[4..];
                    Some(u32::from_le_bytes(arr))
                } else { None };
                if bytes.len() < 4 { return Err("Too short for OKVS dim count".to_string()); }
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&bytes[..4]);
                let dim_count = u32::from_le_bytes(arr) as usize;
                bytes = &bytes[4..];
                let mut okvs_shares = Vec::with_capacity(dim_count);
                for _ in 0..dim_count {
                    if bytes.len() < 4 { return Err("Too short for OKVS dim len".to_string()); }
                    let mut arr = [0u8; 4];
                    arr.copy_from_slice(&bytes[..4]);
                    let dim_len = u32::from_le_bytes(arr) as usize;
                    bytes = &bytes[4..];
                    let mut dim = Vec::with_capacity(dim_len);
                    for _ in 0..dim_len {
                        if bytes.len() < 16 { return Err("Too short for u128 in OKVS share".to_string()); }
                        let mut arr = [0u8; 16];
                        arr.copy_from_slice(&bytes[..16]);
                        dim.push(u128::from_le_bytes(arr));
                        bytes = &bytes[16..];
                    }
                    okvs_shares.push(dim);
                }
                Ok((SharedRange::OKVS { okvs_shares, role, p }, bytes))
            }
            1 => {
                // IntervalFSS
                if bytes.len() < 5 { return Err("Too short for IntervalFSS header".to_string()); }
                let role = bytes[0] != 0;
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&bytes[1..5]);
                let key_count = u32::from_le_bytes(arr) as usize;
                bytes = &bytes[5..];
                let mut keys = Vec::with_capacity(key_count);
                for _ in 0..key_count {
                    let (k, rest) = IntervalFSSKey::<1>::from_bytes_partial(bytes)?;
                    keys.push(k);
                    bytes = rest;
                }
                Ok((SharedRange::IntervalFSS { keys, role }, bytes))
            }
            2 => {
                // DistanceFSSL1
                if bytes.len() < 5 { return Err("Too short for DistanceFSSL1 header".to_string()); }
                let role = bytes[0] != 0;
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&bytes[1..5]);
                let key_count = u32::from_le_bytes(arr) as usize;
                bytes = &bytes[5..];
                let mut keys = Vec::with_capacity(key_count);
                for _ in 0..key_count {
                    let (k, rest) = DistanceFSSKey::<2>::from_bytes_partial(bytes)?;
                    keys.push(k);
                    bytes = rest;
                }
                Ok((SharedRange::DistanceFSSL1 { keys, role }, bytes))
            }
            3 => {
                // DistanceFSSL2
                if bytes.len() < 5 { return Err("Too short for DistanceFSSL2 header".to_string()); }
                let role = bytes[0] != 0;
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&bytes[1..5]);
                let key_count = u32::from_le_bytes(arr) as usize;
                bytes = &bytes[5..];
                let mut keys = Vec::with_capacity(key_count);
                for _ in 0..key_count {
                    let (k, rest) = DistanceFSSKey::<3>::from_bytes_partial(bytes)?;
                    keys.push(k);
                    bytes = rest;
                }
                Ok((SharedRange::DistanceFSSL2 { keys, role }, bytes))
            }
            4 => {
                // DistanceFSSL3
                if bytes.len() < 5 { return Err("Too short for DistanceFSSL3 header".to_string()); }
                let role = bytes[0] != 0;
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&bytes[1..5]);
                let key_count = u32::from_le_bytes(arr) as usize;
                bytes = &bytes[5..];
                let mut keys = Vec::with_capacity(key_count);
                for _ in 0..key_count {
                    let (k, rest) = DistanceFSSKey::<4>::from_bytes_partial(bytes)?;
                    keys.push(k);
                    bytes = rest;
                }
                Ok((SharedRange::DistanceFSSL3 { keys, role }, bytes))
            }
            _ => Err("Unknown SharedRange tag".to_string()),
        }
    }
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
        assert_eq!(x.len(), self.config.dimension, "Input x must match the configured dimension");
        // Calculate the maximum value for u bits
        let max_input = (1u128 << self.config.input_bit_length) - 1;
        
        // Validate that x is within the valid range
        for &xi in x {
            if xi > max_input {
                return Err(SharePhaseError::InvalidRange(
                    format!("Input x ({}) exceeds maximum value for {}-bit input ({})",
                            xi, self.config.input_bit_length, max_input)
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

        match (&self.config.method, &self.config.metric) {
            (ShareMethod::OKVS, DistanceMetric::LInfinity) => {
                match self.config.data {
                    ShareData::OKVS { r1, r2 } => {
                        match self.config.dictionary_type {
                            DictionaryType::Known => {
                                self.share_with_okvs(&x, &left_bound, &right_bound, &r1, &r2, KnownLInfinityStrategy)
                            },
                            DictionaryType::Unknown => {
                                self.share_with_okvs(&x, &left_bound, &right_bound, &r1, &r2, UnknownLInfinityStrategy)
                            },
                        }
                    },
                    _ => return Err(SharePhaseError::InvalidRange("OKVS data not provided for L-infinity".to_string())),
                }
            },
            (ShareMethod::OKVS, DistanceMetric::Lp { p }) => {
                match self.config.data {
                    ShareData::OKVS { r1, r2 } => {
                        match self.config.dictionary_type {
                            DictionaryType::Known => {
                                self.share_with_okvs(&x, &left_bound, &right_bound, &r1, &r2, KnownLpStrategy { p: *p })
                            },
                            DictionaryType::Unknown => {
                                self.share_with_okvs(&x, &left_bound, &right_bound, &r1, &r2, UnknownLpStrategy { p: *p })
                            },
                        }
                    },
                    _ => return Err(SharePhaseError::InvalidRange("OKVS data not provided for Lp distance".to_string())),
                }
            },
            (ShareMethod::FSS, DistanceMetric::LInfinity) => {
                // Use Interval FSS for L-infinity distance
                // FSS method is the same for both known and unknown dictionary
                // since FSS already handles evaluating on prefixes
                self.share_with_interval_fss(&left_bound, &right_bound)
            },
            (ShareMethod::FSS, DistanceMetric::Lp { p }) => {
                // Use Distance FSS for Lp distance
                if *p > 3 {
                    return Err(SharePhaseError::InvalidRange("Distance FSS only supports p <= 3 for practical experiments".to_string()));
                }
                // Distance FSS method is the same for both known and unknown dictionary
                // since FSS already handles evaluating on prefixes
                let modulus_mask = (1u128 << self.config.output_bit_length) - 1;
                match *p {
                    1 => self.share_with_distance_fss_l1(&x, &left_bound, &right_bound, 
                        (delta + 1) & modulus_mask),
                    2 => self.share_with_distance_fss_l2(&x, &left_bound, &right_bound, 
                        (delta * delta + 1) & modulus_mask),
                    3 => self.share_with_distance_fss_l3(&x, &left_bound, &right_bound, 
                        (((delta * delta) & modulus_mask) * delta + 1) & modulus_mask),
                    _ => Err(SharePhaseError::InvalidRange("Distance FSS only supports p in range 1-3".to_string())),
                }
            },
        }
    }

    /// Public method to evaluate at a specific point for a single dimension
    /// This is for backward compatibility and specific use cases like check_phase
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
            SharedRange::OKVS { okvs_shares, role, p } => {
                // Get r1 and r2 from config - now only OKVS variant exists
                let (r1, r2) = match &self.config.data {
                    ShareData::OKVS { r1, r2 } => (r1, r2),
                    _ => return Err(SharePhaseError::InvalidRange("OKVS requires OKVS config".to_string())),
                };

                let result = self.evaluate_okvs_generic(&okvs_shares[dimension], point_bits, *role, r1, r2)?;
                Ok(result)
            }
            SharedRange::IntervalFSS { keys, role } => {
                let result = self.evaluate_interval_fss_at_single_dimension(&keys[dimension], point_bits, *role)?;
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


    /// Generic OKVS sharing method that uses different strategies for key-value pair preparation
    fn share_with_okvs<T: KeyValuePairStrategy>(
        &self,
        x: &[u128], // The original d-dimensional vector
        left_bound: &Vec<u128>,
        right_bound: &Vec<u128>,
        r1: &[u8; 16],
        r2: &[u8; 16],
        strategy: T,
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut okvs_shares_0 = Vec::new();
        let mut okvs_shares_1 = Vec::new();

        // Create separate OKVS for each dimension
        for dim in 0..self.config.dimension {
            let left = left_bound[dim];
            let right = right_bound[dim];
            let x_i = x[dim];

            // Use the strategy to prepare key-value pairs
            let (keys, values_0, values_1) = strategy.prepare_key_value_pairs(
                dim,
                left,
                right,
                x_i,
                self.config.input_bit_length,
                self.config.output_bit_length,
            );

            if keys.is_empty() {
                // If no keys for this dimension, create empty OKVS
                okvs_shares_0.push(Vec::new());
                okvs_shares_1.push(Vec::new());
                continue;
            }

            let columns = max((keys.len() as f64 * 1.1) as usize, 60);
            let band_width = 55;
            let okvs = RbOkvsF2k::<u128>::new(
                keys.len(),
                columns,
                band_width,
                r1,
                r2,
            );

            let encoding_0 = okvs.encode(&keys, &values_0)?;
            let encoding_1 = okvs.encode(&keys, &values_1)?;

            okvs_shares_0.push(encoding_0);
            okvs_shares_1.push(encoding_1);
        }

        Ok((
            SharedRange::OKVS {
                okvs_shares: okvs_shares_0,
                role: false, // Server 0
                p: match &self.config.metric {
                    DistanceMetric::LInfinity => None,
                    DistanceMetric::Lp { p } => Some(*p),
                },
            },
            SharedRange::OKVS {
                okvs_shares: okvs_shares_1,
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
        let modulus_mask = (1u128 << self.config.output_bit_length) - 1;
        
        let okvs = RbOkvsF2k::<u128>::new(
            1,
            okvs_share.len(),
            55, // Band width
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

        let modulus = 1u128 << self.config.output_bit_length;

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
            let mut alpha_bits = u128_to_bits(alpha, self.config.input_bit_length);
            alpha_bits.reverse();
            let mut beta_bits = u128_to_bits(beta, self.config.input_bit_length);
            beta_bits.reverse();

            // Create Interval FSS keys for both servers
            let (fss_key_0, fss_key_1) = IntervalFSSKey::<1>::gen_IntervalFSSKey(
                &alpha_bits, 
                &beta_bits, 
                &left_payload, 
                &mid_payload, 
                &right_payload, 
                modulus
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

    /// Helper: Evaluate FSS at a single dimension (for backward compatibility) 
    fn evaluate_interval_fss_at_single_dimension(
        &self,
        fss_key: &IntervalFSSKey<1>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.output_bit_length;
        let result = fss_key.eval_intervalFSS(point_bits, modulus);
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

        let modulus = 1u128 << self.config.output_bit_length;
        
        for ((&alpha, &beta), &center) in left_bound.iter().zip(right_bound.iter()).zip(x.iter()) {
            if alpha > beta || alpha > center || beta < center {
                return Err(SharePhaseError::InvalidRange(
                    format!("Left bound {} cannot be greater than right bound {}", alpha, beta)
                ));
            }

            // Convert bounds to bits representation
            let mut alpha_bits = u128_to_bits(alpha, self.config.input_bit_length);
            alpha_bits.reverse();
            let mut beta_bits = u128_to_bits(beta, self.config.input_bit_length);
            beta_bits.reverse();
            let mut center_bits = u128_to_bits(center, self.config.input_bit_length);
            center_bits.reverse();

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

        let modulus = 1u128 << self.config.output_bit_length;
        
        for ((&alpha, &beta), &center) in left_bound.iter().zip(right_bound.iter()).zip(x.iter()) {
            if alpha > beta || alpha > center || beta < center {
                return Err(SharePhaseError::InvalidRange(
                    format!("Left bound {} cannot be greater than right bound {}", alpha, beta)
                ));
            }

            // Convert bounds to bits representation
            let mut alpha_bits = u128_to_bits(alpha, self.config.input_bit_length);
            alpha_bits.reverse();
            let mut beta_bits = u128_to_bits(beta, self.config.input_bit_length);
            beta_bits.reverse();
            let mut center_bits = u128_to_bits(center, self.config.input_bit_length);
            center_bits.reverse();

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

        let modulus = 1u128 << self.config.output_bit_length;
        
        for ((&alpha, &beta), &center) in left_bound.iter().zip(right_bound.iter()).zip(x.iter()) {
            if alpha > beta || alpha > center || beta < center {
                return Err(SharePhaseError::InvalidRange(
                    format!("Left bound {} cannot be greater than right bound {}", alpha, beta)
                ));
            }

            // Convert bounds to bits representation
            let mut alpha_bits = u128_to_bits(alpha, self.config.input_bit_length);
            alpha_bits.reverse();
            let mut beta_bits = u128_to_bits(beta, self.config.input_bit_length);
            beta_bits.reverse();
            let mut center_bits = u128_to_bits(center, self.config.input_bit_length);
            center_bits.reverse();

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
        let modulus = 1u128 << self.config.output_bit_length;
        let result = fss_key.eval_distance_fss(point_bits, self.config.input_bit_length, modulus);
        if !role {
            Ok(result) // Server 0 returns the share directly
        } else {
            Ok((modulus - result) % modulus) // Server 1 returns the complement
        }
    }
}

/// Errors that can occur during the share phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SharePhaseError {
    /// Invalid range parameters
    InvalidRange(String),
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
        }
    }
}

impl std::error::Error for SharePhaseError {}

impl From<crate::okvs_f2k::OkvsError> for SharePhaseError {
    fn from(err: crate::okvs_f2k::OkvsError) -> Self {
        SharePhaseError::OKVSError(err.to_string())
    }
}
