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
use crate::data_structures::{field::FieldElm, payload::RingVec};
use crate::util::u128_to_bits;
use serde::{Deserialize, Serialize};

/// Enumeration of different sharing methods available
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareMethod {
    /// Use OKVS for sharing
    OKVS,
    /// Use Interval FSS for sharing
    IntervalFSS,
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
    IntervalFSS {
    }
}

/// Configuration for the share phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareConfig {
    /// The sharing method to use
    pub method: ShareMethod,
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
    /// OKVS-based sharing with one OKVS per dimension
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

        match self.config.method {
            ShareMethod::OKVS => {
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
                    _ => return Err(SharePhaseError::InvalidRange("OKVS data not provided".to_string())),
                }
            },
            ShareMethod::IntervalFSS => {
                // IntervalFSS method is the same for both known and unknown dictionary
                // since FSS already handles evaluating on prefixes
                self.share_with_interval_fss(&left_bound, &right_bound)
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
