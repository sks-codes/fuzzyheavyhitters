use crate::configs::cli_config::ProtocolParameters;

/// Enumeration of different distance metrics
#[derive(Debug, Clone, PartialEq)]
pub enum DistanceMetric {
    /// L-infinity distance (max of absolute differences)
    LInfinity,
    /// Lp distance with specified p value
    Lp { p: u32 },
}

/// Enumeration of dictionary types
#[derive(Debug, Clone, PartialEq)]
pub enum DictionaryType {
    /// Known dictionary case - exact values in range
    Known,
    /// Unknown dictionary case - all prefixes of values in range
    Unknown,
}

/// Enumeration of different sharing methods available
#[derive(Debug, Clone, PartialEq)]
pub enum ShareMethod {
    /// Use OKVS for sharing
    OKVS,
    /// Use FSS for sharing (Interval FSS for L-infinity, Distance FSS for Lp)
    FSS,
}

/// Configuration for the share phase
#[derive(Debug, Clone)]
pub struct ShareConfig {
    pub method: ShareMethod, // Share method, "OKVS" or "FSS"
    pub metric: DistanceMetric, // The distance metric to use
    pub dictionary_type: DictionaryType, // The dictionary type (known or unknown)
    pub h1: usize, // Number of bits for representing input values (u)
    pub h2: usize, // Number of bits for representing output values (v)
    pub d: usize, // Dimension of the input space
    pub sketch_modulus: u128, // Prime modulus used for sketching
    pub delta: u128, // Delta for distance 
}

impl From<ProtocolParameters> for ShareConfig {
    fn from(config: ProtocolParameters) -> Self {
        let method = match config.share_method.as_str() {
            "OKVS" => ShareMethod::OKVS,
            "FSS" => ShareMethod::FSS,
            other => panic!("Unsupported share method: {}", other),
        };

        let dictionary_type = match config.dictionary_type.as_str() {
            "Known" => DictionaryType::Known,
            "Unknown" => DictionaryType::Unknown,
            other => panic!("Unsupported dictionary type: {}", other),
        };

        let metric = match config.distance_metric.as_str() {
            "Linf" => DistanceMetric::LInfinity,
            "L1" => DistanceMetric::Lp { p: 1 },
            "L2" => DistanceMetric::Lp { p: 2 },
            "L3" => DistanceMetric::Lp { p: 3 },
            other => panic!("Unsupported distance metric: {}", other),
        };

        ShareConfig {
            method,
            metric,
            dictionary_type,
            h1: config.h1,
            h2: config.h2,
            d: config.d,
            sketch_modulus: config.sketch_modulus,
            delta: config.delta,
        }
    }
}
