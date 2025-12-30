use rand::Rng;

use crate::{
    channel::CommTrackingChannel,
    fss::{distance::DistanceFSSEval, interval::IntervalFSSEval},
    fuzzy_match::{
        share_okvs_strategies::{
            KnownLInfinityStrategy, KnownLpStrategy, UnknownLInfinityStrategy, UnknownLpStrategy,
        },
        shared_range::SharedRange,
    },
};
use scuttlebutt::AbstractChannel;

// Re-export legacy names for external callers/tests.
pub use super::share_types::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod};
pub use super::shared_range::ShareData;

use std::convert::TryInto;

/// Share phase handler
#[derive(Clone)]
pub struct SharePhase {
    pub(super) config: ShareConfig,
}

impl SharePhase {
    /// Create a new share phase with the given configuration
    pub fn new<C: Into<ShareConfig>>(config: C) -> Self {
        Self {
            config: config.into(),
        }
    }

    /// Share a range [x-delta, x+delta] using the configured method
    /// Input x is a d-dimensional vector, output will be 2 SharedRange (one for each server)
    /// For Linf: share0 = share1 if inside interval, share0 != share1 otherwise
    /// For Lp: SUM share0+share1 mod = distance
    pub fn share_range(
        &self,
        x: &[u128],
        delta: u128,
    ) -> Result<(SharedRange, SharedRange), SharePhaseError> {
        assert_eq!(
            x.len(),
            self.config.d,
            "Input x must match the configured dimension"
        );
        // Calculate the maximum value for u bits
        let max_input = (1u128 << self.config.h1) - 1;

        // Validate that x is within the valid range
        for &xi in x {
            if xi > max_input {
                return Err(SharePhaseError::InvalidRange(format!(
                    "Input x ({}) exceeds maximum value for {}-bit input ({})",
                    xi, self.config.h1, max_input
                )));
            }
        }

        let left_bound = x.iter()
            .map(|&xi| xi.saturating_sub(delta))
            .collect::<Vec<u128>>();

        let right_bound = x.iter()
            .map(|&xi| xi.saturating_add(delta).min(max_input))
            .collect::<Vec<u128>>();

        match &self.config.method {
            ShareMethod::OKVS => {
                let mut rng = rand::rng();
                let r1 = (0..self.config.d)
                    .map(|_| rng.random::<[u8; 16]>())
                    .collect::<Vec<_>>();
                let r2 = (0..self.config.d)
                    .map(|_| rng.random::<[u8; 16]>())
                    .collect::<Vec<_>>();
                match self.config.metric {
                    DistanceMetric::LInfinity => match self.config.dictionary_type {
                        DictionaryType::Known => self.share_with_okvs(
                            &x,
                            &left_bound,
                            &right_bound,
                            &r1,
                            &r2,
                            KnownLInfinityStrategy,
                        ),
                        DictionaryType::Unknown => self.share_with_okvs(
                            &x,
                            &left_bound,
                            &right_bound,
                            &r1,
                            &r2,
                            UnknownLInfinityStrategy,
                        ),
                    },
                    DistanceMetric::Lp { p } => match self.config.dictionary_type {
                        DictionaryType::Known => self.share_with_okvs(
                            &x,
                            &left_bound,
                            &right_bound,
                            &r1,
                            &r2,
                            KnownLpStrategy { p: p },
                        ),
                        DictionaryType::Unknown => self.share_with_okvs(
                            &x,
                            &left_bound,
                            &right_bound,
                            &r1,
                            &r2,
                            UnknownLpStrategy { p: p },
                        ),
                    },
                }
            }
            ShareMethod::FSS => match self.config.metric {
                DistanceMetric::LInfinity => {
                    self.share_with_interval_fss(&left_bound, &right_bound)
                }
                DistanceMetric::Lp { p } => {
                    if p > 3 {
                        return Err(SharePhaseError::InvalidRange(
                            "Distance FSS only supports p <= 3 for practical experiments"
                                .to_string(),
                        ));
                    }
                    let modulus_mask = (1u128 << self.config.h2) - 1;
                    match p {
                        1 => self.share_with_distance_fss_l1(
                            &x,
                            &left_bound,
                            &right_bound,
                            (delta + 1) & modulus_mask,
                        ),
                        2 => self.share_with_distance_fss_l2(
                            &x,
                            &left_bound,
                            &right_bound,
                            (delta * delta + 1) & modulus_mask,
                        ),
                        3 => self.share_with_distance_fss_l3(
                            &x,
                            &left_bound,
                            &right_bound,
                            (((delta * delta) & modulus_mask) * delta + 1) & modulus_mask,
                        ),
                        _ => {
                            return Err(SharePhaseError::InvalidRange(
                                "Distance FSS only supports p in range 1-3".to_string(),
                            ))
                        }
                    }
                }
            },
        }
    }

    // Return shares of evaluation, where (y0 + y1) mod = true_result
    pub fn evaluate_at_single_dimension(
        &self,
        shared_range: &SharedRange,
        point_bits: &[bool],
        dimension: usize,
    ) -> Result<u128, SharePhaseError> {
        // Return 0 if point_bits is empty
        if point_bits.is_empty() {
            return Ok(0);
        }

        match shared_range {
            SharedRange::OKVS {
                okvs_shares,
                okvs_seeds,
                role,
                p: _,
            } => {
                let (r1, r2) = (okvs_seeds[dimension].0, okvs_seeds[dimension].1);
                let result = self.evaluate_okvs_generic(
                    &okvs_shares[dimension],
                    point_bits,
                    *role,
                    &r1,
                    &r2,
                )?;
                Ok(result)
            }
            SharedRange::IntervalFSS { keys, role: _ } => {
                let result =
                    self.evaluate_interval_fss_at_single_dimension(&keys[dimension], point_bits)?;
                Ok(result)
            }
            SharedRange::DistanceFSSL1 { keys, role } => {
                let result =
                    self.evaluate_distance_fss::<2>(&keys[dimension], point_bits, *role)?;
                Ok(result)
            }
            SharedRange::DistanceFSSL2 { keys, role } => {
                let result =
                    self.evaluate_distance_fss::<3>(&keys[dimension], point_bits, *role)?;
                Ok(result)
            }
            SharedRange::DistanceFSSL3 { keys, role } => {
                let result =
                    self.evaluate_distance_fss::<4>(&keys[dimension], point_bits, *role)?;
                Ok(result)
            }
        }
    }

    pub fn expand_prefix(
        &self,
        shared_range: &SharedRange,
        share_data: &ShareData,
        prefix: &[bool],
        dimension: usize,
    ) -> Result<(ShareData, ShareData), SharePhaseError> {
        match shared_range {
            SharedRange::OKVS {
                okvs_shares,
                okvs_seeds,
                role,
                p: _,
            } => match share_data {
                ShareData::OKVS { eval } => {
                    self.expand_prefix_okvs(okvs_shares, okvs_seeds, *role, prefix, eval, dimension)
                }
                _ => Err(SharePhaseError::InvalidShareData(
                    "Expected OKVS share data for OKVS shared range".to_string(),
                )),
            },
            SharedRange::IntervalFSS { keys, role: _ } => match share_data {
                ShareData::IntervalFSS { data } => {
                    self.expand_prefix_interval_fss(keys, data, dimension)
                }
                _ => Err(SharePhaseError::InvalidShareData(
                    "Expected IntervalFSS share data for IntervalFSS shared range".to_string(),
                )),
            },
            SharedRange::DistanceFSSL1 { keys, role } => match share_data {
                ShareData::DistanceFSSL1 { data, eval } => {
                    self.expand_prefix_distance_fss::<2>(keys, prefix, data, eval, dimension, *role)
                }
                _ => Err(SharePhaseError::InvalidShareData(
                    "Expected DistanceFSS share data for DistanceFSSL1 shared range".to_string(),
                )),
            },
            SharedRange::DistanceFSSL2 { keys, role } => match share_data {
                ShareData::DistanceFSSL2 { data, eval } => {
                    self.expand_prefix_distance_fss::<3>(keys, prefix, data, eval, dimension, *role)
                }
                _ => Err(SharePhaseError::InvalidShareData(
                    "Expected DistanceFSS share data for DistanceFSSL2 shared range".to_string(),
                )),
            },
            SharedRange::DistanceFSSL3 { keys, role } => match share_data {
                ShareData::DistanceFSSL3 { data, eval } => {
                    self.expand_prefix_distance_fss::<4>(keys, prefix, data, eval, dimension, *role)
                }
                _ => Err(SharePhaseError::InvalidShareData(
                    "Expected DistanceFSS share data for DistanceFSSL3 shared range".to_string(),
                )),
            },
        }
    }

    pub fn share_data_init(
        &self,
        shared_range: &SharedRange,
    ) -> Result<ShareData, SharePhaseError> {
        let empty_prefix = vec![vec![]; self.config.d];
        let evals = (0..self.config.d)
            .map(|dim| {
                self.evaluate_at_single_dimension(shared_range, &empty_prefix[dim], dim)
                    .map_err(SharePhaseError::from)
            })
            .collect::<Result<Vec<u128>, _>>()?;

        let modulus = 1u128 << self.config.h2;

        match shared_range {
            SharedRange::OKVS {
                okvs_shares: _,
                okvs_seeds: _,
                role: _,
                p: _,
            } => Ok(ShareData::OKVS { eval: evals }),
            SharedRange::IntervalFSS { keys, role: _ } => Ok(ShareData::IntervalFSS {
                data: keys
                    .iter()
                    .map(|key| {
                        key.init_eval(modulus)
                            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))
                    })
                    .collect::<Result<Vec<IntervalFSSEval>, _>>()?,
            }),
            SharedRange::DistanceFSSL1 { keys, role: _ } => Ok(ShareData::DistanceFSSL1 {
                data: keys
                    .iter()
                    .map(|key| {
                        key.init_eval(modulus)
                            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))
                    })
                    .collect::<Result<Vec<DistanceFSSEval<2>>, _>>()?,
                eval: evals,
            }),
            SharedRange::DistanceFSSL2 { keys, role: _ } => Ok(ShareData::DistanceFSSL2 {
                data: keys
                    .iter()
                    .map(|key| {
                        key.init_eval(modulus)
                            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))
                    })
                    .collect::<Result<Vec<DistanceFSSEval<3>>, _>>()?,
                eval: evals,
            }),
            SharedRange::DistanceFSSL3 { keys, role: _ } => Ok(ShareData::DistanceFSSL3 {
                data: keys
                    .iter()
                    .map(|key| {
                        key.init_eval(modulus)
                            .map_err(|e| SharePhaseError::EvaluationError(e.to_string()))
                    })
                    .collect::<Result<Vec<DistanceFSSEval<4>>, _>>()?,
                eval: evals,
            }),
        }
    }
}

#[allow(dead_code)]
fn send_u128(channel: &mut CommTrackingChannel, value: u128) -> Result<(), SharePhaseError> {
    let buf = value.to_le_bytes();
    channel
        .write_bytes(&buf)
        .map_err(|e| SharePhaseError::EvaluationError(format!("Failed to send u128: {}", e)))?;
    Ok(())
}

#[allow(dead_code)]
fn recv_u128(channel: &mut CommTrackingChannel) -> Result<u128, SharePhaseError> {
    let mut buf = [0u8; 16];
    channel
        .read_bytes(&mut buf)
        .map_err(|e| SharePhaseError::EvaluationError(format!("Failed to receive u128: {}", e)))?;
    Ok(u128::from_le_bytes(buf))
}

#[allow(dead_code)]
fn send_array(
    channel: &mut CommTrackingChannel,
    array: &Vec<Vec<u128>>,
    length: usize,
    width: usize,
) -> Result<(), SharePhaseError> {
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&array.len().to_le_bytes());
    for l in 0..length {
        for w in 0..width {
            buffer.extend_from_slice(&array[l][w].to_le_bytes());
        }
    }
    let len_buf: [u8; 8] = buffer.len().to_le_bytes().try_into().unwrap();
    channel.write_bytes(&len_buf).map_err(|e| {
        SharePhaseError::EvaluationError(format!("Failed to send array length: {}", e))
    })?;
    channel.write_bytes(&buffer).map_err(|e| {
        SharePhaseError::EvaluationError(format!("Failed to send array data: {}", e))
    })?;
    Ok(())
}

#[allow(dead_code)]
fn recv_array(
    channel: &mut CommTrackingChannel,
    array: &mut Vec<Vec<u128>>,
    length: usize,
    width: usize,
) -> Result<(), SharePhaseError> {
    let mut len_buf = [0u8; 8];
    channel.read_bytes(&mut len_buf).map_err(|e| {
        SharePhaseError::EvaluationError(format!("Failed to receive array length: {}", e))
    })?;
    let data_len = usize::from_le_bytes(len_buf);
    let mut buffer = vec![0u8; data_len];
    channel.read_bytes(&mut buffer).map_err(|e| {
        SharePhaseError::EvaluationError(format!("Failed to receive array data: {}", e))
    })?;
    let mut offset = 0;
    for l in 0..length {
        for w in 0..width {
            let mut num_buf = [0u8; 16];
            num_buf.copy_from_slice(&buffer[offset..offset + 16]);
            array[l][w] = u128::from_le_bytes(num_buf);
            offset += 16;
        }
    }
    Ok(())
}

/// Errors that can occur during the share phase
#[derive(Debug, Clone)]
pub enum SharePhaseError {
    /// Invalid range parameters
    InvalidRange(String),
    /// Invalid share data for the given shared range
    InvalidShareData(String),
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
            SharePhaseError::InvalidShareData(msg) => write!(f, "Invalid share data: {}", msg),
        }
    }
}

impl std::error::Error for SharePhaseError {}

impl From<crate::okvs_f2k::OkvsError> for SharePhaseError {
    fn from(err: crate::okvs_f2k::OkvsError) -> Self {
        SharePhaseError::OKVSError(err.to_string())
    }
}
