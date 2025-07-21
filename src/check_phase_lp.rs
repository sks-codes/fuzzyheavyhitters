//! Check Phase Lp Implementation
//!
//! This module provides functionality for checking the outputs from the share phase Lp
//! using garbled circuit less-than-or-equal-threshold tests
//!
//! ## Protocol Flow:
//! 1. Client shares a secret using ShareLpPhase
//! 2. Server 1 has evaluation points and evaluates Lp distances at these points
//! 3. Server 2 has evaluation points and evaluates Lp distances at these points
//! 4. The two servers use garbled circuits to check if their distances are ≤ threshold
//!    without revealing the actual distance values

use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::thread;
use scuttlebutt::{AesRng, Channel};
use crate::share_phase_lp::{SharedLpRange, ShareLpPhase, ShareLpPhaseError};
use crate::garbled_circuits::less_than_or_equal_threshold::{
    multiple_gb_complex_comparison, multiple_ev_complex_comparison
};
use crate::data_structures::modint::ModInt;
use serde::{Deserialize, Serialize};

/// Configuration for the check phase Lp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckLpConfig {
    /// Number of threshold tests to perform
    pub num_tests: usize,
    /// Modulus for ModInt operations (must be power of 2)
    pub modulus: u128,
    /// Whether this is the garbler side (true) or evaluator side (false)
    pub is_garbler_side: bool,
}

/// Error types for check phase Lp operations
#[derive(Debug, Clone)]
pub enum CheckLpPhaseError {
    /// Error during share phase Lp evaluation
    ShareLpPhaseError(ShareLpPhaseError),
    /// Mismatched input lengths
    InputLengthMismatch(String),
    /// Channel communication error
    ChannelError(String),
    /// Invalid configuration
    InvalidConfig(String),
    /// Invalid modulus (must be power of 2)
    InvalidModulus(String),
}

impl From<ShareLpPhaseError> for CheckLpPhaseError {
    fn from(error: ShareLpPhaseError) -> Self {
        CheckLpPhaseError::ShareLpPhaseError(error)
    }
}

/// Check phase Lp handler
#[derive(Clone)]
pub struct CheckLpPhase {
    config: CheckLpConfig,
    share_phase: ShareLpPhase,
}

impl CheckLpPhase {
    /// Create a new check phase Lp with the given configuration
    pub fn new(config: CheckLpConfig, share_phase: ShareLpPhase) -> Result<Self, CheckLpPhaseError> {
        // Validate that modulus is a power of 2
        if !is_power_of_two(config.modulus) {
            return Err(CheckLpPhaseError::InvalidModulus(
                format!("Modulus {} must be a power of 2 for garbled circuits", config.modulus)
            ));
        }

        Ok(Self { config, share_phase })
    }

    /// Run less-than-or-equal-threshold tests on the Lp distances from share phase
    /// This evaluates the shared range at this server's evaluation points
    /// and compares the results with the other server using garbled circuits
    pub fn run_lp_threshold_check(
        &self,
        shared_range: &SharedLpRange,
        evaluation_points: &[Vec<u128>],
        thresholds: &[u128],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<Vec<bool>, CheckLpPhaseError> {
        if evaluation_points.len() != thresholds.len() {
            return Err(CheckLpPhaseError::InputLengthMismatch(
                "Evaluation points and thresholds must have the same length".to_string(),
            ));
        }

        // Evaluate the shared range at this server's evaluation points
        let mut this_server_distances = Vec::new();
        for point in evaluation_points {
            let distance = self.share_phase.evaluate_lp_distance(shared_range, point)?;
            this_server_distances.push(distance);
        }

        // Use garbled circuits to compare with the other server's distances
        if self.config.is_garbler_side {
            self.run_garbler_side_threshold_check(&this_server_distances, thresholds, channel, rng)
        } else {
            self.run_evaluator_side_threshold_check(&this_server_distances, channel, rng)
        }
    }

    /// Run garbler side threshold check
    fn run_garbler_side_threshold_check(
        &self,
        this_server_distances: &[u128],
        thresholds: &[u128],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<Vec<bool>, CheckLpPhaseError> {
        // Create ModInt instances for the garbled circuit
        let y_values: Vec<ModInt> = this_server_distances.iter()
            .map(|&d| ModInt::new(d % self.config.modulus, self.config.modulus))
            .collect();

        let t_values: Vec<ModInt> = thresholds.iter()
            .map(|&t| ModInt::new(t % self.config.modulus, self.config.modulus))
            .collect();

        // Run the garbled circuit (garbler side)
        // The garbler side returns (), but the evaluator will get the results
        multiple_gb_complex_comparison(rng, channel, &y_values, &t_values);
        
        // Return empty vector as the garbler doesn't get the results directly
        Ok(vec![false; this_server_distances.len()])
    }

    /// Run evaluator side threshold check
    fn run_evaluator_side_threshold_check(
        &self,
        this_server_distances: &[u128],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<Vec<bool>, CheckLpPhaseError> {
        // Create ModInt instances for the garbled circuit
        let x_values: Vec<ModInt> = this_server_distances.iter()
            .map(|&d| ModInt::new(d % self.config.modulus, self.config.modulus))
            .collect();

        // Run the garbled circuit (evaluator side)
        let results = multiple_ev_complex_comparison(rng, channel, &x_values);
        Ok(results)
    }

    /// Compare shared Lp range evaluations between two servers
    pub fn compare_shared_lp_ranges(
        &self,
        shared_range: &SharedLpRange,
        evaluation_points: &[Vec<u128>],
        thresholds: &[u128],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<LpComparisonResult, CheckLpPhaseError> {
        let threshold_results = self.run_lp_threshold_check(
            shared_range, 
            evaluation_points, 
            thresholds, 
            channel, 
            rng
        )?;
        
        let total_tests = threshold_results.len();
        let within_threshold_count = threshold_results.iter().filter(|&&x| x).count();
        let outside_threshold_count = total_tests - within_threshold_count;
        
        // Calculate actual distances for this server only
        let mut this_server_distances = Vec::new();
        
        for point in evaluation_points {
            let distance = self.share_phase.evaluate_lp_distance(shared_range, point)?;
            this_server_distances.push(distance);
        }

        Ok(LpComparisonResult {
            total_tests,
            within_threshold_count,
            outside_threshold_count,
            threshold_results,
            evaluation_points: evaluation_points.to_vec(),
            thresholds: thresholds.to_vec(),
            this_server_distances,
        })
    }

    /// Get the configuration
    pub fn config(&self) -> &CheckLpConfig {
        &self.config
    }
}

/// Result of comparing shared Lp range evaluations
#[derive(Debug, Clone)]
pub struct LpComparisonResult {
    /// Total number of tests performed
    pub total_tests: usize,
    /// Number of points where the distance is within threshold
    pub within_threshold_count: usize,
    /// Number of points where the distance is outside threshold
    pub outside_threshold_count: usize,
    /// Detailed threshold test results for each evaluation point
    pub threshold_results: Vec<bool>,
    /// Points that were evaluated
    pub evaluation_points: Vec<Vec<u128>>,
    /// Thresholds that were used
    pub thresholds: Vec<u128>,
    /// Actual distances computed for this server
    pub this_server_distances: Vec<u128>,
}

impl LpComparisonResult {
    /// Get the percentage of points within threshold
    pub fn within_threshold_percentage(&self) -> f64 {
        if self.total_tests == 0 {
            0.0
        } else {
            (self.within_threshold_count as f64 / self.total_tests as f64) * 100.0
        }
    }

    /// Check if all evaluations are within threshold
    pub fn all_within_threshold(&self) -> bool {
        self.within_threshold_count == self.total_tests
    }

    /// Check if no evaluations are within threshold
    pub fn none_within_threshold(&self) -> bool {
        self.within_threshold_count == 0
    }

    /// Get points where the distance is outside threshold
    pub fn get_outside_threshold_points(&self) -> Vec<(&Vec<u128>, u128, u128)> {
        self.threshold_results
            .iter()
            .enumerate()
            .filter(|(_, &within_threshold)| !within_threshold)
            .map(|(i, _)| (
                &self.evaluation_points[i],
                self.thresholds[i],
                self.this_server_distances[i],
            ))
            .collect()
    }

    /// Get statistics about the distance computations for this server
    pub fn get_distance_stats(&self) -> LpDistanceStats {
        let min_distance = self.this_server_distances.iter().min().copied().unwrap_or(0);
        let max_distance = self.this_server_distances.iter().max().copied().unwrap_or(0);
        let avg_distance = if self.this_server_distances.is_empty() {
            0.0
        } else {
            self.this_server_distances.iter().sum::<u128>() as f64 / self.this_server_distances.len() as f64
        };

        LpDistanceStats {
            min_distance,
            max_distance,
            avg_distance,
            distances: self.this_server_distances.clone(),
        }
    }
}

/// Statistics about Lp distance computations
#[derive(Debug, Clone)]
pub struct LpDistanceStats {
    /// Minimum distance
    pub min_distance: u128,
    /// Maximum distance
    pub max_distance: u128,
    /// Average distance
    pub avg_distance: f64,
    /// All distances
    pub distances: Vec<u128>,
}

/// Check if a number is a power of 2
fn is_power_of_two(n: u128) -> bool {
    n > 0 && (n & (n - 1)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share_phase_lp::{ShareLpConfig, ShareLpMethod};

    #[test]
    fn test_is_power_of_two() {
        assert!(is_power_of_two(1));
        assert!(is_power_of_two(2));
        assert!(is_power_of_two(4));
        assert!(is_power_of_two(8));
        assert!(is_power_of_two(16));
        assert!(is_power_of_two(64));
        assert!(is_power_of_two(128));
        
        assert!(!is_power_of_two(0));
        assert!(!is_power_of_two(3));
        assert!(!is_power_of_two(5));
        assert!(!is_power_of_two(6));
        assert!(!is_power_of_two(7));
        assert!(!is_power_of_two(97));
    }

    #[test]
    fn test_check_lp_phase_creation() {
        let share_config = ShareLpConfig {
            method: ShareLpMethod::DistanceFSS,
            input_bit_length: 8,
            dimension: 2,
            p: 2,
            modulus: 256,
            r1: [1u8; 16],
            r2: [2u8; 16],
        };

        let check_config = CheckLpConfig {
            num_tests: 3,
            modulus: 64, // Power of 2
            is_garbler_side: true,
        };

        let share_phase = ShareLpPhase::new(share_config).unwrap();
        let check_phase = CheckLpPhase::new(check_config, share_phase).unwrap();

        assert_eq!(check_phase.config.num_tests, 3);
        assert_eq!(check_phase.config.modulus, 64);
    }

    #[test]
    fn test_invalid_modulus() {
        let share_config = ShareLpConfig {
            method: ShareLpMethod::DistanceFSS,
            input_bit_length: 8,
            dimension: 2,
            p: 2,
            modulus: 256,
            r1: [1u8; 16],
            r2: [2u8; 16],
        };

        let check_config = CheckLpConfig {
            num_tests: 1,
            modulus: 97, // Not a power of 2
            is_garbler_side: true,
        };

        let share_phase = ShareLpPhase::new(share_config).unwrap();
        let result = CheckLpPhase::new(check_config, share_phase);
        assert!(result.is_err());
    }
}
