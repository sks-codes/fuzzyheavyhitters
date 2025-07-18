//! Check Phase Implementation
//!
//! This module provides functionality for checking the outputs from the share phase
//! using garbled circuit equality tests from equality.rs
//!
//! ## Protocol Flow:
//! 1. Client shares a secret value x using SharePhase
//! 2. Server 1 has evaluation points y1 and evaluates the shared range at these points
//! 3. Server 2 has evaluation points y2 and evaluates the shared range at these points  
//! 4. The two servers use garbled circuits to compare their evaluation results
//!    without revealing the actual values to each other
//! 5. The result indicates whether their evaluations are equal at corresponding positions

use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::thread;
use scuttlebutt::{AesRng, Channel};
use crate::share_phase::{SharedRange, SharePhase, SharePhaseError};
use crate::garbled_circuits::equality::{multiple_gb_equality_test, multiple_ev_equality_test};
use serde::{Deserialize, Serialize};

/// Configuration for the check phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckConfig {
    /// Number of equality tests to perform
    pub num_tests: usize,
    /// Points to evaluate at
    pub evaluation_points: Vec<u128>,
    /// Dimensions for each evaluation point
    pub evaluation_dimensions: Vec<usize>,
    /// Whether this is the garbler side (true) or evaluator side (false)
    pub is_garbler_side: bool,
}

/// Error types for check phase operations
#[derive(Debug, Clone)]
pub enum CheckPhaseError {
    /// Error during share phase evaluation
    SharePhaseError(SharePhaseError),
    /// Mismatched input lengths
    InputLengthMismatch(String),
    /// Channel communication error
    ChannelError(String),
    /// Invalid configuration
    InvalidConfig(String),
}

impl From<SharePhaseError> for CheckPhaseError {
    fn from(error: SharePhaseError) -> Self {
        CheckPhaseError::SharePhaseError(error)
    }
}

/// Check phase handler
#[derive(Clone)]
pub struct CheckPhase {
    config: CheckConfig,
    share_phase: SharePhase,
}

impl CheckPhase {
    /// Create a new check phase with the given configuration
    pub fn new(config: CheckConfig, share_phase: SharePhase) -> Self {
        Self { config, share_phase }
    }

    /// Run equality tests on the outputs from share phase
    /// This evaluates the shared range at this server's evaluation points
    /// and compares the results with the other server using garbled circuits
    pub fn run_equality_check(
        &self,
        shared_range: &SharedRange,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<Vec<bool>, CheckPhaseError> {
        if self.config.evaluation_points.len() != self.config.evaluation_dimensions.len() {
            return Err(CheckPhaseError::InputLengthMismatch(
                "Evaluation points and dimensions must have the same length".to_string(),
            ));
        }

        // Evaluate the shared range at this server's evaluation points
        let mut this_server_evaluations = Vec::new();
        for (point, dim) in self.config.evaluation_points.iter().zip(self.config.evaluation_dimensions.iter()) {
            let eval = self.share_phase.evaluate_at(shared_range, *point, *dim)?;
            // Concatenate results from all dimensions and convert to u16
            let concat_eval = eval.iter().fold(0u16, |acc, &x| acc.wrapping_add(x as u16));
            this_server_evaluations.push(concat_eval);
        }

        // Use garbled circuits to compare with the other server's evaluations
        if self.config.is_garbler_side {
            self.run_garbler_side_evaluation(&this_server_evaluations, channel, rng)
        } else {
            self.run_evaluator_side_evaluation(&this_server_evaluations, channel, rng)
        }
    }

    /// Run garbler side evaluation
    fn run_garbler_side_evaluation(
        &self,
        this_server_evaluations: &[u16],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<Vec<bool>, CheckPhaseError> {
        // Convert evaluations to the format expected by garbled circuits
        let inputs: Vec<Vec<u16>> = this_server_evaluations.iter().map(|&x| vec![x]).collect();
        
        // Run the equality tests using garbled circuits (garbler side)
        // The evaluator will provide their inputs through the channel
        let results = multiple_gb_equality_test(rng, channel, &inputs);
        Ok(results)
    }

    /// Run evaluator side evaluation
    fn run_evaluator_side_evaluation(
        &self,
        this_server_evaluations: &[u16],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<Vec<bool>, CheckPhaseError> {
        // Convert evaluations to the format expected by garbled circuits
        let inputs: Vec<Vec<u16>> = this_server_evaluations.iter().map(|&x| vec![x]).collect();
        
        // Run the equality tests using garbled circuits (evaluator side)
        // The garbler will provide their inputs through the channel
        let results = multiple_ev_equality_test(rng, channel, &inputs);
        Ok(results)
    }

    /// Compare evaluations of a shared range between two servers
    pub fn compare_shared_ranges(
        &self,
        shared_range: &SharedRange,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<ComparisonResult, CheckPhaseError> {
        let equality_results = self.run_equality_check(shared_range, channel, rng)?;
        
        let total_tests = equality_results.len();
        let equal_count = equality_results.iter().filter(|&&x| x).count();
        let different_count = total_tests - equal_count;
        
        Ok(ComparisonResult {
            total_tests,
            equal_count,
            different_count,
            equality_results,
            evaluation_points: self.config.evaluation_points.clone(),
            evaluation_dimensions: self.config.evaluation_dimensions.clone(),
        })
    }
}

/// Result of comparing two shared ranges
#[derive(Debug, Clone)]
pub struct ComparisonResult {
    /// Total number of tests performed
    pub total_tests: usize,
    /// Number of points where the ranges are equal
    pub equal_count: usize,
    /// Number of points where the ranges are different
    pub different_count: usize,
    /// Detailed equality results for each evaluation point
    pub equality_results: Vec<bool>,
    /// Points that were evaluated
    pub evaluation_points: Vec<u128>,
    /// Dimensions that were evaluated
    pub evaluation_dimensions: Vec<usize>,
}

impl ComparisonResult {
    /// Get the percentage of equal evaluations
    pub fn equality_percentage(&self) -> f64 {
        if self.total_tests == 0 {
            0.0
        } else {
            (self.equal_count as f64 / self.total_tests as f64) * 100.0
        }
    }

    /// Check if all evaluations are equal
    pub fn all_equal(&self) -> bool {
        self.equal_count == self.total_tests
    }

    /// Check if no evaluations are equal
    pub fn none_equal(&self) -> bool {
        self.equal_count == 0
    }

    /// Get points where the ranges differ
    pub fn get_different_points(&self) -> Vec<(u128, usize)> {
        self.equality_results
            .iter()
            .enumerate()
            .filter(|(_, &equal)| !equal)
            .map(|(i, _)| (self.evaluation_points[i], self.evaluation_dimensions[i]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share_phase::{ShareConfig, ShareMethod, ShareData};

    #[test]
    fn test_check_phase_basic() {
        let share_config = ShareConfig {
            method: ShareMethod::OKVS,
            input_bit_length: 8,
            output_bit_length: 8,
            dimension: 2,
            data: ShareData::OKVS {
                r1: [1u8; 16],
                r2: [2u8; 16],
            },
        };

        let check_config = CheckConfig {
            num_tests: 3,
            evaluation_points: vec![10, 20, 30],
            evaluation_dimensions: vec![0, 1, 0],
            is_garbler_side: true,
        };

        let share_phase = SharePhase::new(share_config);
        let check_phase = CheckPhase::new(check_config, share_phase);

        // Test creation
        assert_eq!(check_phase.config.num_tests, 3);
        assert_eq!(check_phase.config.evaluation_points.len(), 3);
        assert_eq!(check_phase.config.evaluation_dimensions.len(), 3);
        assert_eq!(check_phase.config.is_garbler_side, true);
    }
}
