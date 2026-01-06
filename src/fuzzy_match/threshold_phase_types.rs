use crate::{
    fss::{ldcf::LdcfKey, rdcf::RdcfKey},
    configs::cli_config::ProtocolParameters,
};

/// Method for threshold comparison
#[derive(Debug, Clone, PartialEq)]
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
    GarbledCircuits { t: u128 },
    /// FSS key and random value for IntervalFSS privacy
    IntervalFSS {
        /// FSS key for this server
        fss_key: (LdcfKey, RdcfKey),
        /// Random value for this server (r0 for server 0, r1 for server 1)
        random_value: u128,
    },
}

/// Configuration for the threshold phase
#[derive(Debug, Clone)]
pub struct ThresholdConfig {
    pub h3: usize,
    /// Method to use for threshold comparison
    pub method: ThresholdMethod,
}

impl From<ProtocolParameters> for ThresholdConfig {
    fn from(config: ProtocolParameters) -> Self {
        let method = match config.threshold_method.as_str() {
            "GC" => ThresholdMethod::GC,
            "FSS" => ThresholdMethod::FSS,
            other => panic!("Unsupported check method: {}. only support 'GC' and 'FSS'.", other),
        };

        ThresholdConfig {
            h3: config.h3,
            method,
        }
    }
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
