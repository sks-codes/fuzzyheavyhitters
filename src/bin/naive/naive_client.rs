use mosaic::{
    configs::cli_config::CliConfig,
    channel::{connect_to, CommTrackingChannel},
    naive::share_phase::SharePhaseNaive,
    fss::dpf::DpfKey,
};
use scuttlebutt::channel::AbstractChannel;
use clap::Parser;
use std::fs;

/// Load client points directly from JSON file
fn load_client_points(file_path: &str) -> Result<Vec<Vec<u128>>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read client points file {}: {}", file_path, e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse client points file {}: {}", file_path, e))
}

fn send_client_shares(
    dpf_keys0: Vec<DpfKey<1>>,
    dpf_keys1: Vec<DpfKey<1>>,
    channel_server0: &mut CommTrackingChannel,
    channel_server1: &mut CommTrackingChannel,
) -> Result<(), String> {
    let mut data0 = Vec::new();
    data0.extend_from_slice(&(dpf_keys0.len() as u64).to_le_bytes());
    for key in &dpf_keys0 {
        let key_bytes = key.to_bytes();
        data0.extend_from_slice(&key_bytes);
    }
    let len_bytes0 = (data0.len() as u64).to_le_bytes();
    channel_server0.write_bytes(&len_bytes0)
        .map_err(|e| format!("Failed to send length to server 0: {}", e))?;
    channel_server0.write_bytes(&data0)
        .map_err(|e| format!("Failed to send shares to server 0: {}", e))?;
    channel_server0.flush()
        .map_err(|e| format!("Failed to flush to server 0: {}", e))?;

    let mut data1 = Vec::new();
    data1.extend_from_slice(&(dpf_keys1.len() as u64).to_le_bytes());
    for key in &dpf_keys1 {
        let key_bytes = key.to_bytes();
        data1.extend_from_slice(&key_bytes); 
    }
    let len_bytes1 = (data1.len() as u64).to_le_bytes();
    channel_server1.write_bytes(&len_bytes1)
        .map_err(|e| format!("Failed to send length to server 1: {}", e))?;
    channel_server1.write_bytes(&data1)
        .map_err(|e| format!("Failed to send shares to server 1: {}", e))?;
    channel_server1.flush()
        .map_err(|e| format!("Failed to flush to server 1: {}", e))?;

    Ok(())
}

fn run_client(config_path: &str) -> Result<(), String> {
    let config = CliConfig::from_file(config_path)?;
    let share_config = config.to_share_config()?;
    let client_points = load_client_points(&config.data_file)?;

    let share_phase_naive = SharePhaseNaive::new(share_config.clone());
    let mut dpf_keys0 = Vec::new();
    let mut dpf_keys1 = Vec::new();
    for point in client_points.iter() {
        let (keys0, keys1) = share_phase_naive
            .share_range(point, config.protocol.delta)
            .map_err(|e| format!("Failed to share range for point {:?}: {}", point, e))?;
        dpf_keys0.extend(keys0);
        dpf_keys1.extend(keys1);
    }

    let mut channel_server0 = connect_to(
        config.network.server0_addr,
        config.network.client_to_server0_port,
    ).map_err(|e| format!("Failed to connect to server 0: {}", e))?;
    let mut channel_server1 = connect_to(
        config.network.server1_addr,
        config.network.client_to_server1_port,
    ).map_err(|e| format!("Failed to connect to server 1: {}", e))?;

    send_client_shares(dpf_keys0, dpf_keys1, &mut channel_server0, &mut channel_server1)?;

    println!("Client shares sent successfully");

    Ok(())
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    config: String,
}

fn main() {
    let args = Args::parse();
    let config_path = &args.config;
    let result = run_client(config_path);
    if let Err(e) = result {
        eprintln!("Error running client: {}", e);
    }
}