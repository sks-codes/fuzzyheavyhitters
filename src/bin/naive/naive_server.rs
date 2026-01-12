use clap::Parser;
use mosaic::{
    channel::{listen_to, setup_parallel_channels},
    configs::cli_config::CliConfig,
    fuzzy_match::share_phase_types::DictionaryType,
    naive::protocol::NaiveProtocol,
};
use std::fs;

/// Load query points from JSON file
fn load_query_points(file_path: &str) -> Result<Vec<Vec<u128>>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read query file {}: {}", file_path, e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse query file {}: {}", file_path, e))
}

fn run_server(config_path: &str, is_server1: bool, num_threads: usize) -> Result<(), String> {
    println!(
        "Starting naive server (side: {})...",
        if is_server1 { "1" } else { "0" }
    );
    let config = CliConfig::from_file(config_path)?;
    let server_addr = if is_server1 {
        config.network.server1_addr.clone()
    } else {
        config.network.server0_addr.clone()
    };

    let share_config = config.to_share_config()?;
    let dictionary_type = share_config.dictionary_type;
    let threshold = config.protocol.match_threshold;
    let protocol = NaiveProtocol::new(share_config, is_server1, threshold);

    let client_to_server_port = if is_server1 {
        config.network.client_to_server1_port
    } else {
        config.network.client_to_server0_port
    };

    let mut client_channel = listen_to(server_addr.clone(), client_to_server_port)
        .map_err(|e| format!("Failed to listen for client connection: {}", e))?;

    let client_shares = protocol
        .receive_client_shares(&mut client_channel)
        .map_err(|e| format!("Failed to receive client shares: {}", e))?;

    println!(
        "Received {} client shares.",
        client_shares.len()
    );

    let server0_addr = config.network.server0_addr;
    let server0_to_server1_port = config.network.server0_to_server1_port;

    let mut other_server_channels = if is_server1 {
        setup_parallel_channels(true, num_threads, &server0_addr, server0_to_server1_port)
            .map_err(|e| format!("Failed to set up channels to other server: {}", e))?
    } else {
        setup_parallel_channels(false, num_threads, &server0_addr, server0_to_server1_port)
            .map_err(|e| format!("Failed to set up channels from other server: {}", e))?
    };

    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .map_err(|e| format!("Failed to build thread pool: {}", e))?;

    println!("Running server protocol...");
    match dictionary_type {
        DictionaryType::Known => {
            let query_points = load_query_points(&config.query_file)
                .map_err(|e| format!("Failed to load query points: {}", e))?;
            let results = protocol
                .run_server_known_dictionary_parallel(
                    &client_shares,
                    &query_points,
                    &thread_pool,
                    &mut other_server_channels,
                )
                .map_err(|e| format!("Failed to run server protocol: {}", e))?;
            if !is_server1 {
                let heavy_hitter_count = results.iter().filter(|&&hit| hit).count();
                println!("Heavy hitters count: {}", heavy_hitter_count);
            }
        }
        DictionaryType::Unknown => {
            let results = protocol
                .run_server_unknown_dictionary_parallel(
                    &client_shares,
                    &mut other_server_channels,
                )
                .map_err(|e| format!("Failed to run server protocol: {}", e))?;
            if !is_server1 {
                println!("Heavy hitters count: {}", results.len());
            }
        }
    }

    Ok(())
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    config: String,
    #[arg(short, long)]
    side: u8,
    #[arg(short, long, default_value_t = 2)]
    num_threads: usize,
}

fn main() {
    let args = Args::parse();
    let config_path = &args.config;
    let side = args.side == 1;
    let num_threads = args.num_threads;
    let result = run_server(config_path, side, num_threads);
    if let Err(e) = result {
        eprintln!("Error running server: {}", e);
    }
}
