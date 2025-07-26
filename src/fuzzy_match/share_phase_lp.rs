//! Share Phase Lp Implementation
//! 
//! This module provides functionality for sharing OKVS (Oblivious Key-Value Store)
//! for the Lp distance case. In Lp distance, we want to test whether:
//! |x_1-y_1|^p + ... + |x_d - y_d|^p <= δ^p
//! 
//! The share phase allows a client with input x = (x_1, x_2, ..., x_d) and threshold delta
//! to share OKVS that encodes key-value pairs where:
//! - Keys are numbers in the range (x_i - delta, x_i + delta) for each dimension i
//! - Values are structured as: random_value and |key - x_i|^p - random_value
//! 
//! This allows the servers to later learn secret shares of |x_i - y_i|^p for each dimension.

use std::sync::Arc;
use std::cmp::max;
use std::collections::HashSet;
use rand::Rng;
use blake3;

use crate::okvs_f2k::{self, RbOkvsF2k};
use crate::data_structures::{field::FieldElm, payload::RingVec};
use crate::util::u128_to_bits;
use serde::{Deserialize, Serialize};

/// Enumeration of dictionary types for Lp distance
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DictionaryTypeLp {
    /// Known dictionary case - exact values in range
    Known,
    /// Unknown dictionary case - all prefixes of values in range
    Unknown,
}

/// Configuration data for Lp share phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareDataLp {
    /// Random seeds for OKVS construction
    pub r1: [u8; 16],
    pub r2: [u8; 16],
    /// The p value for Lp distance (e.g., p=1 for L1, p=2 for L2)
    pub p: u32,
}

/// Configuration for the Lp share phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareConfigLp {
    /// The dictionary type (known or unknown)
    pub dictionary_type: DictionaryTypeLp,
    /// Number of bits for representing input values (u)
    pub input_bit_length: usize,
    /// Number of bits for representing output values (v)
    pub output_bit_length: usize,
    /// Dimension of the input space
    pub dimension: usize,
    /// Data specific to the Lp sharing
    pub data: ShareDataLp,
}

/// Represents the shared data for Lp distance computation
/// Each server gets one OKVS per dimension
#[derive(Debug, Clone)]
pub struct SharedRangeLp {
    /// OKVS encodings, one per dimension
    pub okvs_shares: Vec<Vec<u128>>,
    /// Server role: true for server 1, false for server 0
    pub role: bool,
}

/// Share phase handler for Lp distance
#[derive(Clone)]
pub struct SharePhaseLp {
    pub config: ShareConfigLp,
}

impl SharePhaseLp {
    /// Create a new Lp share phase with the given configuration
    pub fn new(config: ShareConfigLp) -> Self {
        Self { config }
    }

    /// Share a range for Lp distance computation
    /// Input x is a d-dimensional vector, output will be 2 SharedRangeLp (one for each server)
    /// For each dimension i, creates OKVS mapping keys in [x_i-delta, x_i+delta] to values
    /// that allow reconstruction of |x_i - y_i|^p
    pub fn share_range(&self, x: &[u128], delta: u128) -> Result<(SharedRangeLp, SharedRangeLp), SharePhaseLpError> {
        assert_eq!(x.len(), self.config.dimension, "Input x must match the configured dimension");
        
        // Calculate the maximum value for u bits
        let max_input = (1u128 << self.config.input_bit_length) - 1;
        
        // Validate that x is within the valid range
        for &xi in x {
            if xi > max_input {
                return Err(SharePhaseLpError::InvalidRange(
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

        match self.config.dictionary_type {
            DictionaryTypeLp::Known => {
                self.share_with_okvs_known_lp(&x, &left_bound, &right_bound)
            },
            DictionaryTypeLp::Unknown => {
                self.share_with_okvs_unknown_lp(&x, &left_bound, &right_bound)
            },
        }
    }

    /// Public method to evaluate at a specific point for a single dimension
    /// Returns the secret share of |point - x_i|^p for dimension i
    pub fn evaluate_at_single_dimension(
        &self,
        shared_range: &SharedRangeLp,
        point_bits: &[bool],
        dimension: usize,
    ) -> Result<u128, SharePhaseLpError> {
        // Return 0 if point_bits is empty
        if point_bits.is_empty() {
            return Ok(0);
        }
        
        let result = self.evaluate_okvs_at_single_dimension(&shared_range.okvs_shares[dimension], point_bits, shared_range.role)?;
        Ok(result)
    }

    /// Share using OKVS method for known dictionary in Lp case
    /// For each dimension i, create an OKVS that maps keys in [left_bound[i], right_bound[i]] 
    /// to secret shares of |key - x_i|^p
    fn share_with_okvs_known_lp(
        &self, 
        x: &[u128], // The original d-dimensional vector
        left_bound: &Vec<u128>, 
        right_bound: &Vec<u128>,
    ) -> Result<(SharedRangeLp, SharedRangeLp), SharePhaseLpError> {
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
                let distance_p = self.compute_power(distance, self.config.data.p) & modulus_mask;
                
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
                &self.config.data.r1, 
                &self.config.data.r2,
            );

            let encoding_0 = okvs.encode(&keys, &values_0)?;
            let encoding_1 = okvs.encode(&keys, &values_1)?;

            okvs_shares_0.push(encoding_0);
            okvs_shares_1.push(encoding_1);
        }

        Ok((
            SharedRangeLp {
                okvs_shares: okvs_shares_0,
                role: false, // Server 0
            },
            SharedRangeLp {
                okvs_shares: okvs_shares_1,
                role: true, // Server 1
            },
        ))
    }

    /// Share using OKVS method for unknown dictionary in Lp case
    /// For each dimension i, create an OKVS that maps all prefixes of keys in [left_bound[i], right_bound[i]] 
    /// to secret shares of |key - x_i|^p (where key is the full value corresponding to the prefix)
    fn share_with_okvs_unknown_lp(
        &self, 
        x: &[u128], // The original d-dimensional vector
        left_bound: &Vec<u128>, 
        right_bound: &Vec<u128>,
    ) -> Result<(SharedRangeLp, SharedRangeLp), SharePhaseLpError> {
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
                    distinct_prefixes.insert((prefix, prefix_len)); // Remove original_value as we don't need it
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
                
                let distance_p = if self.is_prefix_of(prefix, prefix_len, x_i) {
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
                        self.compute_power(distance, self.config.data.p) & modulus_mask
                    } else {
                        // Case 3: prefix > x_i prefix, closest string is prefix + all 0s
                        let closest_string = prefix << (self.config.input_bit_length - prefix_len);
                        let distance = closest_string - x_i; // closest_string > x_i always in this case
                        self.compute_power(distance, self.config.data.p) & modulus_mask
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
                
                // Secret sharing: share0 gets random_value, share1 gets |original_value - x_i|^p - random_value + mask
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
                &self.config.data.r1, 
                &self.config.data.r2,
            );

            let encoding_0 = okvs.encode(&keys, &values_0)?;
            let encoding_1 = okvs.encode(&keys, &values_1)?;

            okvs_shares_0.push(encoding_0);
            okvs_shares_1.push(encoding_1);
        }

        Ok((
            SharedRangeLp {
                okvs_shares: okvs_shares_0,
                role: false, // Server 0
            },
            SharedRangeLp {
                okvs_shares: okvs_shares_1,
                role: true, // Server 1
            },
        ))
    }

    /// Helper: Evaluate OKVS at a single dimension
    /// Returns the secret share of |point - x_i|^p for the specified dimension
    fn evaluate_okvs_at_single_dimension(
        &self,
        okvs_share: &Vec<u128>,
        point_bits: &[bool],
        role: bool,
    ) -> Result<u128, SharePhaseLpError> {
        let key_bits = point_bits.to_vec();
        let modulus_mask = (1u128 << self.config.output_bit_length) - 1;
        
        let okvs = RbOkvsF2k::<u128>::new(
            1,
            okvs_share.len(),
            55, // Band width
            &self.config.data.r1, 
            &self.config.data.r2,
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

    /// Compute base^exponent modulo 2^output_bit_length
    /// Uses fast exponentiation for efficiency
    fn compute_power(&self, base: u128, exponent: u32) -> u128 {
        if exponent == 0 {
            return 1;
        }
        if exponent == 1 {
            return base;
        }
        
        let modulus_mask = (1u128 << self.config.output_bit_length) - 1;
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
    fn is_prefix_of(&self, prefix: u128, prefix_len: usize, target: u128) -> bool {
        if prefix_len == 0 {
            return true; // Empty prefix is always a prefix
        }
        if prefix_len > self.config.input_bit_length {
            return false; // Prefix longer than input length
        }
        
        // Extract the first prefix_len bits of target
        let target_prefix = target >> (self.config.input_bit_length - prefix_len);
        prefix == target_prefix
    }
}

/// Errors that can occur during the Lp share phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SharePhaseLpError {
    /// Invalid range parameters
    InvalidRange(String),
    /// OKVS encoding failed
    OKVSError(String),
    /// Evaluation failed
    EvaluationError(String),
}

impl std::fmt::Display for SharePhaseLpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SharePhaseLpError::InvalidRange(msg) => write!(f, "Invalid range: {}", msg),
            SharePhaseLpError::OKVSError(msg) => write!(f, "OKVS error: {}", msg),
            SharePhaseLpError::EvaluationError(msg) => write!(f, "Evaluation error: {}", msg),
        }
    }
}

impl std::error::Error for SharePhaseLpError {}

impl From<crate::okvs_f2k::OkvsError> for SharePhaseLpError {
    fn from(err: crate::okvs_f2k::OkvsError) -> Self {
        SharePhaseLpError::OKVSError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test basic Lp share phase functionality with known dictionary
    #[test]
    fn test_lp_share_phase_known() {
        let config = ShareConfigLp {
            dictionary_type: DictionaryTypeLp::Known,
            input_bit_length: 8,
            output_bit_length: 16,
            dimension: 2,
            data: ShareDataLp {
                r1: [1u8; 16],
                r2: [2u8; 16],
                p: 2, // L2 distance
            },
        };

        let share_phase = SharePhaseLp::new(config);
        let x = vec![50u128, 75u128]; // 2D input
        let delta = 10u128;

        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok());

        let (shared_0, shared_1) = result.unwrap();
        assert_eq!(shared_0.okvs_shares.len(), 2); // One OKVS per dimension
        assert_eq!(shared_1.okvs_shares.len(), 2);
        assert!(!shared_0.role); // Server 0
        assert!(shared_1.role);  // Server 1
    }

    /// Test Lp share phase with unknown dictionary
    #[test]
    fn test_lp_share_phase_unknown() {
        let config = ShareConfigLp {
            dictionary_type: DictionaryTypeLp::Unknown,
            input_bit_length: 4, // Small for testing
            output_bit_length: 8,
            dimension: 1,
            data: ShareDataLp {
                r1: [3u8; 16],
                r2: [4u8; 16],
                p: 1, // L1 distance
            },
        };

        let share_phase = SharePhaseLp::new(config);
        let x = vec![8u128]; // 1D input
        let delta = 2u128;

        let result = share_phase.share_range(&x, delta);
        assert!(result.is_ok());

        let (shared_0, shared_1) = result.unwrap();
        assert_eq!(shared_0.okvs_shares.len(), 1); // One OKVS for single dimension
        assert_eq!(shared_1.okvs_shares.len(), 1);
    }

    /// Test power computation
    #[test]
    fn test_compute_power() {
        let config = ShareConfigLp {
            dictionary_type: DictionaryTypeLp::Known,
            input_bit_length: 8,
            output_bit_length: 16,
            dimension: 1,
            data: ShareDataLp {
                r1: [1u8; 16],
                r2: [2u8; 16],
                p: 3,
            },
        };

        let share_phase = SharePhaseLp::new(config);
        
        assert_eq!(share_phase.compute_power(2, 0), 1);
        assert_eq!(share_phase.compute_power(2, 1), 2);
        assert_eq!(share_phase.compute_power(2, 2), 4);
        assert_eq!(share_phase.compute_power(2, 3), 8);
        assert_eq!(share_phase.compute_power(3, 2), 9);
        assert_eq!(share_phase.compute_power(5, 2), 25);
    }

    /// Test evaluation at specific points
    #[test]
    fn test_evaluation() {
        let config = ShareConfigLp {
            dictionary_type: DictionaryTypeLp::Known,
            input_bit_length: 4,
            output_bit_length: 8,
            dimension: 1,
            data: ShareDataLp {
                r1: [5u8; 16],
                r2: [6u8; 16],
                p: 2,
            },
        };

        let share_phase = SharePhaseLp::new(config);
        let x = vec![8u128];
        let delta = 2u128;

        let (shared_0, shared_1) = share_phase.share_range(&x, delta).unwrap();

        // Test evaluation at a point within range
        let point = 7u128;
        let point_bits = u128_to_bits(point, 4);
        let mut point_bits_rev = point_bits.clone();
        point_bits_rev.reverse();

        let eval_0 = share_phase.evaluate_at_single_dimension(&shared_0, &point_bits_rev, 0);
        let eval_1 = share_phase.evaluate_at_single_dimension(&shared_1, &point_bits_rev, 0);

        assert!(eval_0.is_ok());
        assert!(eval_1.is_ok());

        // The sum of shares should equal |7 - 8|^2 = 1
        let sum = (eval_0.unwrap() + eval_1.unwrap()) & ((1u128 << 8) - 1);
        assert_eq!(sum, 1);
    }

    /// Test prefix distance computation for unknown dictionary
    #[test]
    fn test_prefix_distance() {
        let config = ShareConfigLp {
            dictionary_type: DictionaryTypeLp::Known,
            input_bit_length: 4,
            output_bit_length: 8,
            dimension: 1,
            data: ShareDataLp {
                r1: [1u8; 16],
                r2: [2u8; 16],
                p: 2,
            },
        };

        let share_phase = SharePhaseLp::new(config);
        
        // Test is_prefix_of function
        // x_i = 8 = 1000 in binary
        let x_i = 8u128;
        
        // Test case 1: prefix is a prefix of x_i
        // prefix = 1 (len=1) should be prefix of 1000
        assert!(share_phase.is_prefix_of(1, 1, x_i));
        
        // Test case 2: prefix is not a prefix of x_i
        // prefix = 0 (len=1) should not be prefix of 1000
        assert!(!share_phase.is_prefix_of(0, 1, x_i));
        
        // Test case 3: full prefix match
        // prefix = 8 (len=4) should be prefix of 1000
        assert!(share_phase.is_prefix_of(8, 4, x_i));
        
        // Test edge cases
        assert!(share_phase.is_prefix_of(0, 0, x_i)); // Empty prefix
        assert!(!share_phase.is_prefix_of(1, 5, x_i)); // Prefix longer than input
    }
}
