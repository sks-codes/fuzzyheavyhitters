//! CLI Configuration for Fuzzy Heavy Hitters Protocol
//!
//! This module defines configuration structures for the CLI application

use crate::fuzzy_match::{
    check_phase::{CheckConfig, CheckMethod, CheckProperty},
    protocol::ProtocolConfig,
    share_types::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod},
    threshold_phase::{ThresholdConfig, ThresholdMethod},
};
use serde::{Deserialize, Serialize};

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
    /// Whether to enable sketching verification
    #[serde(default)]
    pub enable_sketch: bool,
    /// Input bit length (for coordinates)
    pub h1: usize,
    /// Output bit length (for ring operations)
    pub h2: usize,
    /// Check phase output bit length (for aggregation)
    pub h3: usize,
    /// Number of dimensions
    pub d: usize,
    /// Share phase method ("OKVS" or "IntervalFSS")
    pub share_method: String,
    /// Dictionary type ("Known" or "Unknown")
    pub dictionary_type: String,
    pub check_method: String,   // ("GC", "FSS")
    pub check_property: String, // ("Equality", "MuBounded")
    /// Threshold phase method ("GarbledCircuits" or "IntervalFSS")
    pub threshold_method: String,
    /// Distance metric ("Linf", "L1", "L2", "L3")
    pub distance_metric: String,
    /// Number of clients participating in the protocol
    pub num_clients: usize,
    /// Prime modulo for arithmetic sketching
    pub sketch_modulus: u128,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Address for server 0
    pub server0_addr: String,
    /// Address for server 1
    pub server1_addr: String,
    /// Port for server 0
    pub server0_to_server1_port: u16,
    /// Port for dealer to server 0 communication
    pub dealer_to_server0_port: u16,
    /// Port for dealer to server 1 communication
    pub dealer_to_server1_port: u16,
    /// Port for client to server 0 communication
    pub client_to_server0_port: u16,
    /// Port for client to server 1 communication
    pub client_to_server1_port: u16,
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
        let share_method = match self.protocol.share_method.as_str() {
            "OKVS" => ShareMethod::OKVS,
            "FSS" => ShareMethod::FSS,
            other => return Err(format!("Unsupported share method: {}", other)),
        };

        // Convert dictionary type
        let dictionary_type = match self.protocol.dictionary_type.as_str() {
            "Known" => DictionaryType::Known,
            "Unknown" => DictionaryType::Unknown,
            other => return Err(format!("Unsupported dictionary type: {}", other)),
        };

        // Convert distance metric based on the explicit distance_metric field
        let distance_metric = match self.protocol.distance_metric.as_str() {
            "Linf" => DistanceMetric::LInfinity,
            "L1" => DistanceMetric::Lp { p: 1 },
            "L2" => DistanceMetric::Lp { p: 2 },
            "L3" => DistanceMetric::Lp { p: 3 },
            other => return Err(format!("Unsupported distance metric: {}", other)),
        };

        let share_config = ShareConfig {
            method: share_method,
            dictionary_type,
            metric: distance_metric,
            h1: self.protocol.h1,
            h2: self.protocol.h2,
            d: self.protocol.d,
            sketch_modulus: self.protocol.sketch_modulus,
            delta: self.protocol.delta,
        };

        // Convert check method
        let check_method = match self.protocol.check_method.as_str() {
            "GC" => CheckMethod::GC,
            "FSS" => CheckMethod::FSS,
            other => return Err(format!("Unsupported check method: {}", other)),
        };
        let check_property = match self.protocol.check_property.as_str() {
            "Equality" => CheckProperty::Equality,
            "MuBounded" => CheckProperty::MuBounded,
            other => return Err(format!("Unsupported check property: {}", other)),
        };

        let check_config = CheckConfig {
            h2: self.protocol.h2,
            h3: self.protocol.h3,
            d: self.protocol.d,
            is_garbler_side: is_server1,
            property: check_property,
            method: check_method,
        };

        // Convert threshold method and data
        let threshold_method = match self.protocol.threshold_method.as_str() {
            "GC" => ThresholdMethod::GC,
            "FSS" => ThresholdMethod::FSS,
            other => return Err(format!("Unsupported threshold method: {}", other)),
        };

        let threshold_config = ThresholdConfig {
            h3: self.protocol.h3,
            is_garbler_side: is_server1,
            method: threshold_method,
        };

        Ok(ProtocolConfig {
            share_config,
            check_config,
            threshold_config,
            threshold: self.protocol.threshold,
            delta: self.protocol.delta,
            num_clients: self.protocol.num_clients,
            enable_sketch: self.protocol.enable_sketch,
        })
    }

    /// Convert CLI config to share config for client
    pub fn to_share_config(&self) -> Result<ShareConfig, String> {
        // Convert share method and data
        let share_method = match self.protocol.share_method.as_str() {
            "OKVS" => ShareMethod::OKVS,
            "FSS" => ShareMethod::FSS,
            other => return Err(format!("Unsupported share method: {}", other)),
        };

        // Convert dictionary type
        let dictionary_type = match self.protocol.dictionary_type.as_str() {
            "Known" => DictionaryType::Known,
            "Unknown" => DictionaryType::Unknown,
            other => return Err(format!("Unsupported dictionary type: {}", other)),
        };

        // Convert distance metric based on the explicit distance_metric field
        let distance_metric = match self.protocol.distance_metric.as_str() {
            "Linf" => DistanceMetric::LInfinity,
            "L1" => DistanceMetric::Lp { p: 1 },
            "L2" => DistanceMetric::Lp { p: 2 },
            "L3" => DistanceMetric::Lp { p: 3 },
            other => return Err(format!("Unsupported distance metric: {}", other)),
        };

        Ok(ShareConfig {
            method: share_method,
            dictionary_type,
            metric: distance_metric,
            h1: self.protocol.h1,
            h2: self.protocol.h2,
            d: self.protocol.d,
            sketch_modulus: self.protocol.sketch_modulus,
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

/// Generate a sample configuration file
pub fn generate_config(output_path: &str) -> Result<(), String> {
    let sample_config = CliConfig {
        data_file: "data/synthetic/client_points.json".to_string(),
        query_file: "data/synthetic/server_points.json".to_string(),
        protocol: ProtocolParameters {
            delta: 5,
            threshold: 3,
            enable_sketch: false,
            h1: 10,
            h2: 16,
            h3: 20, // output_bit_length + 4 for aggregation
            d: 2,
            share_method: "OKVS".to_string(), // Can also be "IntervalFSS"
            dictionary_type: "Known".to_string(), // Can also be "Unknown"
            check_method: "FSS".to_string(),  // Can also be "GC"
            check_property: "Equality".to_string(), // Can also be "MuBounded"
            threshold_method: "GC".to_string(), // Can also be "IntervalFSS"
            distance_metric: "Linf".to_string(), // Can also be "L1", "L2", "L3"
            num_clients: 100,                 // Number of clients participating in the protocol
            sketch_modulus: 1362378130168812918549609490751, // A prime number of 100 bits
        },
        network: NetworkConfig {
            server0_addr: "127.0.0.1".to_string(),
            server1_addr: "127.0.0.1".to_string(),
            server0_to_server1_port: 8000,
            dealer_to_server0_port: 9000,
            dealer_to_server1_port: 9001,
            client_to_server0_port: 7000,
            client_to_server1_port: 7001,
        },
        output: OutputConfig {
            verbose: true,
            show_intermediate: false,
            output_file: Some("results.json".to_string()),
        },
    };

    sample_config.to_file(output_path)?;
    println!("Sample configuration written to {}", output_path);
    Ok(())
}
