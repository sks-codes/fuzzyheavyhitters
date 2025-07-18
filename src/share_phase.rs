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
use rand;

use crate::okvs_f2k::{self, RbOkvsF2k};
use crate::fss::interval::{IntervalFSSKey, IntervalFSSEval};
use crate::data_structures::{field::FieldElm, payload::RingVec};
use serde::{Deserialize, Serialize};

/// Enumeration of different sharing methods available
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareMethod {
    /// Use OKVS for sharing
    OKVS,
    /// Use Interval FSS for sharing
    IntervalFSS,
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
    /// OKVS-based sharing with one OKVS per dimension for secret sharing mod 2^v
    OKVS {
        okvs_shares: Vec<Vec<u128>>, // One OKVS encoding per dimension
    },
    /// Interval FSS-based sharing (N=1)
    IntervalFSS {
        fss_key: Vec<IntervalFSSKey<1>>,
    },
}


/// Share phase handler
#[derive(Clone)]
pub struct SharePhase {
    config: ShareConfig,
}

impl SharePhase {
    /// Create a new share phase with the given configuration
    pub fn new(config: ShareConfig) -> Self {
        Self { config }
    }

    /// Share a range [x-delta, x+delta] using the configured method
    /// Input x is in range [0, 2^u-1], output will always be bool
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
                        self.share_with_okvs(&left_bound, &right_bound, &r1, &r2)
                    },
                    _ => return Err(SharePhaseError::InvalidRange("OKVS data not provided".to_string())),
                }
            },
            ShareMethod::IntervalFSS => self.share_with_interval_fss(&left_bound, &right_bound),
        }
    }

    /// Share using OKVS method
    fn share_with_okvs(
        &self, 
        left_bound: &Vec<u128>, 
        right_bound: &Vec<u128>,
        r1: &[u8; 16],
        r2: &[u8; 16],
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        let mut okvs_shares_0 = Vec::new();
        let mut okvs_shares_1 = Vec::new();

        // Create separate OKVS for each dimension
        for dim in 0..self.config.dimension {
            let left = left_bound[dim];
            let right = right_bound[dim];
            
            // Create key-value pairs for this dimension's range [left, right]
            let mut keys = Vec::new();
            let mut values_0 = Vec::new();
            let mut values_1 = Vec::new();

            for key in left..=right {
                let mut key_bits = u128_to_bits(key, self.config.input_bit_length);
                key_bits.reverse();
                
                // Generate random shares for the output value
                let share0 = rand::random::<u128>() & ((1u128 << self.config.output_bit_length) - 1);
                let share1 = share0; // For now, both shares are the same (representing value 1)
                
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
            },
            SharedRange::OKVS {
                okvs_shares: okvs_shares_1,
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
            },
            SharedRange::IntervalFSS {
                fss_key: fss_1,
            },
        ))
    }

    /// Evaluate the shared range at a specific point
    /// Returns the concatenated results from all dimensions
    pub fn evaluate_at(&self, shared_range: &SharedRange, point: u128, dim: usize) -> Result<Vec<u128>, SharePhaseError> {
        match shared_range {
            SharedRange::OKVS { okvs_shares } => {
                self.evaluate_okvs_at(okvs_shares, point, dim)
            },
            SharedRange::IntervalFSS { fss_key } => {
                self.evaluate_interval_fss_at(fss_key, point, dim)
            },
        }
    }

    /// Evaluate OKVS at a specific point
    /// Returns concatenated results from all dimensions
    fn evaluate_okvs_at(
        &self,
        okvs_shares: &[Vec<u128>],
        point: u128,
        dim: usize,
    ) -> Result<Vec<u128>, SharePhaseError> {
        if dim >= okvs_shares.len() {
            return Err(SharePhaseError::EvaluationError(
                format!("Dimension {} out of bounds for {} OKVS shares", dim, okvs_shares.len())
            ));
        }

        let mut point_bits = u128_to_bits(point, self.config.input_bit_length);
        point_bits.reverse();
        
        match &self.config.data {
            ShareData::OKVS { r1, r2 } => {
                let okvs_share = &okvs_shares[dim];
                if okvs_share.is_empty() {
                    return Ok(vec![0]); // Return 0 if no OKVS data for this dimension
                }

                let okvs = RbOkvsF2k::<u128>::new(
                    1,
                    okvs_share.len(),
                    55, // Band width
                    &r1, 
                    &r2,
                );
                let result = okvs.decode(okvs_share, &[point_bits]);
                if result.is_empty() {
                    return Ok(vec![0]); // Return 0 if decode fails
                }
                Ok(vec![result[0]])
            },
            _ => return Err(SharePhaseError::InvalidRange("OKVS data not provided".to_string())),
        }
    }

    /// Evaluate Interval FSS at a specific point
    /// Returns concatenated results from all dimensions
    fn evaluate_interval_fss_at(
        &self,
        fss_keys: &[IntervalFSSKey<1>],
        point: u128,
        dim: usize
    ) -> Result<Vec<u128>, SharePhaseError> {
        if dim >= fss_keys.len() {
            return Err(SharePhaseError::EvaluationError(
                format!("Dimension {} out of bounds for {} FSS keys", dim, fss_keys.len())
            ));
        }
        
        let mut point_bits = u128_to_bits(point, self.config.input_bit_length);
        point_bits.reverse();
        
        // Use output_bit_length to determine the modulus
        let modulus = 1u128 << self.config.output_bit_length;
        
        // Evaluate with the FSS key for the specified dimension
        let result = fss_keys[dim].eval_intervalFSS(&point_bits, modulus);
        // Return the result value
        Ok(vec![result[0]])
    }
}

/// Convert a u128 value to a vector of bits with specified bit length
fn u128_to_bits(value: u128, bit_length: usize) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bit_length);
    for i in 0..bit_length {
        bits.push((value >> i) & 1 == 1);
    }
    bits
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
