use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::convert::{TryFrom, TryInto};
use scuttlebutt::{AesRng, Channel, Block, AbstractChannel};
use crate::fuzzy_match::share_phase::{SharePhase, SharePhaseError, SharedRange};
use crate::garbled_circuits::equality_full::{multiple_gb_equality_test, multiple_ev_equality_test};
use crate::garbled_circuits::greater_than_or_equal_threshold::{multiple_gb_greater_than_ss, multiple_ev_greater_than_ss};
use crate::data_structures::modint::ModInt;
use crate::fss::interval::IntervalFSSKey;
use crate::util::u128_to_bits;
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use ocelot::ot::{Receiver, Sender};
use crate::{Group, Share};
use serde::{Deserialize, Serialize};

/// Method for check phase comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CheckMethod {
    /// Use L-infinity distance equality test (original fuzzy matching behavior)
    Linf,
    /// Use Lp distance comparison with garbled circuits
    LpGarbledCircuits,
    /// Use Lp distance comparison with IntervalFSS
    LpIntervalFSS,
}

/// Data for check phase configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CheckData {
    /// No additional data needed for L-infinity equality test
    Linf,
    /// Threshold value for Lp distance comparison with garbled circuits
    LpGarbledCircuits {
        /// Threshold value for comparison
        threshold: u128,
    },
    /// FSS key, random value, and threshold for Lp distance comparison with IntervalFSS
    LpIntervalFSS {
        /// Threshold value for comparison
        threshold: u128,
        /// FSS key for this server
        fss_key: IntervalFSSKey<1>,
        /// Random value for this server (r0 for server 0, r1 for server 1)
        random_value: u128,
    },
}

/// Configuration for the check phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckConfig {
    pub input_bit_length: usize,
    pub output_bit_length: usize,
    /// Number of dimensions for evaluation
    pub num_dimensions: usize,
    /// Whether this is the garbler side (true) or evaluator side (false)
    pub is_garbler_side: bool,
    /// Method to use for check phase
    pub method: CheckMethod,
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

    /// Run fuzzy match check for a d-dimensional query point against a shared range
    /// This is specifically designed for the fuzzy matching protocol
    /// Input: query_point is now a slice of Vec<bool> where each Vec<bool> represents the bits for one dimension
    pub fn run_fuzzy_match_check(
        &self,
        shared_range: &SharedRange,
        query_point: &[Vec<bool>],
        check_data: &CheckData,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<ModInt, CheckPhaseError> {
        if query_point.len() != self.config.num_dimensions {
            return Err(CheckPhaseError::InputLengthMismatch(
                format!("Query point has {} dimensions but config expects {}", query_point.len(), self.config.num_dimensions)
            ));
        }

        // Choose the appropriate check method based on configuration and data
        match (&self.config.method, check_data) {
            (CheckMethod::Linf, CheckData::Linf) => {
                self.run_linf_check(shared_range, query_point, channel, rng)
            }
            (CheckMethod::LpGarbledCircuits, CheckData::LpGarbledCircuits { threshold }) => {
                let threshold_modint = ModInt::new(*threshold, 1 << self.config.input_bit_length);
                self.run_lp_distance_check_gc(shared_range, query_point, threshold_modint, channel, rng)
            }
            (CheckMethod::LpIntervalFSS, CheckData::LpIntervalFSS { threshold, fss_key, random_value }) => {
                self.run_lp_distance_check_intervalfss(shared_range, query_point, *threshold, *random_value, fss_key, channel)
            }
            _ => {
                Err(CheckPhaseError::InvalidConfig(
                    format!("Mismatch between check method {:?} and check data type", self.config.method)
                ))
            }
        }
    }

    /// L-infinity distance equality test method (extracted for clarity)
    fn run_linf_check(
        &self,
        shared_range: &SharedRange,
        query_point: &[Vec<bool>],
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<ModInt, CheckPhaseError> {
        // Step 1: For each dimension i from 0 to d-1, evaluate query_point[i] with OKVS for dimension i
        let mut all_dimension_eval = Vec::<ModInt>::new();
        
        for dim in 0..self.config.num_dimensions {
            // Use the bits directly from query_point
            let point_bits = &query_point[dim];
            
            // Evaluate at the specific dimension
            let res = self.share_phase.evaluate_at_single_dimension(shared_range, point_bits, dim)?;
            all_dimension_eval.push(ModInt::new(res, 1 << self.config.input_bit_length));
        }

        // Step 2: Run equality test with the concatenated boolean vector
        let equality_result = if self.config.is_garbler_side {
            let result = multiple_gb_equality_test(rng, channel, &all_dimension_eval);
            result
        } else {
            let result = multiple_ev_equality_test(rng, channel, &all_dimension_eval);
            result
        };
        
        // Step 3: Convert boolean result to ring share using OT
        let ring_share = self.boolean_to_ring_share_modint(
            equality_result,
            1 << self.config.output_bit_length,
            channel,
            rng,
            self.config.is_garbler_side,
        )?;
        
        Ok(ring_share)
    }

    /// Run Lp distance check for a d-dimensional query point against a shared range
    /// This method aggregates the dimension shares and compares with threshold using garbled circuits
    /// Input: query_point is a slice of Vec<bool> where each Vec<bool> represents the bits for one dimension
    pub fn run_lp_distance_check_gc(
        &self,
        shared_range: &SharedRange,
        query_point: &[Vec<bool>],
        threshold: ModInt,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
    ) -> Result<ModInt, CheckPhaseError> {
        if query_point.len() != self.config.num_dimensions {
            return Err(CheckPhaseError::InputLengthMismatch(
                format!("Query point has {} dimensions but config expects {}", query_point.len(), self.config.num_dimensions)
            ));
        }

        // Step 1: For each dimension i from 0 to d-1, evaluate query_point[i] with OKVS for dimension i
        let mut all_dimension_eval = Vec::<ModInt>::new();
        
        for dim in 0..self.config.num_dimensions {
            // Use the bits directly from query_point
            let point_bits = &query_point[dim];
            
            // Evaluate at the specific dimension to get |query_point[i] - x[i]|^p
            let res = self.share_phase.evaluate_at_single_dimension(shared_range, point_bits, dim)?;
            all_dimension_eval.push(ModInt::new(res, 1 << self.config.input_bit_length));
        }

        // Step 2: Aggregate all dimension evaluations (sum of Lp distances across dimensions)
        let modulus = 1u128 << self.config.input_bit_length;
        let mut aggregated_share = ModInt::new(0, modulus);
        for dimension_result in &all_dimension_eval {
            aggregated_share = aggregated_share + *dimension_result;
        }

        // Step 3: Compare aggregated sum with threshold using garbled circuits
        // Check if aggregated_sum <= threshold (distance is within threshold)
        let comparison_result = if self.config.is_garbler_side {
            let results = multiple_gb_greater_than_ss(rng, channel, &[threshold], &[aggregated_share]);
            results[0] // true if threshold >= aggregated_sum (distance within threshold)
        } else {
            let results = multiple_ev_greater_than_ss(rng, channel, &[aggregated_share]);
            results[0]
        };
        
        // Step 4: Convert boolean result to ring share using OT
        let ring_share = self.boolean_to_ring_share_modint(
            comparison_result,
            1 << self.config.output_bit_length,
            channel,
            rng,
            self.config.is_garbler_side,
        )?;
        
        Ok(ring_share)
    }

    /// Run Lp distance check for a d-dimensional query point against a shared range
    /// This method aggregates the dimension shares and compares with threshold using IntervalFSS
    /// Input: query_point is a slice of Vec<bool> where each Vec<bool> represents the bits for one dimension
    pub fn run_lp_distance_check_intervalfss(
        &self,
        shared_range: &SharedRange,
        query_point: &[Vec<bool>],
        threshold: u128,
        random_value: u128,
        fss_key: &IntervalFSSKey<1>,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
    ) -> Result<ModInt, CheckPhaseError> {
        if query_point.len() != self.config.num_dimensions {
            return Err(CheckPhaseError::InputLengthMismatch(
                format!("Query point has {} dimensions but config expects {}", query_point.len(), self.config.num_dimensions)
            ));
        }

        // Step 1: For each dimension i from 0 to d-1, evaluate query_point[i] with OKVS for dimension i
        let mut all_dimension_eval = Vec::<ModInt>::new();
        
        for dim in 0..self.config.num_dimensions {
            // Use the bits directly from query_point
            let point_bits = &query_point[dim];
            
            // Evaluate at the specific dimension to get |query_point[i] - x[i]|^p
            let res = self.share_phase.evaluate_at_single_dimension(shared_range, point_bits, dim)?;
            all_dimension_eval.push(ModInt::new(res, 1 << self.config.input_bit_length));
        }

        // Step 2: Aggregate all dimension evaluations (sum of Lp distances across dimensions)
        let modulus = 1u128 << self.config.input_bit_length;
        let mut aggregated_share = ModInt::new(0, modulus);
        for dimension_result in &all_dimension_eval {
            aggregated_share = aggregated_share + *dimension_result;
        }

        println!("Aggregated distance share before masking: {}", aggregated_share.val());

        // Step 3: Add random value to aggregated share and exchange with other server
        let masked_share = aggregated_share + ModInt::new(random_value, modulus);

        println!("Masked distance share: {}", masked_share.val());
        
        let reconstructed_masked_distance = if self.config.is_garbler_side {
            // Server 1 (garbler) sends first, then receives
            let share_bytes = masked_share.val().to_le_bytes();
            channel.write_bytes(&share_bytes)
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to send masked share: {}", e)))?;
            channel.flush()
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to flush after sending: {}", e)))?;

            println!("Successfully sent masked distance share: {}", masked_share.val());
            
            let mut received_bytes = [0u8; 16];
            channel.read_bytes(&mut received_bytes)
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to receive masked share: {}", e)))?;
            let other_masked_share = u128::from_le_bytes(received_bytes);
            let other_masked_share_modint = ModInt::new(other_masked_share, modulus);

            println!("Received masked distance share: {}", other_masked_share_modint.val());
            
            masked_share + other_masked_share_modint
        } else {
            // Server 0 (evaluator) receives first, then sends
            let mut received_bytes = [0u8; 16];
            channel.read_bytes(&mut received_bytes)
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to receive masked share: {}", e)))?;
            let other_masked_share = u128::from_le_bytes(received_bytes);
            let other_masked_share_modint = ModInt::new(other_masked_share, modulus);
            
            let share_bytes = masked_share.val().to_le_bytes();
            channel.write_bytes(&share_bytes)
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to send masked share: {}", e)))?;
            channel.flush()
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to flush after sending: {}", e)))?;
            
            masked_share + other_masked_share_modint
        };

        // Step 4: Evaluate the reconstructed masked distance using FSS key
        // The reconstructed_masked_distance = actual_distance + r0 + r1
        // We want to check if actual_distance <= threshold
        // The FSS should be set up for interval [0, threshold + r0 + r1] (less than or equal)
        // Convert distance to bit representation
        let distance_bits = u128_to_bits(reconstructed_masked_distance.val(), self.config.input_bit_length);
        
        // Evaluate FSS: returns payload for the specified interval
        // Since we want to check if actual_distance <= threshold, and we have actual_distance + r0 + r1,
        // the dealer should have set up FSS for interval [0, threshold + r0 + r1]
        let fss_result = fss_key.eval_intervalFSS(&distance_bits, 2); // modulus 2 for binary output

        println!("FSS evaluation result: {:?}", fss_result);
        
        // The FSS is set up for interval [0, threshold + r0 + r1], so:
        // - If actual_distance + r0 + r1 is in [0, threshold + r0 + r1], FSS output is 1 (within threshold)
        // - If actual_distance + r0 + r1 is outside this interval, FSS output is 0 (exceeds threshold)
        let within_threshold = fss_result.val()[0] == 1;

        // Step 5: Convert boolean result to ring share using OT
        let ring_share = self.boolean_to_ring_share_modint(
            within_threshold,
            1 << self.config.output_bit_length,
            channel,
            &mut AesRng::new(),
            self.config.is_garbler_side,
        )?;
        
        Ok(ring_share)
    }


    /// Convert boolean share to ModInt ring share using OT
    pub fn boolean_to_ring_share_modint(
        &self,
        boolean_share: bool,
        modulus: u128,
        channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
        rng: &mut AesRng,
        is_garbler_side: bool,
    ) -> Result<ModInt, CheckPhaseError> {
        if is_garbler_side {
            // Garbler side: generate random shares and send via OT
            let ring_share = ModInt::random(modulus);
            let r0 = ModInt::zero(modulus) - ring_share;
            let r1 = ModInt::one(modulus) - ring_share; 
            
            let r0_block: Block = r0.clone().try_into()
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to convert r0 to Block: {:?}", e)))?;
            let r1_block: Block = r1.clone().try_into()
                .map_err(|e| CheckPhaseError::ChannelError(format!("Failed to convert r1 to Block: {:?}", e)))?;
            
            let shares = if !boolean_share {
                (r0_block, r1_block)
            } else {
                (r1_block, r0_block)
            };
            
            let mut ot = OtSender::init(channel, rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT sender init failed: {:?}", e)))?;
            
            ot.send(channel, &[shares], rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT send failed: {:?}", e)))?;
            
            // Return r1 as the garbler's share
            Ok(ring_share)
        } else {
            // Receiver side: receive share via OT
            let mut ot = OtReceiver::init(channel, rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT receiver init failed: {:?}", e)))?;
            
            let out_blocks = ot.receive(channel, &[boolean_share], rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT receive failed: {:?}", e)))?;
            
            // Convert block to u128 and create ModInt with correct modulus
            let raw_value: u128 = unsafe { std::mem::transmute(out_blocks[0]) };
            let ring_share = ModInt::new(raw_value, modulus);
            
            Ok(ring_share)
        }
    }
}
