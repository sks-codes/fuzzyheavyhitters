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

use crate::okvs_f2k;
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

/// Configuration for the share phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareConfig {
    /// The sharing method to use
    pub method: ShareMethod,
    /// Number of bits for representing input values (u)
    pub input_bit_length: usize,
    /// Number of bits for output values (v) - output will be mod 2^v
    pub output_bit_length: usize,
    /// Modulus for ring operations (must be power of 2)
    pub modulus: u128,
}

/// Represents the shared data for a range around input x
#[derive(Debug, Clone)]
pub enum SharedRange {
    /// OKVS-based sharing with two OKVS for secret sharing mod 2^v
    OKVS {
        /// The first OKVS share
        okvs_share_0: Vec<u128>,
        /// The second OKVS share  
        okvs_share_1: Vec<u128>,
        /// Range parameters
        center: u128,
        delta: u128,
    },
    /// Interval FSS-based sharing (N=1)
    IntervalFSS {
        /// The interval FSS keys (two shares) with N=1
        fss_keys: (IntervalFSSKey<1>, IntervalFSSKey<1>),
        /// Range parameters  
        center: u128,
        delta: u128,
    },
}

/// Share phase handler
pub struct SharePhase {
    config: ShareConfig,
}

impl SharePhase {
    /// Create a new share phase with the given configuration
    pub fn new(config: ShareConfig) -> Self {
        Self { config }
    }

    /// Share a range [x-delta, x+delta] using the configured method
    /// Input x is in range [0, 2^u-1], output will be mod 2^v
    pub fn share_range(&self, x: u128, delta: u128) -> Result<SharedRange, SharePhaseError> {
        // Calculate the maximum value for u bits
        let max_input = (1u128 << self.config.input_bit_length) - 1;
        
        // Validate that x is within the valid range
        if x > max_input {
            return Err(SharePhaseError::InvalidRange(
                format!("Input x ({}) exceeds maximum value for {}-bit input ({})", 
                        x, self.config.input_bit_length, max_input)
            ));
        }

        // Calculate bounds, clamping to valid range [0, 2^u-1]
        let left_bound = x.saturating_sub(delta).max(0);
        let right_bound = (x.saturating_add(delta)).min(max_input);

        match self.config.method {
            ShareMethod::OKVS => self.share_with_okvs(x, delta, left_bound, right_bound),
            ShareMethod::IntervalFSS => self.share_with_interval_fss(x, delta, left_bound, right_bound),
        }
    }

    /// Share using OKVS method
    fn share_with_okvs(
        &self, 
        x: u128, 
        delta: u128, 
        left_bound: u128, 
        right_bound: u128
    ) -> Result<SharedRange, SharePhaseError> {
        // Convert bounds to bit representations
        let left_bits = self.u128_to_bits(left_bound);
        let right_bits = self.u128_to_bits(right_bound);

        // Create key-value pairs for the range [left_bound, right_bound]
        let mut key_value_pairs = Vec::new();
        for key in left_bound..=right_bound {
            let key_bits = self.u128_to_bits(key);
            // We want secret shares of 1 mod 2^v
            let value = 1u128;
            key_value_pairs.push((key_bits, value));
        }

        // Create two OKVS shares for secret sharing mod 2^v
        let (okvs_share_0, okvs_share_1) = self.encode_okvs_shares(&key_value_pairs)?;

        Ok(SharedRange::OKVS {
            okvs_share_0,
            okvs_share_1,
            center: x,
            delta,
        })
    }

    /// Share using Interval FSS method (N=1)
    fn share_with_interval_fss(
        &self,
        x: u128,
        delta: u128,
        left_bound: u128,
        right_bound: u128
    ) -> Result<SharedRange, SharePhaseError> {
        // Convert bounds to bit representations
        let alpha_bits = self.u128_to_bits(left_bound);
        let beta_bits = self.u128_to_bits(right_bound);

        // Create payload vectors with N=1: left=1, mid=0, right=1
        let left_payload = RingVec::<1>::one(self.config.modulus);
        let mid_payload = RingVec::<1>::zero(self.config.modulus);
        let right_payload = RingVec::<1>::one(self.config.modulus);

        // Generate interval FSS keys with N=1
        let (fss_key_0, fss_key_1) = IntervalFSSKey::gen_IntervalFSSKey(
            &alpha_bits,
            &beta_bits, 
            left_payload,
            mid_payload,
            right_payload,
            self.config.modulus
        );

        Ok(SharedRange::IntervalFSS {
            fss_keys: (fss_key_0, fss_key_1),
            center: x,
            delta,
        })
    }

    /// Convert u128 to bit vector with configured input bit length
    fn u128_to_bits(&self, value: u128) -> Vec<bool> {
        let mut bits = Vec::with_capacity(self.config.input_bit_length);
        for i in 0..self.config.input_bit_length {
            bits.push((value & (1u128 << i)) != 0);
        }
        bits
    }

    /// Encode key-value pairs into two OKVS shares for secret sharing mod 2^v
    fn encode_okvs_shares(&self, key_value_pairs: &[(Vec<bool>, u128)]) -> Result<(Vec<u128>, Vec<u128>), SharePhaseError> {
        // TODO: Implement actual OKVS encoding using the okvs_f2k module
        // For now, return placeholder shares
        let encoded_size = key_value_pairs.len().next_power_of_two();
        let mut okvs_share_0 = vec![0u128; encoded_size];
        let mut okvs_share_1 = vec![0u128; encoded_size];
        
        // Placeholder encoding: create secret shares mod 2^v of 1
        // share_0 + share_1 = 1 (mod 2^v)
        let output_modulus = 1u128 << self.config.output_bit_length;
        let output_mask = output_modulus - 1;
        
        for (i, (_, _)) in key_value_pairs.iter().enumerate() {
            if i < okvs_share_0.len() {
                // Create random share_0 and set share_1 = 1 - share_0 (mod 2^v)
                let share_0_val = rand::random::<u128>() & output_mask;
                let share_1_val = (1u128.wrapping_sub(share_0_val)) & output_mask;
                
                okvs_share_0[i] = share_0_val;
                okvs_share_1[i] = share_1_val;
            }
        }

        Ok((okvs_share_0, okvs_share_1))
    }

    /// Evaluate the shared range at a specific point
    pub fn evaluate_at(&self, shared_range: &SharedRange, point: u128) -> Result<RingVec<1>, SharePhaseError> {
        match shared_range {
            SharedRange::OKVS { okvs_share_0, okvs_share_1, center, delta } => {
                self.evaluate_okvs_at(okvs_share_0, okvs_share_1, *center, *delta, point)
            },
            SharedRange::IntervalFSS { fss_keys, center, delta } => {
                self.evaluate_interval_fss_at(fss_keys, *center, *delta, point)
            },
        }
    }

    /// Evaluate OKVS at a specific point
    fn evaluate_okvs_at(
        &self,
        okvs_share_0: &[u128],
        okvs_share_1: &[u128],
        center: u128,
        delta: u128,
        point: u128
    ) -> Result<RingVec<1>, SharePhaseError> {
        let max_input = (1u128 << self.config.input_bit_length) - 1;
        let left_bound = center.saturating_sub(delta).max(0);
        let right_bound = (center.saturating_add(delta)).min(max_input);

        // Check if point is in range
        if point >= left_bound && point <= right_bound {
            // TODO: Implement actual OKVS evaluation
            // For now, return a vector with value 1 mod 2^v
            let output_modulus = 1u128 << self.config.output_bit_length;
            Ok(RingVec::<1>::one(output_modulus))
        } else {
            // Point is outside range, return 0
            let output_modulus = 1u128 << self.config.output_bit_length;
            Ok(RingVec::<1>::zero(output_modulus))
        }
    }

    /// Evaluate Interval FSS at a specific point
    fn evaluate_interval_fss_at(
        &self,
        fss_keys: &(IntervalFSSKey<1>, IntervalFSSKey<1>),
        center: u128,
        delta: u128,
        point: u128
    ) -> Result<RingVec<1>, SharePhaseError> {
        let point_bits = self.u128_to_bits(point);
        
        // Evaluate with the first key (could be either key depending on which party is evaluating)
        let result = fss_keys.0.eval_intervalFSS(&point_bits, self.config.modulus);
        Ok(result)
    }

    /// Get the range bounds for a shared range
    pub fn get_range_bounds(shared_range: &SharedRange, input_bit_length: usize) -> (u128, u128, u128, u128) {
        let max_input = (1u128 << input_bit_length) - 1;
        match shared_range {
            SharedRange::OKVS { center, delta, .. } => {
                let left_bound = center.saturating_sub(*delta).max(0);
                let right_bound = (center.saturating_add(*delta)).min(max_input);
                (*center, *delta, left_bound, right_bound)
            },
            SharedRange::IntervalFSS { center, delta, .. } => {
                let left_bound = center.saturating_sub(*delta).max(0);
                let right_bound = (center.saturating_add(*delta)).min(max_input);
                (*center, *delta, left_bound, right_bound)
            },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_share_phase_interval_fss() {
        let config = ShareConfig {
            method: ShareMethod::IntervalFSS,
            input_bit_length: 16,  // u = 16
            output_bit_length: 8,  // v = 8, output mod 2^8
            modulus: 256, // 2^8
        };

        let share_phase = SharePhase::new(config);
        let x = 100u128;
        let delta = 10u128;

        let shared_range = share_phase.share_range(x, delta).unwrap();

        // Verify the shared range
        match shared_range {
            SharedRange::IntervalFSS { center, delta: d, .. } => {
                assert_eq!(center, x);
                assert_eq!(d, delta);
            },
            _ => panic!("Expected IntervalFSS variant"),
        }
    }

    #[test]
    fn test_share_phase_okvs() {
        let config = ShareConfig {
            method: ShareMethod::OKVS,
            input_bit_length: 16,  // u = 16
            output_bit_length: 8,  // v = 8, output mod 2^8
            modulus: 256,
        };

        let share_phase = SharePhase::new(config);
        let x = 50u128;
        let delta = 5u128;

        let shared_range = share_phase.share_range(x, delta).unwrap();

        // Verify the shared range
        match shared_range {
            SharedRange::OKVS { center, delta: d, .. } => {
                assert_eq!(center, x);
                assert_eq!(d, delta);
            },
            _ => panic!("Expected OKVS variant"),
        }
    }

    #[test]
    fn test_valid_range_with_large_delta() {
        let config = ShareConfig {
            method: ShareMethod::IntervalFSS,
            input_bit_length: 4,   // u = 4, max value = 15
            output_bit_length: 8,  // v = 8
            modulus: 256,
        };

        let share_phase = SharePhase::new(config);
        
        // Test case where delta > x (should be allowed, clamped to [0, 2^u-1])
        let result = share_phase.share_range(5u128, 10u128);
        assert!(result.is_ok());

        // Test case where x + delta > 2^u-1 (should be allowed, clamped)
        let result = share_phase.share_range(12u128, 10u128); // Range [2, 15] (clamped)
        assert!(result.is_ok());
    }

    #[test]
    fn test_input_exceeds_max() {
        let config = ShareConfig {
            method: ShareMethod::IntervalFSS,
            input_bit_length: 4,   // u = 4, max value = 15
            output_bit_length: 8,  // v = 8
            modulus: 256,
        };

        let share_phase = SharePhase::new(config);
        
        // Test case where x > 2^u-1 (should fail)
        let result = share_phase.share_range(20u128, 5u128); // x = 20 > 15
        assert!(result.is_err());
    }

    #[test]
    fn test_range_bounds() {
        let config = ShareConfig {
            method: ShareMethod::IntervalFSS,
            input_bit_length: 8,   // u = 8, max value = 255
            output_bit_length: 4,  // v = 4
            modulus: 256,
        };

        let share_phase = SharePhase::new(config);
        let x = 100u128;
        let delta = 10u128;

        let shared_range = share_phase.share_range(x, delta).unwrap();
        let (center, d, left_bound, right_bound) = SharePhase::get_range_bounds(&shared_range, 8);

        assert_eq!(center, x);
        assert_eq!(d, delta);
        assert_eq!(left_bound, x - delta);
        assert_eq!(right_bound, x + delta);
    }
}
