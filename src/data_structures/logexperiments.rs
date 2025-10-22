use std::fs::OpenOptions;
use serde::{Deserialize, Serialize};
use std::io::Write;
#[derive(Debug, Serialize, Deserialize)]
pub struct Experiment {
    pub metadata: Metadata,
    pub parameters: Parameters,
    pub client_side: ClientSide,
    pub server_side: Vec<(ServerSide,ServerSide)>,
    pub experiment_results: ExperimentResults
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub experiment_id: String,
    pub date: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Parameters {
    pub num_clients: usize,
    pub dimensions: usize,
    pub string_length: usize,
    pub threshold: usize,
    pub ball_radius: u32
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientSide {
    pub key_gen_time_avg_ms: f64,
    pub key_size_bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerSide {
    pub total_level_time: f64,
    pub time_breakdown: TimeBreakdown,
    pub num_threads: usize,
    pub nodes_searched: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExperimentResults {
    pub total_time: f64,
    pub num_heavy_hitters: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimeBreakdown {
    pub fss: f64,
    pub gc_equality: f64,
    pub field_actions : f64,
    pub gc_compare: f64

}

pub fn log_experiment_to_json(exp: &Experiment, file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;
    serde_json::to_writer(&mut file, exp)?;
    writeln!(&mut file)?;
    Ok(())
}