//! Threshold Phase Implementation
//!
//! This module provides functionality for the threshold phase of the fuzzy matching protocol.
//! It aggregates ring shares from multiple clients and compares the sum with a threshold
//! using garbled circuits.

use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::convert::TryInto;
use scuttlebutt::{AesRng, Channel, Block};
use serde::{Deserialize, Serialize};

use crate::garbled_circuits::less_than_or_equal_threshold::{
    multiple_gb_less_than_ss, multiple_ev_less_than_ss
};
use crate::data_structures::modint::ModInt;
use crate::{Share, Group};

/// Configuration for the threshold phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Modulus for ring operations (must be power of 2)
    pub modulus: u128,
    /// Whether this is the garbler side (true) or evaluator side (false)
    pub is_garbler_side: bool,
}

/// Error types for threshold phase operations
#[derive(Debug, Clone)]
pub enum ThresholdPhaseError {
    /// Channel communication error
    ChannelError(String),
    /// Invalid configuration
    InvalidConfig(String),
    /// Conversion error
    ConversionError(String),
    /// Garbled circuit error
    GarbledCircuitError(String),
}

/// Match result from the check phase for a client
#[derive(Debug, Clone)]
pub struct MatchResult<T> {
    /// Ring share representing the match result
    pub ring_share: T,
    /// Client identifier
    pub client_id: usize,
}

/// Result of the threshold comparison
#[derive(Debug, Clone)]
pub struct ThresholdResult {
    /// Whether the aggregated matches exceed the threshold
    pub exceeds_threshold: bool,
    /// Total number of clients processed
    pub total_clients: usize,
}

/// Threshold phase handler
pub struct ThresholdPhase {
    config: ThresholdConfig,
}

impl ThresholdPhase {
    /// Create a new threshold phase with the given configuration
    pub fn new(config: ThresholdConfig) -> Self {
        Self { config }
    }

    /// Aggregate match results and compare with threshold
    /// 
    /// This method:
    /// 1. Takes ring shares from the check phase for all clients: b^1, b^2, ..., b^n
    /// 2. Aggregates them: sum = b^1 + b^2 + ... + b^n (number of clients that "match")
    /// 3. Compares aggregated sum with threshold using garbled circuits
    /// 4. Returns whether matches exceed threshold
    pub fn compare_with_threshold(
        &self,
        match_results: &[MatchResult<ModInt>],
        threshold: ModInt,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<ThresholdResult, ThresholdPhaseError> {
        // Step 1: Aggregate all ring shares
        // sum = b^1 + b^2 + ... + b^n (number of clients that "match")
        let mut aggregated_share = ModInt::new(0, self.config.modulus);
        for result in match_results {
            aggregated_share = aggregated_share + result.ring_share;
        }

        // Step 2: Compare aggregated share with threshold using garbled circuits
        // Both aggregated_share and threshold are already ModInt, so we can use them directly
        let comparison_result = if self.config.is_garbler_side {
            multiple_gb_less_than_ss(rng, channel, &[aggregated_share], &[threshold]);
            false // Garbler doesn't see the result
        } else {
            // Evaluator side - gets the actual comparison result
            let results = multiple_ev_less_than_ss(rng, channel, &[aggregated_share]);
            results[0]
        };

        Ok(ThresholdResult {
            exceeds_threshold: comparison_result,
            total_clients: match_results.len(),
        })
    }

    /// Get the configuration
    pub fn config(&self) -> &ThresholdConfig {
        &self.config
    }

    /// Create match result for a client
    pub fn create_match_result(ring_share: ModInt, client_id: usize) -> MatchResult<ModInt> {
        MatchResult {
            ring_share,
            client_id,
        }
    }
}