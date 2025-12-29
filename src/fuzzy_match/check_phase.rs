use crate::channel::CommTrackingChannel;
use crate::data_structures::mod2k::Mod2k;
use crate::fss::{dpf::DpfKey, ldcf::LdcfKey, rdcf::RdcfKey};
use crate::fuzzy_match::share_phase::SharePhaseError;
use crate::garbled_circuits::{
    batch_equality_full::{batch_ev_equality_test, batch_gb_equality_test},
    less_than_or_equal_threshold::{multiple_ev_less_than_ss, multiple_gb_less_than_ss},
};
use crate::util::{bits_to_u8s, u128_to_bits_msb, u8s_to_bits};
use ocelot::ot::{Receiver, Sender};
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use scuttlebutt::{AbstractChannel, AesRng, Block};
use std::convert::TryInto;

/// Method for check phase comparison
#[derive(Debug, Clone, PartialEq)]
pub enum CheckMethod {
    GC,
    FSS,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckProperty {
    Equality,
    MuBounded,
}

/// Data for check phase configuration
#[derive(Debug, Clone)]
pub enum CheckData {
    LinfGarbledCircuits,
    LinfDpf {
        fss_key: DpfKey,
        random_value: Vec<bool>,
    },
    /// Threshold value for Lp distance comparison with garbled circuits
    LpGarbledCircuits {
        /// Threshold value for comparison
        mu: u128,
    },
    /// FSS key, random value, and threshold for Lp distance comparison with IntervalFSS
    LpIntervalFSS {
        fss_key: (LdcfKey, RdcfKey),
        random_value: u128,
    },
}

/// Configuration for the check phase
#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub h2: usize, // Input bit length of Check Phase, which is the output bit length of Share Phase
    pub h3: usize, // Output bit length of Check Phase, which is the input bit length of threshold phase
    pub d: usize,
    /// Number of dimensions for evaluation
    pub is_garbler_side: bool,
    pub property: CheckProperty,
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
}

impl CheckPhase {
    /// Create a new check phase with the given configuration
    pub fn new(config: CheckConfig) -> Self {
        Self { config }
    }

    pub fn run_batch_fuzzy_match_check(
        &self,
        evals: &[Vec<u128>],
        check_data_list: &[CheckData],
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<Vec<Mod2k>, CheckPhaseError> {
        match &self.config.property {
            CheckProperty::Equality => {
                let inputs = evals
                    .iter()
                    .map(|eval| {
                        let mut input = Vec::new();
                        for &val in eval {
                            input.extend_from_slice(&u128_to_bits_msb(val, self.config.h2));
                        }
                        input
                    })
                    .collect::<Vec<Vec<bool>>>();

                match &self.config.method {
                    CheckMethod::GC => {
                        for check_data in check_data_list.iter() {
                            match check_data {
                                CheckData::LinfGarbledCircuits => {}
                                _ => {
                                    return Err(CheckPhaseError::InvalidConfig(
                                        "Incorrect check data for garbled circuit equality testing. Currently only support LinfGarbledCircuits".to_string()
                                    ));
                                }
                            }
                        }
                        self.batch_equality_testing_gc(&inputs, channel, rng)
                    }
                    CheckMethod::FSS => {
                        let mut fss_keys = Vec::new();
                        let mut random_values = Vec::new();
                        for check_data in check_data_list.iter() {
                            match check_data {
                                CheckData::LinfDpf {
                                    fss_key,
                                    random_value,
                                } => {
                                    fss_keys.push(fss_key.clone());
                                    random_values.push(random_value.clone());
                                }
                                _ => {
                                    return Err(CheckPhaseError::InvalidConfig(
                                        "Incorrect check data for FSS equality testing. Currently only support LinfDpf".to_string()
                                    ));
                                }
                            }
                        }
                        self.batch_equality_testing_fss(&inputs, &fss_keys, &random_values, channel)
                    }
                }
            }
            CheckProperty::MuBounded => {
                let inputs = evals
                    .iter()
                    .map(|eval| {
                        let mut input = Mod2k::zero(1u128 << self.config.h2);
                        eval.iter().for_each(|&val| {
                            input = input + Mod2k::new(val, 1u128 << self.config.h2);
                        });
                        input
                    })
                    .collect::<Vec<Mod2k>>();
                match &self.config.method {
                    CheckMethod::GC => {
                        let mut current_mu = Mod2k::zero(1u128 << self.config.h2);
                        for (i, check_data) in check_data_list.iter().enumerate() {
                            match check_data {
                                CheckData::LpGarbledCircuits { mu } => {
                                    if i != 0 {
                                        if current_mu.val() != *mu {
                                            return Err(CheckPhaseError::InvalidConfig(
                                                "All mu values must be the same for garbled circuit mu-bounded check".to_string()
                                            ));
                                        }
                                    } else {
                                        current_mu = Mod2k::new(*mu, 1u128 << self.config.h2);
                                    }
                                }
                                _ => {
                                    return Err(CheckPhaseError::InvalidConfig(
                                        "Incorrect check data for garbled circuit mu-bounded check. Currently only support LpGarbledCircuits".to_string()
                                    ));
                                }
                            }
                        }
                        self.batch_mu_bounded_testing_gc(&inputs, &current_mu, channel, rng)
                    }
                    CheckMethod::FSS => {
                        let mut fss_keys = Vec::new();
                        let mut random_values = Vec::new();
                        for check_data in check_data_list.iter() {
                            match check_data {
                                CheckData::LpIntervalFSS {
                                    fss_key,
                                    random_value,
                                } => {
                                    fss_keys.push(fss_key.clone());
                                    random_values
                                        .push(Mod2k::new(*random_value, 1u128 << self.config.h2));
                                }
                                _ => {
                                    return Err(CheckPhaseError::InvalidConfig(
                                        "Incorrect check data for FSS mu-bounded check. Currently only support LpIntervalFSS".to_string()
                                    ));
                                }
                            }
                        }
                        self.batch_mu_bounded_testing_fss(
                            &inputs,
                            &fss_keys,
                            &random_values,
                            channel,
                        )
                    }
                }
            }
        }
    }

    pub fn batch_equality_testing_gc(
        &self,
        inputs: &[Vec<bool>],
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<Vec<Mod2k>, CheckPhaseError> {
        let all_equality_results = if self.config.is_garbler_side {
            batch_gb_equality_test(rng, channel, &inputs)
        } else {
            batch_ev_equality_test(rng, channel, &inputs)
        };
        let ring_shares = self.batch_boolean_to_ring_share_modint(
            &all_equality_results,
            1 << self.config.h3,
            channel,
            rng,
            self.config.is_garbler_side,
        )?;
        Ok(ring_shares)
    }

    pub fn batch_equality_testing_fss(
        &self,
        inputs: &[Vec<bool>],
        fss_keys: &[DpfKey],
        random_values: &[Vec<bool>],
        channel: &mut CommTrackingChannel,
    ) -> Result<Vec<Mod2k>, CheckPhaseError> {
        if inputs.len() != fss_keys.len() {
            return Err(CheckPhaseError::InputLengthMismatch(format!(
                "Number of inputs ({}) must match number of FSS keys ({})",
                inputs.len(),
                fss_keys.len()
            )));
        }
        if inputs.len() != random_values.len() {
            return Err(CheckPhaseError::InputLengthMismatch(format!(
                "Number of inputs ({}) must match number of random values ({})",
                inputs.len(),
                random_values.len()
            )));
        }

        let masked_values = inputs
            .iter()
            .zip(random_values.iter())
            .map(|(input, rand)| {
                input
                    .iter()
                    .zip(rand.iter())
                    .map(|(&input_bit, &rand_bit)| input_bit ^ rand_bit)
                    .collect::<Vec<bool>>()
            })
            .collect::<Vec<Vec<bool>>>();

        let masked_values_u8s = masked_values
            .iter()
            .map(|masked_eval| bits_to_u8s(masked_eval))
            .collect::<Vec<Vec<u8>>>();

        let num_values = masked_values_u8s.len();
        let values_bytes_length = masked_values_u8s[0].len();
        let values_bits_length = masked_values[0].len();

        let other_masked_values_u8s: Vec<Vec<u8>> = if self.config.is_garbler_side {
            for u8s in masked_values_u8s.iter() {
                channel.write_bytes(u8s).map_err(|e| {
                    CheckPhaseError::ChannelError(format!("Failed to send masked evals: {}", e))
                })?;
            }
            channel.flush().map_err(|e| {
                CheckPhaseError::ChannelError(format!("Failed to flush after sending: {}", e))
            })?;

            let mut out = Vec::with_capacity(num_values);
            for _ in 0..num_values {
                let mut other_masked_eval_bytes = vec![0u8; values_bytes_length];
                channel
                    .read_bytes(&mut other_masked_eval_bytes)
                    .map_err(|e| {
                        CheckPhaseError::ChannelError(format!(
                            "Failed to read other masked evals: {}",
                            e
                        ))
                    })?;
                out.push(other_masked_eval_bytes);
            }
            out
        } else {
            let mut other_masked_values_u8s = Vec::with_capacity(num_values);
            for _ in 0..num_values {
                let mut other_masked_eval_bytes = vec![0u8; values_bytes_length];
                channel
                    .read_bytes(&mut other_masked_eval_bytes)
                    .map_err(|e| {
                        CheckPhaseError::ChannelError(format!(
                            "Failed to read other masked evals: {}",
                            e
                        ))
                    })?;
                other_masked_values_u8s.push(other_masked_eval_bytes);
            }

            for u8s in masked_values_u8s.iter() {
                channel.write_bytes(u8s).map_err(|e| {
                    CheckPhaseError::ChannelError(format!("Failed to send masked evals: {}", e))
                })?;
            }
            channel.flush().map_err(|e| {
                CheckPhaseError::ChannelError(format!("Failed to flush after sending: {}", e))
            })?;
            other_masked_values_u8s
        };

        let other_masked_values = other_masked_values_u8s
            .iter()
            .map(|u8s| u8s_to_bits(u8s, values_bits_length))
            .collect::<Vec<Vec<bool>>>();

        let combined_masked_values = masked_values
            .iter()
            .zip(other_masked_values.iter())
            .map(|(eval, other_eval)| {
                eval.iter()
                    .zip(other_eval.iter())
                    .map(|(&eval_bit, &other_bit)| eval_bit ^ other_bit)
                    .collect::<Vec<bool>>()
            })
            .collect::<Vec<Vec<bool>>>();

        let result = combined_masked_values
            .iter()
            .zip(fss_keys.iter())
            .map(|(masked_eval, fss_key)| {
                let fss_result = fss_key
                    .eval_dpf(masked_eval, 1u128 << self.config.h3)
                    .map_err(|e| {
                        CheckPhaseError::SharePhaseError(SharePhaseError::EvaluationError(
                            e.to_string(),
                        ))
                    })?;
                let value = if self.config.is_garbler_side {
                    Mod2k::new(
                        (1u128 << self.config.h3) - fss_result[0],
                        1u128 << self.config.h3,
                    )
                } else {
                    Mod2k::new(fss_result[0], 1u128 << self.config.h3)
                };
                Ok(value)
            })
            .collect::<Result<Vec<Mod2k>, CheckPhaseError>>()?;

        Ok(result)
    }

    pub fn batch_mu_bounded_testing_gc(
        &self,
        inputs: &[Mod2k],
        mu: &Mod2k,
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
    ) -> Result<Vec<Mod2k>, CheckPhaseError> {
        let comparison_results = if self.config.is_garbler_side {
            multiple_gb_less_than_ss(rng, channel, inputs, mu)
        } else {
            multiple_ev_less_than_ss(rng, channel, inputs)
        };

        let ring_shares = self.batch_boolean_to_ring_share_modint(
            &comparison_results,
            1 << self.config.h3,
            channel,
            rng,
            self.config.is_garbler_side,
        )?;

        Ok(ring_shares)
    }

    pub fn batch_mu_bounded_testing_fss(
        &self,
        inputs: &[Mod2k],
        fss_keys: &[(LdcfKey, RdcfKey)],
        random_values: &[Mod2k],
        channel: &mut CommTrackingChannel,
    ) -> Result<Vec<Mod2k>, CheckPhaseError> {
        let masked_values = inputs
            .iter()
            .zip(random_values.iter())
            .map(|(&input, &random_value)| input + random_value)
            .collect::<Vec<Mod2k>>();

        let combined_masked_values = if self.config.is_garbler_side {
            for masked_eval in masked_values.iter() {
                let eval_bytes = masked_eval.val().to_le_bytes();
                channel.write_bytes(&eval_bytes).map_err(|e| {
                    CheckPhaseError::ChannelError(format!("Failed to send masked evals: {}", e))
                })?;
            }
            channel.flush().map_err(|e| {
                CheckPhaseError::ChannelError(format!("Failed to flush after sending: {}", e))
            })?;

            let mut out = Vec::with_capacity(masked_values.len());
            for &masked_value in masked_values.iter() {
                let mut received_bytes = [0u8; 16];
                channel.read_bytes(&mut received_bytes).map_err(|e| {
                    CheckPhaseError::ChannelError(format!(
                        "Failed to read other masked evals: {}",
                        e
                    ))
                })?;
                let other_masked_value =
                    Mod2k::new(u128::from_le_bytes(received_bytes), 1u128 << self.config.h2);
                out.push(masked_value + other_masked_value);
            }
            out
        } else {
            let mut combined_masked_values = Vec::with_capacity(masked_values.len());
            for &masked_value in masked_values.iter() {
                let mut received_bytes = [0u8; 16];
                channel.read_bytes(&mut received_bytes).map_err(|e| {
                    CheckPhaseError::ChannelError(format!(
                        "Failed to read other masked evals: {}",
                        e
                    ))
                })?;
                let other_masked_value =
                    Mod2k::new(u128::from_le_bytes(received_bytes), 1u128 << self.config.h2);
                combined_masked_values.push(masked_value + other_masked_value);
            }

            for masked_eval in masked_values.iter() {
                let eval_bytes = masked_eval.val().to_le_bytes();
                channel.write_bytes(&eval_bytes).map_err(|e| {
                    CheckPhaseError::ChannelError(format!("Failed to send masked evals: {}", e))
                })?;
            }
            channel.flush().map_err(|e| {
                CheckPhaseError::ChannelError(format!("Failed to flush after sending: {}", e))
            })?;
            combined_masked_values
        };

        let out_modulus = 1u128 << self.config.h3;
        let results = combined_masked_values
            .iter()
            .zip(fss_keys.iter())
            .map(|(masked_value, (fss_key0, fss_key1))| -> Result<Mod2k, CheckPhaseError> {
                let masked_value_bits = u128_to_bits_msb(masked_value.val(), self.config.h2);
                let fss_ldcf = fss_key0
                    .eval_ldcf(&masked_value_bits, out_modulus)
                    .map_err(|e| CheckPhaseError::InvalidConfig(e.to_string()))?;
                let fss_rdcf = fss_key1
                    .eval_rdcf(&masked_value_bits, out_modulus)
                    .map_err(|e| CheckPhaseError::InvalidConfig(e.to_string()))?;

                let sum = (fss_ldcf[0] + fss_rdcf[0]) % out_modulus;
                Ok(if self.config.is_garbler_side {
                    Mod2k::new(out_modulus - sum, out_modulus)
                } else {
                    Mod2k::new(sum, out_modulus)
                })
            })
            .collect::<Result<Vec<Mod2k>, CheckPhaseError>>()?;

        Ok(results)
    }

    /// Convert multiple boolean shares to ModInt ring shares using batched OT
    /// This is more efficient than calling boolean_to_ring_share_modint multiple times
    fn batch_boolean_to_ring_share_modint(
        &self,
        boolean_shares: &[bool],
        modulus: u128,
        channel: &mut CommTrackingChannel,
        rng: &mut AesRng,
        is_garbler_side: bool,
    ) -> Result<Vec<Mod2k>, CheckPhaseError> {
        if boolean_shares.is_empty() {
            return Ok(Vec::new());
        }

        if is_garbler_side {
            // Garbler side: generate random shares and send via batched OT
            let mut ring_shares = Vec::new();
            let mut ot_pairs = Vec::new();

            for &boolean_share in boolean_shares {
                let ring_share = Mod2k::random(modulus);
                let r0 = Mod2k::zero(modulus) - ring_share;
                let r1 = Mod2k::one(modulus) - ring_share;

                let r0_block: Block = r0.clone().try_into().map_err(|e| {
                    CheckPhaseError::ChannelError(format!("Failed to convert r0 to Block: {:?}", e))
                })?;
                let r1_block: Block = r1.clone().try_into().map_err(|e| {
                    CheckPhaseError::ChannelError(format!("Failed to convert r1 to Block: {:?}", e))
                })?;

                let shares = if !boolean_share {
                    (r0_block, r1_block)
                } else {
                    (r1_block, r0_block)
                };

                ot_pairs.push(shares);
                ring_shares.push(ring_share);
            }

            let mut ot = OtSender::init(channel, rng).map_err(|e| {
                CheckPhaseError::ChannelError(format!("OT sender init failed: {:?}", e))
            })?;
            ot.send(channel, &ot_pairs, rng)
                .map_err(|e| CheckPhaseError::ChannelError(format!("OT send failed: {:?}", e)))?;
            channel.flush().map_err(|e| {
                CheckPhaseError::ChannelError(format!("Failed to flush after sending: {}", e))
            })?;

            Ok(ring_shares)
        } else {
            // Receiver side: receive shares via batched OT
            let mut ot = OtReceiver::init(channel, rng).map_err(|e| {
                CheckPhaseError::ChannelError(format!("OT receiver init failed: {:?}", e))
            })?;

            let out_blocks = ot.receive(channel, boolean_shares, rng).map_err(|e| {
                CheckPhaseError::ChannelError(format!("OT receive failed: {:?}", e))
            })?;

            // Convert blocks to ModInts
            let mut ring_shares = Vec::new();
            for block in out_blocks {
                let raw_value: u128 = unsafe { std::mem::transmute(block) };
                let ring_share = Mod2k::new(raw_value, modulus);
                ring_shares.push(ring_share);
            }

            Ok(ring_shares)
        }
    }
}
