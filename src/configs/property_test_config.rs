use serde::Deserialize;

#[derive(Deserialize)]
pub struct BenchmarkConfig {
    pub dealer_addr: String,
    pub server0_addr: String,
    pub server0_to_server1_port: String,
    pub server0_to_dealer_port: String,
    pub server1_to_dealer_port: String,
    pub h1: usize,
    pub h2: usize,
    pub h3: usize,
    pub d: usize,
    pub num_clients: usize,
    pub mu: u128,
    pub threshold: u128,
}

impl BenchmarkConfig {
    pub fn from_file(file_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config_str = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read file {}: {}", file_path, e))?;
        serde_json::from_str(&config_str).map_err(|e| format!("Failed to parse JSON: {}", e).into())
    }
}
