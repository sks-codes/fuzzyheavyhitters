//! Share Phase Lp Implementation
//!
//! This module provides functionality for sharing using distance FSS and OKVS
//! for Lp norm distance computations (where p >= 1).

use std::sync::Arc;
use std::cmp::max;
use rand;

use crate::okvs_f2k::{self, RbOkvsF2k};
use crate::fss::distance::{DistanceFSSKey};
use crate::data_structures::{field::FieldElm, payload::RingVec};
use serde::{Deserialize, Serialize};

/// Enumeration of different Lp sharing methods available
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareLpMethod {
    /// Use Distance FSS for sharing
    DistanceFSS,
    /// Use OKVS combined with Distance FSS for sharing
    OKVSDistanceFSS,
}

/// Configuration for the share phase Lp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareLpConfig {
    /// The sharing method to use
    pub method: ShareLpMethod,
    /// Number of bits for representing input values (u)
    pub input_bit_length: usize,
    /// Dimension of the input vectors
    pub dimension: usize,
    /// Lp norm parameter (p in Lp norm), must be >= 1
    pub p: usize,
    /// Modulus for arithmetic operations
    pub modulus: u128,
    /// Random seeds for OKVS (if used)
    pub r1: [u8; 16],
    pub r2: [u8; 16],
}

/// Represents the shared data for Lp distance computation
#[derive(Debug, Clone)]
pub enum SharedLpRange {
    /// Distance FSS-based sharing
    DistanceFSS {
        fss_keys: Vec<DistanceFSSKey<6>>, // Support up to L5 norm (N = p+1 = 6)
        center_point: Vec<u128>,
        threshold: u128,
    },
    /// OKVS combined with Distance FSS
    OKVSDistanceFSS {
        okvs_share: Vec<bool>,
        fss_keys: Vec<DistanceFSSKey<6>>,
        center_point: Vec<u128>,
        threshold: u128,
    },
}

/// Error types for share phase Lp operations
#[derive(Debug, Clone)]
pub enum ShareLpPhaseError {
    /// Invalid input range
    InvalidRange(String),
    /// OKVS encoding error
    OKVSError(String),
    /// Distance FSS generation error
    DistanceFSSError(String),
    /// Invalid configuration
    InvalidConfig(String),
}

impl From<okvs_f2k::OkvsError> for ShareLpPhaseError {
    fn from(error: okvs_f2k::OkvsError) -> Self {
        ShareLpPhaseError::OKVSError(format!("OKVS error: {:?}", error))
    }
}

/// Share phase Lp handler
#[derive(Clone)]
pub struct ShareLpPhase {
    config: ShareLpConfig,
}

impl ShareLpPhase {
    /// Create a new share phase Lp with the given configuration
    pub fn new(config: ShareLpConfig) -> Result<Self, ShareLpPhaseError> {
        // Validate configuration
        if config.p == 0 {
            return Err(ShareLpPhaseError::InvalidConfig(
                "p must be >= 1 for Lp norm".to_string()
            ));
        }
        
        if config.p > 5 {
            return Err(ShareLpPhaseError::InvalidConfig(
                "p must be <= 5 (maximum supported Lp norm)".to_string()
            ));
        }

        if config.dimension == 0 {
            return Err(ShareLpPhaseError::InvalidConfig(
                "dimension must be > 0".to_string()
            ));
        }

        Ok(Self { config })
    }

    /// Share an Lp ball around center point x with radius threshold
    /// Input x is in range [0, 2^u-1]^d, where d is dimension
    pub fn share_lp_ball(
        &self,
        center: &[u128],
        threshold: u128,
    ) -> Result<(SharedLpRange, SharedLpRange), ShareLpPhaseError> {
        if center.len() != self.config.dimension {
            return Err(ShareLpPhaseError::InvalidRange(
                format!("Center point dimension ({}) must match configured dimension ({})",
                        center.len(), self.config.dimension)
            ));
        }

        // Calculate the maximum value for u bits
        let max_input = (1u128 << self.config.input_bit_length) - 1;
        
        // Validate that center is within the valid range
        for &xi in center {
            if xi > max_input {
                return Err(ShareLpPhaseError::InvalidRange(
                    format!("Center point coordinate ({}) exceeds maximum value for {}-bit input ({})",
                            xi, self.config.input_bit_length, max_input)
                ));
            }
        }

        match self.config.method {
            ShareLpMethod::DistanceFSS => {
                self.share_with_distance_fss(center, threshold)
            },
            ShareLpMethod::OKVSDistanceFSS => {
                self.share_with_okvs_distance_fss(center, threshold)
            },
        }
    }

    /// Share using Distance FSS method
    fn share_with_distance_fss(
        &self,
        center: &[u128],
        threshold: u128,
    ) -> Result<(SharedLpRange, SharedLpRange), ShareLpPhaseError> {
        let mut fss_keys_0 = Vec::new();
        let mut fss_keys_1 = Vec::new();

        // Generate distance FSS keys for each dimension
        for (dim, &center_coord) in center.iter().enumerate() {
            // Convert center coordinate to bits
            let mut center_bits = u128_to_bits(center_coord, self.config.input_bit_length);
            center_bits.reverse();

            // For Lp ball, we need to consider the range where the distance is <= threshold
            // Left bound: max(0, center_coord - threshold)
            let left_bound = if center_coord < threshold {
                0
            } else {
                center_coord - threshold
            };

            // Right bound: min(max_input, center_coord + threshold)
            let max_input = (1u128 << self.config.input_bit_length) - 1;
            let right_bound = if center_coord + threshold > max_input {
                max_input
            } else {
                center_coord + threshold
            };

            let mut left_bits = u128_to_bits(left_bound, self.config.input_bit_length);
            left_bits.reverse();
            let mut right_bits = u128_to_bits(right_bound, self.config.input_bit_length);
            right_bits.reverse();

            // Generate distance FSS keys with N = p+1
            let (key_0, key_1) = DistanceFSSKey::gen_distance_fss_key(
                center_coord,
                &center_bits,
                &left_bits,
                &right_bits,
                self.config.modulus,
            );

            fss_keys_0.push(key_0);
            fss_keys_1.push(key_1);
        }

        Ok((
            SharedLpRange::DistanceFSS {
                fss_keys: fss_keys_0,
                center_point: center.to_vec(),
                threshold,
            },
            SharedLpRange::DistanceFSS {
                fss_keys: fss_keys_1,
                center_point: center.to_vec(),
                threshold,
            },
        ))
    }

    /// Share using OKVS combined with Distance FSS method
    fn share_with_okvs_distance_fss(
        &self,
        center: &[u128],
        threshold: u128,
    ) -> Result<(SharedLpRange, SharedLpRange), ShareLpPhaseError> {
        // First, generate the Distance FSS keys
        let (distance_share_0, distance_share_1) = self.share_with_distance_fss(center, threshold)?;

        // Extract the FSS keys from the distance shares
        let fss_keys_0 = match distance_share_0 {
            SharedLpRange::DistanceFSS { fss_keys, .. } => fss_keys,
            _ => unreachable!(),
        };
        
        let fss_keys_1 = match distance_share_1 {
            SharedLpRange::DistanceFSS { fss_keys, .. } => fss_keys,
            _ => unreachable!(),
        };

        // Create OKVS to store precomputed values for efficiency
        let mut keys = Vec::new();
        let mut values_0 = Vec::new();
        let mut values_1 = Vec::new();

        // Sample key-value pairs around the center point
        // This is a simplified approach - in practice, you might want to be more systematic
        let sample_radius = std::cmp::min(threshold, 10); // Limit sampling for efficiency
        
        for dim in 0..self.config.dimension {
            let center_coord = center[dim];
            let left_bound = if center_coord < sample_radius {
                0
            } else {
                center_coord - sample_radius
            };
            let right_bound = std::cmp::min(
                center_coord + sample_radius,
                (1u128 << self.config.input_bit_length) - 1,
            );

            for sample_point in left_bound..=right_bound {
                let mut key_bits = u128_to_bits(sample_point, self.config.input_bit_length);
                key_bits.reverse();
                
                // Add dimension index to the key
                let mut dim_bits = u128_to_bits(dim as u128, 8);
                dim_bits.reverse();
                key_bits.extend(&dim_bits);

                // Compute the distance contribution for this dimension
                let distance_contribution = if sample_point >= center_coord {
                    sample_point - center_coord
                } else {
                    center_coord - sample_point
                };

                // Create secret shares of the distance contribution
                let share0 = rand::random::<bool>();
                let share1 = share0; // XOR with the actual value would be done during evaluation

                keys.push(key_bits);
                values_0.push(share0);
                values_1.push(share1);
            }
        }

        // Create and encode OKVS
        let columns = max((keys.len() as f64 * 1.1) as usize, 60);
        let band_width = 55;
        let okvs = RbOkvsF2k::<bool>::new(
            keys.len(),
            columns,
            band_width,
            &self.config.r1,
            &self.config.r2,
        );

        let encoding_0 = okvs.encode(&keys, &values_0)?;
        let encoding_1 = okvs.encode(&keys, &values_1)?;

        Ok((
            SharedLpRange::OKVSDistanceFSS {
                okvs_share: encoding_0,
                fss_keys: fss_keys_0,
                center_point: center.to_vec(),
                threshold,
            },
            SharedLpRange::OKVSDistanceFSS {
                okvs_share: encoding_1,
                fss_keys: fss_keys_1,
                center_point: center.to_vec(),
                threshold,
            },
        ))
    }

    /// Evaluate the shared Lp range at a specific point
    pub fn evaluate_lp_distance(
        &self,
        shared_range: &SharedLpRange,
        point: &[u128],
    ) -> Result<u128, ShareLpPhaseError> {
        if point.len() != self.config.dimension {
            return Err(ShareLpPhaseError::InvalidRange(
                format!("Point dimension ({}) must match configured dimension ({})",
                        point.len(), self.config.dimension)
            ));
        }

        match shared_range {
            SharedLpRange::DistanceFSS { fss_keys, center_point, .. } => {
                self.evaluate_distance_fss(fss_keys, center_point, point)
            },
            SharedLpRange::OKVSDistanceFSS { okvs_share, fss_keys, center_point, .. } => {
                // For now, just use the FSS keys - OKVS can be used for optimization
                self.evaluate_distance_fss(fss_keys, center_point, point)
            },
        }
    }

    /// Evaluate Distance FSS at a specific point
    fn evaluate_distance_fss(
        &self,
        fss_keys: &[DistanceFSSKey<6>],
        center_point: &[u128],
        point: &[u128],
    ) -> Result<u128, ShareLpPhaseError> {
        let mut total_distance = 0u128;

        for (dim, (key, (&center_coord, &point_coord))) in fss_keys.iter()
            .zip(center_point.iter().zip(point.iter()))
            .enumerate() {
            
            // Convert point coordinate to bits
            let mut point_bits = u128_to_bits(point_coord, self.config.input_bit_length);
            point_bits.reverse();

            // Evaluate the distance FSS key
            let distance_contribution = key.eval_distance_fss(&point_bits, self.config.input_bit_length, self.config.modulus);
            
            // Add to total distance (for Lp norm computation)
            total_distance = (total_distance + distance_contribution) % self.config.modulus;
        }

        Ok(total_distance)
    }

    /// Get the configuration
    pub fn config(&self) -> &ShareLpConfig {
        &self.config
    }
}

/// Convert u128 to bit vector (LSB first)
fn u128_to_bits(mut value: u128, bit_length: usize) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bit_length);
    for _ in 0..bit_length {
        bits.push((value & 1) == 1);
        value >>= 1;
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_share_lp_phase_creation() {
        let config = ShareLpConfig {
            method: ShareLpMethod::DistanceFSS,
            input_bit_length: 8,
            dimension: 2,
            p: 2, // L2 norm
            modulus: 256,
            r1: [1u8; 16],
            r2: [2u8; 16],
        };

        let share_phase = ShareLpPhase::new(config).unwrap();
        assert_eq!(share_phase.config.p, 2);
        assert_eq!(share_phase.config.dimension, 2);
    }

    #[test]
    fn test_invalid_config() {
        let config = ShareLpConfig {
            method: ShareLpMethod::DistanceFSS,
            input_bit_length: 8,
            dimension: 2,
            p: 0, // Invalid p
            modulus: 256,
            r1: [1u8; 16],
            r2: [2u8; 16],
        };

        let result = ShareLpPhase::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_u128_to_bits() {
        let bits = u128_to_bits(5, 4); // 5 = 0101 in binary
        assert_eq!(bits, vec![true, false, true, false]); // LSB first
    }
}
