//! CLI Configuration for Fuzzy Heavy Hitters Protocol
//! 
//! This module defines configuration structures for the CLI application

use serde::{Deserialize, Serialize};
use crate::fuzzy_match::share_phase::{ShareConfig, ShareMethod, ShareData, DictionaryType};
use crate::fuzzy_match::check_phase::CheckConfig;
use crate::fuzzy_match::threshold_phase::{ThresholdConfig, ThresholdMethod, ThresholdData};
use crate::protocol::ProtocolConfig;

/// CLI configuration that combines all protocol parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// Path to the synthetic data file (clusters.json)
    pub data_file: String,
    /// Path to server query points file  
    pub query_file: String,
    /// Protocol parameters
    pub protocol: ProtocolParameters,
    /// Network configuration
    pub network: NetworkConfig,
    /// Logging and output configuration
    pub output: OutputConfig,
}

/// Protocol-specific parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolParameters {
    /// L-infinity distance for fuzzy matching
    pub delta: u128,
    /// Threshold for heavy hitters detection
    pub threshold: u128,
    /// Input bit length (for coordinates)
    pub input_bit_length: usize,
    /// Output bit length (for ring operations)
    pub output_bit_length: usize,
    /// Check phase output bit length (for aggregation)
    pub check_output_bit_length: usize,
    /// Number of dimensions
    pub dimensions: usize,
    /// Share phase method ("OKVS" or "IntervalFSS")
    pub share_method: String,
    /// Dictionary type ("Known" or "Unknown")
    pub dictionary_type: String,
    /// Threshold phase method ("GarbledCircuits" or "IntervalFSS")
    pub threshold_method: String,
    /// OKVS parameters (if using OKVS sharing)
    pub okvs: Option<OkvsConfig>,
}

/// OKVS-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OkvsConfig {
    /// Random seed 1 for OKVS
    pub r1: [u8; 16],
    /// Random seed 2 for OKVS  
    pub r2: [u8; 16],
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Address for server 0
    pub server0_addr: String,
    /// Address for server 1
    pub server1_addr: String,
    /// Port for server 0
    pub server0_port: u16,
    /// Port for server 1
    pub server1_port: u16,
}

/// Output and logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Whether to enable verbose output
    pub verbose: bool,
    /// Whether to show intermediate results (for debugging)
    pub show_intermediate: bool,
    /// Output file for results (optional)
    pub output_file: Option<String>,
}

impl CliConfig {
    /// Convert CLI config to protocol config for a specific server
    pub fn to_protocol_config(&self, is_server1: bool) -> Result<ProtocolConfig, String> {
        // Convert share method and data
        let (share_method, share_data) = match self.protocol.share_method.as_str() {
            "OKVS" => {
                // Get OKVS configuration
                let okvs_config = self.protocol.okvs.as_ref()
                    .ok_or("OKVS configuration required for OKVS share method")?;

                let share_data = ShareData::OKVS {
                    r1: okvs_config.r1,
                    r2: okvs_config.r2,
                };
                
                (ShareMethod::OKVS, share_data)
            },
            "IntervalFSS" => {
                let share_data = ShareData::IntervalFSS {};
                (ShareMethod::IntervalFSS, share_data)
            },
            other => return Err(format!("Unsupported share method: {}", other)),
        };

        // Convert dictionary type
        let dictionary_type = match self.protocol.dictionary_type.as_str() {
            "Known" => DictionaryType::Known,
            "Unknown" => DictionaryType::Unknown,
            other => return Err(format!("Unsupported dictionary type: {}", other)),
        };

        let share_config = ShareConfig {
            method: share_method,
            dictionary_type,
            input_bit_length: self.protocol.input_bit_length,
            output_bit_length: self.protocol.output_bit_length,
            dimension: self.protocol.dimensions,
            data: share_data,
        };

        // Convert threshold method and data
        let threshold_method = match self.protocol.threshold_method.as_str() {
            "GarbledCircuits" => ThresholdMethod::GarbledCircuits,
            "IntervalFSS" => ThresholdMethod::IntervalFSS,
            other => return Err(format!("Unsupported threshold method: {}", other)),
        };

        let check_config = CheckConfig {
            input_bit_length: self.protocol.output_bit_length,
            output_bit_length: self.protocol.check_output_bit_length,
            num_dimensions: self.protocol.dimensions,
            is_garbler_side: is_server1,
        };

        let threshold_config = ThresholdConfig {
            input_bit_length: self.protocol.check_output_bit_length,
            is_garbler_side: is_server1,
            method: threshold_method,
        };

        Ok(ProtocolConfig {
            share_config,
            check_config,
            threshold_config,
            threshold: self.protocol.threshold,
            delta: self.protocol.delta,
        })
    }

    /// Load configuration from a JSON file
    pub fn from_file(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file {}: {}", path, e))?;
        
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file {}: {}", path, e))
    }

    /// Save configuration to a JSON file
    pub fn to_file(&self, path: &str) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write config file {}: {}", path, e))
    }
}
