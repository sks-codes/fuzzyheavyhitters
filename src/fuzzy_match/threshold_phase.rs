//! Threshold Phase Implementation
use crate::garbled_circuits::greater_than_or_equal_threshold::{
    multiple_gb_greater_than_ss, multiple_ev_greater_than_ss};
use crate::data_structures::modint::ModInt;
use crate::data_structures::payload::RingVec;
use crate::fss::interval::IntervalFSSKey;
use crate::util::u128_to_bits;
use crate::{Share, Group};


use std::convert::TryInto;
use scuttlebutt::{AesRng, Block, AbstractChannel};
use crate::channel::CommTrackingChannel;
use serde::{Deserialize, Serialize};

/// Method for threshold comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThresholdMethod {
    /// Use garbled circuits for threshold comparison
    GarbledCircuits,
    /// Use IntervalFSS for threshold comparison
    IntervalFSS,
}

/// Data for threshold phase configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThresholdData {
    /// No additional data needed for garbled circuits
    GarbledCircuits,
    /// FSS key and random value for IntervalFSS privacy
    IntervalFSS {
        /// FSS key for this server
        fss_key: IntervalFSSKey<1>,
        /// Random value for this server (r0 for server 0, r1 for server 1)
        random_value: u128,
    },
}

/// Configuration for the threshold phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    pub input_bit_length: usize,
    /// Whether this is the garbler side (true) or evaluator side (false)
    pub is_garbler_side: bool,
    /// Method to use for threshold comparison
    pub method: ThresholdMethod,
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

/// Threshold phase handler
#[derive(Debug, Clone)]
pub struct ThresholdPhase {
    config: ThresholdConfig,
}

impl ThresholdPhase {
    /// Create a new threshold phase with the given configuration
    pub fn new(config: ThresholdConfig) -> Self {
        Self { config }
    }

    /// Aggregate match results and compare with threshold using garbled circuits
    /// 
    /// This method:
    /// 1. Takes ring shares from the check phase for all clients: b^1, b^2, ..., b^n
    /// 2. Aggregates them: sum = b^1 + b^2 + ... + b^n (number of clients that "match")
    /// 3. Compares aggregated sum with threshold using garbled circuits
    /// 4. Returns whether matches exceed threshold
    pub fn compare_with_threshold_gc(
        &self,
        match_results: &[ModInt],
        threshold: ModInt,
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<bool, ThresholdPhaseError> {
        let modulus = 1u128 << self.config.input_bit_length;
        // Step 1: Aggregate all ring shares
        // sum = b^1 + b^2 + ... + b^n (number of clients that "match")
        let mut aggregated_share = ModInt::new(0, modulus);
        for result in match_results {
            aggregated_share = aggregated_share + *result;
        }

        // Step 2: Compare aggregated share with threshold using garbled circuits
        // Both aggregated_share and threshold are already ModInt, so we can use them directly
        let comparison_result = if self.config.is_garbler_side {
            let results = multiple_gb_greater_than_ss(rng, channel, &[aggregated_share], &[threshold]);
            results[0]
        } else {
            // Evaluator side - gets the actual comparison result
            let results = multiple_ev_greater_than_ss(rng, channel, &[aggregated_share]);
            results[0]
        };

        Ok(comparison_result)
    }

    /// Compare with threshold using IntervalFSS approach
    /// 
    /// This method:
    /// 1. Takes ring shares from the check phase for all clients: b^1, b^2, ..., b^n
    /// 2. Aggregates them: sum = b^1 + b^2 + ... + b^n (number of clients that "match")
    /// 3. Each server adds their random value to their aggregated share and sends to the other server
    /// 4. Each server evaluates the reconstructed count using their FSS key for interval [threshold + r0 + r1, MAX]
    /// 5. Returns the FSS evaluation result (1 if count >= threshold, 0 otherwise)
    pub fn compare_with_threshold_intervalfss(
        &self,
        match_results: &[ModInt],
        threshold: u128,
        random_value: u128,
        fss_key: &IntervalFSSKey<1>,
        channel: &mut CommTrackingChannel,
    ) -> Result<bool, ThresholdPhaseError> {
        let modulus = 1u128 << self.config.input_bit_length;
        
        // Step 1: Aggregate all ring shares locally
        // sum = b^1 + b^2 + ... + b^n (number of clients that "match")
        let mut aggregated_share = ModInt::new(0, modulus);
        for result in match_results {
            aggregated_share = aggregated_share + *result;
        }

        // Step 2: Add random value to aggregated share and exchange with other server
        let masked_share = aggregated_share + ModInt::new(random_value, modulus);
        
        let reconstructed_masked_count = if self.config.is_garbler_side {
            // Server 1 (garbler) sends first, then receives
            let share_bytes = masked_share.val().to_le_bytes();
            channel.write_bytes(&share_bytes)
                .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to send masked share: {}", e)))?;
            channel.flush()
                .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to flush after sending: {}", e)))?;
            
            let mut received_bytes = [0u8; 16];
            channel.read_bytes(&mut received_bytes)
                .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to receive masked share: {}", e)))?;
            let other_masked_share = u128::from_le_bytes(received_bytes);
            let other_masked_share_modint = ModInt::new(other_masked_share, modulus);

            masked_share + other_masked_share_modint
        } else {
            // Server 0 (evaluator) receives first, then sends
            let mut received_bytes = [0u8; 16];
            channel.read_bytes(&mut received_bytes)
                .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to receive masked share: {}", e)))?;
            let other_masked_share = u128::from_le_bytes(received_bytes);
            let other_masked_share_modint = ModInt::new(other_masked_share, modulus);
            
            let share_bytes = masked_share.val().to_le_bytes();
            channel.write_bytes(&share_bytes)
                .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to send masked share: {}", e)))?;
            channel.flush()
                .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to flush after sending: {}", e)))?;
            
            masked_share + other_masked_share_modint
        };

        // Step 3: Evaluate the reconstructed masked count using FSS key for interval [threshold + r0 + r1, MAX]
        // The reconstructed_masked_count = actual_count + r0 + r1
        // Convert count to bit representation
        let mut count_bits = u128_to_bits(reconstructed_masked_count.val(), self.config.input_bit_length);
        count_bits.reverse(); // Reverse to MSB-first order
        
        // Evaluate FSS: returns payload for interval [threshold + r0 + r1, MAX]
        // Since we want to check if actual_count >= threshold, and we have actual_count + r0 + r1,
        // we need to check if actual_count + r0 + r1 >= threshold + r0 + r1
        let fss_result = fss_key.eval_intervalFSS(&count_bits, 2); // modulus 2 for binary output

        // The FSS is set up for interval [threshold + r0 + r1, MAX], so:
        // - If actual_count + r0 + r1 is in [threshold + r0 + r1, MAX], FSS output is 1 (threshold exceeded)
        // - If actual_count + r0 + r1 is outside [threshold + r0 + r1, MAX], FSS output is 0
        let threshold_exceeded = fss_result[0] == 1;

        Ok(threshold_exceeded)
    }

    /// Common function to compare with threshold using the configured method
    /// 
    /// This function dispatches to either garbled circuits or IntervalFSS based on the config
    pub fn compare_with_threshold(
        &self,
        match_results: &[ModInt],
        threshold: u128,
        threshold_data: &ThresholdData,
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<bool, ThresholdPhaseError> {
        match self.config.method {
            ThresholdMethod::GarbledCircuits => {
                // Verify we have the right data
                match threshold_data {
                    ThresholdData::GarbledCircuits => {},
                    _ => return Err(ThresholdPhaseError::InvalidConfig(
                        "GarbledCircuits method requires GarbledCircuits data".to_string()
                    )),
                }
                
                let modulus = 1u128 << self.config.input_bit_length;
                let threshold_modint = ModInt::new(threshold, modulus);
                self.compare_with_threshold_gc(match_results, threshold_modint, channel, rng)
            }
            ThresholdMethod::IntervalFSS => {
                let (fss_key, random_value) = match threshold_data {
                    ThresholdData::IntervalFSS { fss_key, random_value } => (fss_key, *random_value),
                    _ => return Err(ThresholdPhaseError::InvalidConfig(
                        "IntervalFSS method requires IntervalFSS data with FSS key and random value".to_string()
                    )),
                };
                
                self.compare_with_threshold_intervalfss(match_results, threshold, random_value, fss_key, channel)
            }
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &ThresholdConfig {
        &self.config
    }
}