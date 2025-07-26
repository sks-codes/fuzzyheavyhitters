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

use crate::okvs_f2k::{self, RbOkvsF2k};
use crate::fss::interval::{IntervalFSSKey, IntervalFSSEval};
use crate::fss::distance::DistanceFSSKey;
use crate::data_structures::{field::FieldElm, payload::RingVec};
use crate::util::u128_to_bits;
use serde::{Deserialize, Serialize};

/// Enumeration of different sharing methods available
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareMethod {
    /// Use OKVS for sharing
    OKVS,
    /// Use FSS for sharing (Interval FSS for L-infinity, Distance FSS for Lp)
    FSS,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareData {
    OKVS {
        r1: [u8; 16],
        r2: [u8; 16],
    },
    FSS {
    },
    /// Data for Lp distance sharing
    Lp {
        r1: [u8; 16],
        r2: [u8; 16],
        p: u32, // The p value for Lp distance
    },
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
    /// OKVS-based sharing with one OKVS per dimension (for L-infinity)
    /// Each server gets one OKVS per dimension
    OKVS {
        okvs_shares: Vec<Vec<u128>>, // One OKVS encoding per dimension
        role: bool, // True for server 1, false for server 0
    },
    /// Interval FSS-based sharing (N=1)
    IntervalFSS {
        fss_key: Vec<IntervalFSSKey<1>>,
        role: bool, // True for server 1, false for server 0
    },
    /// OKVS-based sharing for Lp distance
    /// Each server gets one OKVS per dimension that encodes |y - x_i|^p
    OKVSLp {
        okvs_shares: Vec<Vec<u128>>, // One OKVS encoding per dimension
        role: bool, // True for server 1, false for server 0
        p: u32, // The p value for Lp distance
    },
    /// Distance FSS-based sharing for Lp distance (p=1)
    DistanceFSS1 {
        fss_keys: Vec<DistanceFSSKey<2>>, // One Distance FSS key per dimension (p=1, N=2)
        role: bool, // True for server 1, false for server 0
    },
    /// Distance FSS-based sharing for Lp distance (p=2)
    DistanceFSS2 {
        fss_keys: Vec<DistanceFSSKey<3>>, // One Distance FSS key per dimension (p=2, N=3)
        role: bool, // True for server 1, false for server 0
    },
    /// Distance FSS-based sharing for Lp distance (p=3)
    DistanceFSS3 {
        fss_keys: Vec<DistanceFSSKey<4>>, // One Distance FSS key per dimension (p=3, N=4)
        role: bool, // True for server 1, false for server 0
    },
    /// Distance FSS-based sharing for Lp distance (p=4)
    DistanceFSS4 {
        fss_keys: Vec<DistanceFSSKey<5>>, // One Distance FSS key per dimension (p=4, N=5)
        role: bool, // True for server 1, false for server 0
    },
    /// Distance FSS-based sharing for Lp distance (p=5)
    DistanceFSS5 {
        fss_keys: Vec<DistanceFSSKey<6>>, // One Distance FSS key per dimension (p=5, N=6)
        role: bool, // True for server 1, false for server 0
    },
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
                                self.share_with_okvs_known(&x, &left_bound, &right_bound, &r1, &r2)
                            },
                            DictionaryType::Unknown => {
                                self.share_with_okvs_unknown(&x, &left_bound, &right_bound, &r1, &r2)
                            },
                        }
                    },
                    _ => return Err(SharePhaseError::InvalidRange("OKVS data not provided for L-infinity".to_string())),
                }
            },
            (ShareMethod::OKVS, DistanceMetric::Lp { p }) => {
                match self.config.data {
                    ShareData::Lp { r1, r2, p: data_p } => {
                        if *p != data_p {
                            return Err(SharePhaseError::InvalidRange("Metric p value does not match data p value".to_string()));
                        }
                        match self.config.dictionary_type {
                            DictionaryType::Known => {
                                self.share_with_okvs_known_lp(&x, &left_bound, &right_bound, &r1, &r2, *p)
                            },
                            DictionaryType::Unknown => {
                                self.share_with_okvs_unknown_lp(&x, &left_bound, &right_bound, &r1, &r2, *p)
                            },
                        }
                    },
                    _ => return Err(SharePhaseError::InvalidRange("Lp data not provided for Lp distance".to_string())),
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
                if *p > 5 {
                    return Err(SharePhaseError::InvalidRange("Distance FSS only supports p <= 5".to_string()));
                }
                // Distance FSS method is the same for both known and unknown dictionary
                // since FSS already handles evaluating on prefixes
                self.share_with_distance_fss(&x, &left_bound, &right_bound, *p)
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
            SharedRange::OKVS { okvs_shares, role } => {
                let result = self.evaluate_okvs_at_single_dimension(&okvs_shares[dimension], point_bits, *role)?;
                Ok(result)
            }
            SharedRange::IntervalFSS { fss_key, role } => {
                self.evaluate_interval_fss_at_single_dimension(&fss_key[dimension], point_bits, *role)
            }
            SharedRange::OKVSLp { okvs_shares, role, p } => {
                let result = self.evaluate_okvs_lp_at_single_dimension(&okvs_shares[dimension], point_bits, *role, *p)?;
                Ok(result)
            }
            SharedRange::DistanceFSS1 { fss_keys, role } => {
                let result = self.evaluate_distance_fss1_at_single_dimension(&fss_keys[dimension], point_bits, *role)?;
                Ok(result)
            }
            SharedRange::DistanceFSS2 { fss_keys, role } => {
                let result = self.evaluate_distance_fss2_at_single_dimension(&fss_keys[dimension], point_bits, *role)?;
                Ok(result)
            }
            SharedRange::DistanceFSS3 { fss_keys, role } => {
                let result = self.evaluate_distance_fss3_at_single_dimension(&fss_keys[dimension], point_bits, *role)?;
                Ok(result)
            }
            SharedRange::DistanceFSS4 { fss_keys, role } => {
                let result = self.evaluate_distance_fss4_at_single_dimension(&fss_keys[dimension], point_bits, *role)?;
                Ok(result)
            }
            SharedRange::DistanceFSS5 { fss_keys, role } => {
                let result = self.evaluate_distance_fss5_at_single_dimension(&fss_keys[dimension], point_bits, *role)?;
                Ok(result)
            }
        }
    }


    /// Share using OKVS method for known dictionary
    /// For each dimension i, create an OKVS that maps keys in [left_bound[i], right_bound[i]] to output_bit_length-sized vectors
    fn share_with_okvs_known(
        &self, 
        x: &[u128], // The original d-dimensional vector
        left_bound: &Vec<u128>, 
        right_bound: &Vec<u128>,
        r1: &[u8; 16],
        r2: &[u8; 16],
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut okvs_shares_0 = Vec::new();
        let mut okvs_shares_1 = Vec::new();
        let modulus_mask = (1u128 << self.config.output_bit_length) - 1;

        // Create separate OKVS for each dimension
        for dim in 0..self.config.dimension {
            let left = left_bound[dim];
            let right = right_bound[dim];
            println!("x: {:?}, left: {}, right: {}", x, left, right);
            
            // Create key-value pairs for this dimension's range [left, right]
            let mut keys = Vec::new();
            let mut values_0 = Vec::new();
            let mut values_1 = Vec::new();

            for key in left..=right {
                let mut key_bits = u128_to_bits(key, self.config.input_bit_length);
                key_bits.reverse();
                
                let value = rand::rng().random::<u128>() & modulus_mask;
                // Generate u128 from Blake3 hash of key_bits
                let key_bits_bytes: Vec<u8> = key_bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
                let hash = blake3::hash(&key_bits_bytes);
                let hash_bytes = hash.as_bytes();
                let value_mask = u128::from_le_bytes([
                    hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
                    hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
                    hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
                    hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
                ]) & modulus_mask;
                
                // For now, use simple secret sharing where both shares are identical
                let share0 = value;
                let share1 = (value + value_mask) & modulus_mask; // share0 XOR share1 = value
                
                keys.push(key_bits);
                values_0.push(share0);
                values_1.push(share1);
            }

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
                &r1, 
                &r2,
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
            },
            SharedRange::OKVS {
                okvs_shares: okvs_shares_1,
                role: true, // Server 1
            },
        ))
    }

    /// Helper: Evaluate OKVS at a single dimension (for backward compatibility)
    fn evaluate_okvs_at_single_dimension(
        &self,
        okvs_share: &Vec<u128>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseError> {
        let mut key_bits = point_bits.to_vec();
        let modulus_mask = (1u128 << self.config.output_bit_length) - 1;
        
        match &self.config.data {
            ShareData::OKVS { r1, r2 } => {
                let okvs = RbOkvsF2k::<u128>::new(
                    1,
                    okvs_share.len(),
                    55, // Band width
                    &r1, 
                    &r2,
                );
                let result = okvs.decode(&okvs_share, &[key_bits.clone()]);
                if result.is_empty() {
                    Ok(0) // Return 0 if decode fails
                } else {
                    if !role {
                        Ok(result[0])
                    } else {
                        // Generate u128 from Blake3 hash of key_bits
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
            },
            _ => return Err(SharePhaseError::InvalidRange("OKVS data not provided".to_string())),
        }
    }

    /// Share using OKVS method for unknown dictionary
    /// For each dimension i, create an OKVS that maps all prefixes of keys in [left_bound[i], right_bound[i]] to output_bit_length-sized vectors
    fn share_with_okvs_unknown(
        &self, 
        x: &[u128], // The original d-dimensional vector
        left_bound: &Vec<u128>, 
        right_bound: &Vec<u128>,
        r1: &[u8; 16],
        r2: &[u8; 16],
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        println!("Creating OKVS for unknown dictionary with bounds: left={:?}, right={:?}", left_bound, right_bound);

        let mut okvs_shares_0 = Vec::new();
        let mut okvs_shares_1 = Vec::new();
        let modulus_mask = (1u128 << self.config.output_bit_length) - 1;

        // Create separate OKVS for each dimension
        for dim in 0..self.config.dimension {
            let left = left_bound[dim];
            let right = right_bound[dim];
            println!("Unknown dictionary - x: {:?}, left: {}, right: {}", x, left, right);
            
            // Create key-value pairs for all prefixes of values in this dimension's range [left, right]
            let mut keys = Vec::new();
            let mut values_0 = Vec::new();
            let mut values_1 = Vec::new();

            // Use HashSet to efficiently collect distinct prefixes
            let mut distinct_prefixes = HashSet::new();

            // Generate all distinct prefixes for values in the range [left, right]
            for value in left..=right {
                for prefix_len in 1..=self.config.input_bit_length {
                    let prefix = value >> (self.config.input_bit_length - prefix_len);
                    // Create a compact representation for the prefix with its length
                    distinct_prefixes.insert((prefix, prefix_len));
                }
            }

            // Convert distinct prefixes to keys and generate corresponding values
            for (prefix, prefix_len) in distinct_prefixes {
                let mut prefix_bits = u128_to_bits(prefix, prefix_len);
                prefix_bits.reverse();

                println!("Prefix bits: {:?}", prefix_bits);
                
                let value = rand::rng().random::<u128>() & modulus_mask;
                // Generate u128 from Blake3 hash of prefix_bits
                let prefix_bits_bytes: Vec<u8> = prefix_bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
                let hash = blake3::hash(&prefix_bits_bytes);
                let hash_bytes = hash.as_bytes();
                let value_mask = u128::from_le_bytes([
                    hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
                    hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
                    hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
                    hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
                ]) & modulus_mask;
                
                // For now, use simple secret sharing where both shares are identical
                let share0 = value;
                let share1 = (value + value_mask) & modulus_mask; // share0 XOR share1 = value
                
                keys.push(prefix_bits);
                values_0.push(share0);
                values_1.push(share1);
            }

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
                &r1, 
                &r2,
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
            },
            SharedRange::OKVS {
                okvs_shares: okvs_shares_1,
                role: true, // Server 1
            },
        ))
    }


    /// Share using Interval FSS method (N=1)
    fn share_with_interval_fss(
        &self,
        left_bound: &Vec<u128>,
        right_bound: &Vec<u128>
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut fss_0 = Vec::new();
        let mut fss_1 = Vec::new();

        // Use output_bit_length to determine the modulus
        let modulus = 1u128 << self.config.output_bit_length;

        for (&alpha, &beta) in left_bound.iter().zip(right_bound.iter()) {
            if alpha >= beta {
                return Err(SharePhaseError::InvalidRange(
                    format!("Left bound ({}) must be less than right bound ({})", alpha, beta)
                ));
            }

            // Convert bounds to bit representations
            let mut alpha_bits = u128_to_bits(alpha, self.config.input_bit_length);
            alpha_bits.reverse();
            let mut beta_bits = u128_to_bits(beta, self.config.input_bit_length);
            beta_bits.reverse();

            // Create payload vectors with N=1: left=1, mid=0, right=1
            let left_payload = RingVec::<1>::new([1], modulus);
            let mid_payload = RingVec::<1>::zero(modulus);
            let right_payload = RingVec::<1>::new([1], modulus);

            // Generate interval FSS keys with N=1
            let (fss_key_0, fss_key_1) = IntervalFSSKey::gen_IntervalFSSKey(
                &alpha_bits,
                &beta_bits, 
                left_payload,
                mid_payload,
                right_payload,
                modulus
            );

            fss_0.push(fss_key_0);
            fss_1.push(fss_key_1);
        }

        Ok((
            SharedRange::IntervalFSS {
                fss_key: fss_0,
                role: false, // Server 0
            },
            SharedRange::IntervalFSS {
                fss_key: fss_1,
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
        let key_bits = point_bits.to_vec();
        
        // Use output_bit_length to determine the modulus
        let modulus = 1u128 << self.config.output_bit_length;
        
        // Evaluate with the FSS key for the specified dimension
        let result = fss_key.eval_intervalFSS(&key_bits, modulus);
        // Return the result value
        Ok(result[0])
    }

    /// Share using OKVS method for known dictionary in Lp case
    /// For each dimension i, create an OKVS that maps keys in [left_bound[i], right_bound[i]] 
    /// to secret shares of |key - x_i|^p
    fn share_with_okvs_known_lp(
        &self, 
        x: &[u128], // The original d-dimensional vector
        left_bound: &Vec<u128>, 
        right_bound: &Vec<u128>,
        r1: &[u8; 16],
        r2: &[u8; 16],
        p: u32,
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut okvs_shares_0 = Vec::new();
        let mut okvs_shares_1 = Vec::new();
        let modulus_mask = (1u128 << self.config.output_bit_length) - 1;

        // Create separate OKVS for each dimension
        for dim in 0..self.config.dimension {
            let left = left_bound[dim];
            let right = right_bound[dim];
            let x_i = x[dim];
            
            println!("Lp known - dim {}: x_i={}, left={}, right={}", dim, x_i, left, right);
            
            // Create key-value pairs for this dimension's range [left, right]
            let mut keys = Vec::new();
            let mut values_0 = Vec::new();
            let mut values_1 = Vec::new();

            for key in left..=right {
                let mut key_bits = u128_to_bits(key, self.config.input_bit_length);
                key_bits.reverse();
                
                // Calculate |key - x_i|^p
                let distance = if key >= x_i { key - x_i } else { x_i - key };
                let distance_p = Self::compute_power_static(distance, p, self.config.output_bit_length) & modulus_mask;
                
                // Generate random value for secret sharing
                let random_value = rand::rng().random::<u128>() & modulus_mask;
                
                // Generate deterministic mask from key bits using Blake3
                let key_bits_bytes: Vec<u8> = key_bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
                let hash = blake3::hash(&key_bits_bytes);
                let hash_bytes = hash.as_bytes();
                let value_mask = u128::from_le_bytes([
                    hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
                    hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
                    hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
                    hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
                ]) & modulus_mask;
                
                // Secret sharing: share0 gets random_value, share1 gets |key - x_i|^p - random_value + mask
                let share0 = random_value;
                let share1 = (distance_p + modulus_mask + 1 - random_value + value_mask) & modulus_mask;
                
                keys.push(key_bits);
                values_0.push(share0);
                values_1.push(share1);
            }

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
            SharedRange::OKVSLp {
                okvs_shares: okvs_shares_0,
                role: false, // Server 0
                p,
            },
            SharedRange::OKVSLp {
                okvs_shares: okvs_shares_1,
                role: true, // Server 1
                p,
            },
        ))
    }

    /// Share using OKVS method for unknown dictionary in Lp case
    /// For each dimension i, create an OKVS that maps all prefixes of keys in [left_bound[i], right_bound[i]] 
    /// to secret shares of the appropriate distance based on prefix rules
    fn share_with_okvs_unknown_lp(
        &self, 
        x: &[u128], // The original d-dimensional vector
        left_bound: &Vec<u128>, 
        right_bound: &Vec<u128>,
        r1: &[u8; 16],
        r2: &[u8; 16],
        p: u32,
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        println!("Creating OKVS for Lp unknown dictionary with bounds: left={:?}, right={:?}", left_bound, right_bound);

        let mut okvs_shares_0 = Vec::new();
        let mut okvs_shares_1 = Vec::new();
        let modulus_mask = (1u128 << self.config.output_bit_length) - 1;

        // Create separate OKVS for each dimension
        for dim in 0..self.config.dimension {
            let left = left_bound[dim];
            let right = right_bound[dim];
            let x_i = x[dim];
            
            println!("Lp unknown - dim {}: x_i={}, left={}, right={}", dim, x_i, left, right);
            
            // Create key-value pairs for all prefixes of values in this dimension's range [left, right]
            let mut keys = Vec::new();
            let mut values_0 = Vec::new();
            let mut values_1 = Vec::new();

            // Use HashSet to efficiently collect distinct prefixes
            let mut distinct_prefixes = HashSet::new();

            // Generate all distinct prefixes for values in the range [left, right]
            for value in left..=right {
                for prefix_len in 1..=self.config.input_bit_length {
                    let prefix = value >> (self.config.input_bit_length - prefix_len);
                    // Create a compact representation for the prefix with its length
                    distinct_prefixes.insert((prefix, prefix_len));
                }
            }

            // Convert distinct prefixes to keys and generate corresponding values
            for (prefix, prefix_len) in distinct_prefixes {
                let mut prefix_bits = u128_to_bits(prefix, prefix_len);
                prefix_bits.reverse();

                println!("Prefix bits: {:?}, prefix_len: {}", prefix_bits, prefix_len);
                
                // Calculate the distance based on the prefix rules:
                // 1. If prefix is a prefix of x_i, distance = 0
                // 2. If prefix < x_i (lexicographically), closest string is prefix followed by all 1s
                // 3. If prefix > x_i (lexicographically), closest string is prefix followed by all 0s
                
                let distance_p = if Self::is_prefix_of_static(prefix, prefix_len, x_i, self.config.input_bit_length) {
                    // Case 1: prefix is a prefix of x_i, distance = 0
                    0u128
                } else {
                    // Get the prefix of x_i with the same length
                    let x_i_prefix = x_i >> (self.config.input_bit_length - prefix_len);
                    
                    if prefix < x_i_prefix {
                        // Case 2: prefix < x_i prefix, closest string is prefix + all 1s
                        let closest_string = prefix << (self.config.input_bit_length - prefix_len) |
                                           ((1u128 << (self.config.input_bit_length - prefix_len)) - 1);
                        let distance = x_i - closest_string; // x_i > closest_string always in this case
                        Self::compute_power_static(distance, p, self.config.output_bit_length) & modulus_mask
                    } else {
                        // Case 3: prefix > x_i prefix, closest string is prefix + all 0s
                        let closest_string = prefix << (self.config.input_bit_length - prefix_len);
                        let distance = closest_string - x_i; // closest_string > x_i always in this case
                        Self::compute_power_static(distance, p, self.config.output_bit_length) & modulus_mask
                    }
                };
                
                // Generate random value for secret sharing
                let random_value = rand::rng().random::<u128>() & modulus_mask;
                
                // Generate deterministic mask from prefix bits using Blake3
                let prefix_bits_bytes: Vec<u8> = prefix_bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
                let hash = blake3::hash(&prefix_bits_bytes);
                let hash_bytes = hash.as_bytes();
                let value_mask = u128::from_le_bytes([
                    hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
                    hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
                    hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
                    hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
                ]) & modulus_mask;
                
                // Secret sharing: share0 gets random_value, share1 gets distance_p - random_value + mask
                let share0 = random_value;
                let share1 = (distance_p + modulus_mask + 1 - random_value + value_mask) & modulus_mask;
                
                keys.push(prefix_bits);
                values_0.push(share0);
                values_1.push(share1);
            }

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
            SharedRange::OKVSLp {
                okvs_shares: okvs_shares_0,
                role: false, // Server 0
                p,
            },
            SharedRange::OKVSLp {
                okvs_shares: okvs_shares_1,
                role: true, // Server 1
                p,
            },
        ))
    }

    /// Helper: Evaluate OKVS at a single dimension for Lp distance
    /// Returns the secret share of |point - x_i|^p for the specified dimension
    fn evaluate_okvs_lp_at_single_dimension(
        &self,
        okvs_share: &Vec<u128>,
        point_bits: &[bool],
        role: bool,
        p: u32,
    ) -> Result<u128, SharePhaseError> {
        let key_bits = point_bits.to_vec();
        let modulus_mask = (1u128 << self.config.output_bit_length) - 1;
        
        match &self.config.data {
            ShareData::Lp { r1, r2, .. } => {
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
            },
            _ => return Err(SharePhaseError::EvaluationError("Lp data not provided for Lp evaluation".to_string())),
        }
    }

    /// Compute base^exponent modulo 2^output_bit_length
    /// Uses fast exponentiation for efficiency
    fn compute_power_static(base: u128, exponent: u32, output_bit_length: usize) -> u128 {
        if exponent == 0 {
            return 1;
        }
        if exponent == 1 {
            return base;
        }
        
        let modulus_mask = (1u128 << output_bit_length) - 1;
        let mut result = 1u128;
        let mut base = base & modulus_mask;
        let mut exp = exponent;
        
        while exp > 0 {
            if exp % 2 == 1 {
                result = (result * base) & modulus_mask;
            }
            base = (base * base) & modulus_mask;
            exp /= 2;
        }
        
        result
    }

    /// Check if a given prefix is a prefix of the target value
    /// Returns true if the first prefix_len bits of target match the prefix
    fn is_prefix_of_static(prefix: u128, prefix_len: usize, target: u128, input_bit_length: usize) -> bool {
        if prefix_len == 0 {
            return true; // Empty prefix is always a prefix
        }
        if prefix_len > input_bit_length {
            return false; // Prefix longer than input length
        }
        
        // Extract the first prefix_len bits of target
        let target_prefix = target >> (input_bit_length - prefix_len);
        prefix == target_prefix
    }

    /// Share using Distance FSS method for Lp distance
    /// For each dimension i, create a Distance FSS that computes |y - x_i|^p
    /// Both known and unknown dictionary cases use the same implementation since FSS handles prefixes
    fn share_with_distance_fss(
        &self,
        x: &[u128], // The original d-dimensional vector
        left_bound: &Vec<u128>,
        right_bound: &Vec<u128>,
        p: u32,
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        if p > 5 {
            return Err(SharePhaseError::InvalidRange("Distance FSS only supports p <= 5".to_string()));
        }

        // Use output_bit_length to determine the modulus
        let modulus = 1u128 << self.config.output_bit_length;

        match p {
            1 => {
                let mut fss_keys_0 = Vec::new();
                let mut fss_keys_1 = Vec::new();

                // Create separate Distance FSS for each dimension
                for dim in 0..self.config.dimension {
                    let left = left_bound[dim];
                    let right = right_bound[dim];
                    let x_i = x[dim];

                    println!("Distance FSS p=1 - dim {}: x_i={}, left={}, right={}", dim, x_i, left, right);

                    if left >= right {
                        return Err(SharePhaseError::InvalidRange(
                            format!("Left bound ({}) must be less than right bound ({})", left, right)
                        ));
                    }

                    // Convert bounds and x_i to bit representations
                    let mut x_i_bits = u128_to_bits(x_i, self.config.input_bit_length);
                    x_i_bits.reverse();
                    let mut left_bits = u128_to_bits(left, self.config.input_bit_length);
                    left_bits.reverse();
                    let mut right_bits = u128_to_bits(right, self.config.input_bit_length);
                    right_bits.reverse();

                    // Generate distance FSS keys with N = p + 1 = 2
                    let (fss_key_0, fss_key_1) = DistanceFSSKey::<2>::gen_distance_fss_key(x_i, &x_i_bits, &left_bits, &right_bits, modulus);

                    fss_keys_0.push(fss_key_0);
                    fss_keys_1.push(fss_key_1);
                }

                Ok((
                    SharedRange::DistanceFSS1 {
                        fss_keys: fss_keys_0,
                        role: false, // Server 0
                    },
                    SharedRange::DistanceFSS1 {
                        fss_keys: fss_keys_1,
                        role: true, // Server 1
                    },
                ))
            },
            2 => {
                let mut fss_keys_0 = Vec::new();
                let mut fss_keys_1 = Vec::new();

                for dim in 0..self.config.dimension {
                    let left = left_bound[dim];
                    let right = right_bound[dim];
                    let x_i = x[dim];

                    let mut x_i_bits = u128_to_bits(x_i, self.config.input_bit_length);
                    x_i_bits.reverse();
                    let mut left_bits = u128_to_bits(left, self.config.input_bit_length);
                    left_bits.reverse();
                    let mut right_bits = u128_to_bits(right, self.config.input_bit_length);
                    right_bits.reverse();

                    let (fss_key_0, fss_key_1) = DistanceFSSKey::<3>::gen_distance_fss_key(x_i, &x_i_bits, &left_bits, &right_bits, modulus);

                    fss_keys_0.push(fss_key_0);
                    fss_keys_1.push(fss_key_1);
                }

                Ok((
                    SharedRange::DistanceFSS2 {
                        fss_keys: fss_keys_0,
                        role: false,
                    },
                    SharedRange::DistanceFSS2 {
                        fss_keys: fss_keys_1,
                        role: true,
                    },
                ))
            },
            3 => {
                let mut fss_keys_0 = Vec::new();
                let mut fss_keys_1 = Vec::new();

                for dim in 0..self.config.dimension {
                    let left = left_bound[dim];
                    let right = right_bound[dim];
                    let x_i = x[dim];

                    let mut x_i_bits = u128_to_bits(x_i, self.config.input_bit_length);
                    x_i_bits.reverse();
                    let mut left_bits = u128_to_bits(left, self.config.input_bit_length);
                    left_bits.reverse();
                    let mut right_bits = u128_to_bits(right, self.config.input_bit_length);
                    right_bits.reverse();

                    let (fss_key_0, fss_key_1) = DistanceFSSKey::<4>::gen_distance_fss_key(x_i, &x_i_bits, &left_bits, &right_bits, modulus);

                    fss_keys_0.push(fss_key_0);
                    fss_keys_1.push(fss_key_1);
                }

                Ok((
                    SharedRange::DistanceFSS3 {
                        fss_keys: fss_keys_0,
                        role: false,
                    },
                    SharedRange::DistanceFSS3 {
                        fss_keys: fss_keys_1,
                        role: true,
                    },
                ))
            },
            4 => {
                let mut fss_keys_0 = Vec::new();
                let mut fss_keys_1 = Vec::new();

                for dim in 0..self.config.dimension {
                    let left = left_bound[dim];
                    let right = right_bound[dim];
                    let x_i = x[dim];

                    let mut x_i_bits = u128_to_bits(x_i, self.config.input_bit_length);
                    x_i_bits.reverse();
                    let mut left_bits = u128_to_bits(left, self.config.input_bit_length);
                    left_bits.reverse();
                    let mut right_bits = u128_to_bits(right, self.config.input_bit_length);
                    right_bits.reverse();

                    let (fss_key_0, fss_key_1) = DistanceFSSKey::<5>::gen_distance_fss_key(x_i, &x_i_bits, &left_bits, &right_bits, modulus);

                    fss_keys_0.push(fss_key_0);
                    fss_keys_1.push(fss_key_1);
                }

                Ok((
                    SharedRange::DistanceFSS4 {
                        fss_keys: fss_keys_0,
                        role: false,
                    },
                    SharedRange::DistanceFSS4 {
                        fss_keys: fss_keys_1,
                        role: true,
                    },
                ))
            },
            5 => {
                let mut fss_keys_0 = Vec::new();
                let mut fss_keys_1 = Vec::new();

                for dim in 0..self.config.dimension {
                    let left = left_bound[dim];
                    let right = right_bound[dim];
                    let x_i = x[dim];

                    let mut x_i_bits = u128_to_bits(x_i, self.config.input_bit_length);
                    x_i_bits.reverse();
                    let mut left_bits = u128_to_bits(left, self.config.input_bit_length);
                    left_bits.reverse();
                    let mut right_bits = u128_to_bits(right, self.config.input_bit_length);
                    right_bits.reverse();

                    let (fss_key_0, fss_key_1) = DistanceFSSKey::<6>::gen_distance_fss_key(x_i, &x_i_bits, &left_bits, &right_bits, modulus);

                    fss_keys_0.push(fss_key_0);
                    fss_keys_1.push(fss_key_1);
                }

                Ok((
                    SharedRange::DistanceFSS5 {
                        fss_keys: fss_keys_0,
                        role: false,
                    },
                    SharedRange::DistanceFSS5 {
                        fss_keys: fss_keys_1,
                        role: true,
                    },
                ))
            },
            _ => return Err(SharePhaseError::InvalidRange("Distance FSS only supports p in range 1-5".to_string())),
        }
    }

    /// Helper: Evaluate Distance FSS at a single dimension (p=1)
    fn evaluate_distance_fss1_at_single_dimension(
        &self,
        fss_key: &DistanceFSSKey<2>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.output_bit_length;
        let result = fss_key.eval_distance_fss(point_bits, self.config.input_bit_length, modulus);
        Ok(result)
    }

    /// Helper: Evaluate Distance FSS at a single dimension (p=2)
    fn evaluate_distance_fss2_at_single_dimension(
        &self,
        fss_key: &DistanceFSSKey<3>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.output_bit_length;
        let result = fss_key.eval_distance_fss(point_bits, self.config.input_bit_length, modulus);
        Ok(result)
    }

    /// Helper: Evaluate Distance FSS at a single dimension (p=3)
    fn evaluate_distance_fss3_at_single_dimension(
        &self,
        fss_key: &DistanceFSSKey<4>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.output_bit_length;
        let result = fss_key.eval_distance_fss(point_bits, self.config.input_bit_length, modulus);
        Ok(result)
    }

    /// Helper: Evaluate Distance FSS at a single dimension (p=4)
    fn evaluate_distance_fss4_at_single_dimension(
        &self,
        fss_key: &DistanceFSSKey<5>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.output_bit_length;
        let result = fss_key.eval_distance_fss(point_bits, self.config.input_bit_length, modulus);
        Ok(result)
    }

    /// Helper: Evaluate Distance FSS at a single dimension (p=5)
    fn evaluate_distance_fss5_at_single_dimension(
        &self,
        fss_key: &DistanceFSSKey<6>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseError> {
        let modulus = 1u128 << self.config.output_bit_length;
        let result = fss_key.eval_distance_fss(point_bits, self.config.input_bit_length, modulus);
        Ok(result)
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
