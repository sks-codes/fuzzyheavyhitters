use serde::Serialize;

#[derive(Serialize)]
pub struct Experiment {
    pub metadata: Metadata,
    pub parameters: Parameters,
    pub client_side: ClientSide,
    pub server_side: Vec<ServerSideLevel>,
    experiment_results: ExperimentResults
}

#[derive(Serialize)]
pub struct Metadata {
    pub experiment_id: String,
    pub date: String,         // Use chrono::Utc::now().to_rfc3339()
    pub git_commit: Option<String>,
}

#[derive(Serialize)]
pub struct Parameters {
    pub num_clients: usize,
    pub dimensions: usize,
    pub string_length: usize,
    pub threshold: usize,
    pub ball_radius: f64,
    pub num_threads: usize,
}

#[derive(Serialize)]
pub struct ClientSide {
    pub key_gen_time_avg_ms: f64,
    pub key_size_avg_bytes: usize,
}

#[derive(Serialize)]
pub struct ServerSideLevel {
    pub total_level_time_ms: f64,
    pub time_breakdown: TimeBreakdown,
    pub nodes_searched: Vec<usize>,
}

#[derive(Serialize)]
pub struct ExperimentResults {
    pub total_time_ms: f64,
    pub num_heavy_hitters: usize,
}

#[derive(Serialize)]
pub struct TimeBreakdown {
    pub FSS: f64,
    pub GCequality: f64,
    pub OTconvert: f64,
    pub GCCompare: f64,
}