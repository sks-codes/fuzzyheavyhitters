use crate::garbled_circuits::greater_than_or_equal_threshold::{
    multiple_gb_greater_than_ss, multiple_ev_greater_than_ss};
use crate::data_structures::{
    modint::ModInt,
};
use crate::fss::{
    ldcf::LdcfKey,
    rdcf::RdcfKey,
};
use crate::util::u128_to_bits_msb;
use crate::channel::CommTrackingChannel;
use scuttlebutt::{AesRng, AbstractChannel};

/// Method for threshold comparison
#[derive(Debug, Clone)]
pub enum ThresholdMethod {
    /// Use garbled circuits for threshold comparison
    GC,
    /// Use IntervalFSS for threshold comparison
    FSS,
}

/// Data for threshold phase configuration
#[derive(Debug, Clone)]
pub enum ThresholdData {
    /// No additional data needed for garbled circuits
    GarbledCircuits {
        t: u128,
    },
    /// FSS key and random value for IntervalFSS privacy
    IntervalFSS {
        /// FSS key for this server
        fss_key: (LdcfKey<1>, RdcfKey<1>),
        /// Random value for this server (r0 for server 0, r1 for server 1)
        random_value: u128,
    },
}

/// Configuration for the threshold phase
#[derive(Debug, Clone)]
pub struct ThresholdConfig {
    pub h3: usize,
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

    pub fn aggregate_match_results(
        &self,
        match_results: &[ModInt],
    ) -> Result<ModInt, ThresholdPhaseError> {
        let modulus = 1u128 << self.config.h3;
        let mut aggregated_share = ModInt::new(0, modulus);
        for result in match_results {
            aggregated_share = aggregated_share + *result;
        }
        Ok(aggregated_share)
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
    ) -> Result<Vec<bool>, ThresholdPhaseError> {
        // Step 2: Compare aggregated share with threshold using garbled circuits
        // Both aggregated_share and threshold are already ModInt, so we can use them directly
        let comparison_result = if self.config.is_garbler_side {
            multiple_gb_greater_than_ss(rng, channel, match_results, &threshold)
        } else {
            // Evaluator side - gets the actual comparison result
            multiple_ev_greater_than_ss(rng, channel, match_results)
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
        random_values: &[ModInt],
        fss_keys: &[(LdcfKey<1>, RdcfKey<1>)],
        channel: &mut CommTrackingChannel,
    ) -> Result<Vec<bool>, ThresholdPhaseError> {
        let modulus = 1u128 << self.config.h3;
        let masked_values = match_results.iter().zip(random_values.iter()).map(|(&input, &random_value)| {
            input + random_value
        }).collect::<Vec<ModInt>>();

        let combined_masked_values = if self.config.is_garbler_side {
            masked_values.iter().for_each(|masked_eval| {
                let eval_bytes = masked_eval.val().to_le_bytes();
                channel.write_bytes(&eval_bytes)
                    .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to send masked evals: {}", e)));
            });
            channel.flush().map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to flush after sending: {}", e)));

            masked_values.iter().map(|&masked_value| {
                let mut received_bytes = [0u8; 16];
                channel.read_bytes(&mut received_bytes)
                    .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to read other masked evals: {}", e)));
                let other_masked_value = ModInt::new(u128::from_le_bytes(received_bytes), 1u128 << self.config.h3);
                masked_value + other_masked_value
            }).collect::<Vec<ModInt>>()
        } else {
            let combined_masked_values = masked_values.iter().map(|&masked_value| {
                let mut received_bytes = [0u8; 16];
                channel.read_bytes(&mut received_bytes)
                    .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to read other masked evals: {}", e)));
                let other_masked_value = ModInt::new(u128::from_le_bytes(received_bytes), 1u128 << self.config.h3);
                masked_value + other_masked_value
            }).collect::<Vec<ModInt>>();

            masked_values.iter().for_each(|masked_eval| {
                let eval_bytes = masked_eval.val().to_le_bytes();
                channel.write_bytes(&eval_bytes)
                    .map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to send masked evals: {}", e)));
            });
            channel.flush().map_err(|e| ThresholdPhaseError::ChannelError(format!("Failed to flush after sending: {}", e)));
            combined_masked_values
        };

        let out_modulus = 1u128 << self.config.h3;
        let threshold_exceeded = combined_masked_values.iter().zip(fss_keys.iter()).map(|(masked_value, (fss_key0, fss_key1))| {
            let mut masked_value_bits = u128_to_bits_msb(masked_value.val(), self.config.h3);
            let fss_result = fss_key0.eval_ldcf(&masked_value_bits, out_modulus) + fss_key1.eval_rdcf(&masked_value_bits, out_modulus);
            fss_result[0] == 1
        }).collect::<Vec<bool>>();

        Ok(threshold_exceeded)
    }

    /// Common function to compare with threshold using the configured method
    /// 
    /// This function dispatches to either garbled circuits or IntervalFSS based on the config
    pub fn compare_with_threshold(
        &self,
        aggregated_results: &[ModInt],
        threshold_data_list: &[ThresholdData],
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<Vec<bool>, ThresholdPhaseError> {
        match self.config.method {
            ThresholdMethod::GC => {
                let mut current_t = ModInt::zero(1u128<<self.config.h3);
                for (i, threshold_data) in threshold_data_list.iter().enumerate() {
                    match threshold_data {
                        ThresholdData::GarbledCircuits { t } => {
                            if i != 0 {
                                if *t != current_t.val() {
                                    return Err(ThresholdPhaseError::InvalidConfig(
                                        "All GarbledCircuits thresholds must be the same".to_string(),
                                    ));
                                }
                            } else {
                                current_t = ModInt::new(*t, 1u128 << self.config.h3);
                            }
                        },
                        _ => return Err(ThresholdPhaseError::InvalidConfig(
                            "Garbled circuits method requires GarbledCircuits data".to_string()
                        )),
                    }
                }
                self.compare_with_threshold_gc(aggregated_results, current_t, channel, rng)
            }
            ThresholdMethod::FSS => {
                let mut fss_keys = Vec::new();
                let mut random_values = Vec::new();
                for threshold_data in threshold_data_list {
                    match threshold_data {
                        ThresholdData::IntervalFSS { fss_key, random_value } => {
                            fss_keys.push(fss_key.clone());
                            random_values.push(ModInt::new(*random_value, 1u128 << self.config.h3));
                        },
                        _ => return Err(ThresholdPhaseError::InvalidConfig(
                            "FSS method requires FSS data with FSS key and random value".to_string()
                        )),
                    }
                }
                self.compare_with_threshold_intervalfss(aggregated_results, &random_values, &fss_keys, channel)
            }
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &ThresholdConfig {
        &self.config
    }
}