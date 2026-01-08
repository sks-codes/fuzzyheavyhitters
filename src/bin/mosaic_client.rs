use clap::Parser;
use mosaic::{channel::connect_to, configs::cli_config::CliConfig, fuzzy_match::client::Client};
use std::fs;

/// Load client points directly from JSON file
fn load_client_points(file_path: &str) -> Result<Vec<Vec<u128>>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read client points file {}: {}", file_path, e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse client points file {}: {}", file_path, e))
}

/// Run as client - generates shares and sends them to servers
fn run_client(config_path: &str) -> Result<(), String> {
    let start_time = std::time::Instant::now();
    println!("Starting Client...");
    let cli_config = CliConfig::from_file(config_path)?;

    // Load client data directly from client_points.json
    println!("Loading client data from {}", cli_config.data_file);
    let client_points = load_client_points(&cli_config.data_file)?;

    println!("Loaded {} client points", client_points.len());

    // Generate client shares
    let share_start = std::time::Instant::now();
    let share_config = cli_config.to_share_config()?; // Client uses share config
    let enable_sketch = cli_config.protocol.enable_sketch;
    let num_clients = cli_config.protocol.num_clients;
    let client = Client::new(share_config, enable_sketch, num_clients);
    let (shares_server0, shares_server1) = client
        .generate_client_shares(&client_points)
        .map_err(|e| e.to_string())?;
    let share_time = share_start.elapsed();

    println!("Connecting to servers...");
    let mut channel_server0 = connect_to(
        cli_config.network.server0_addr,
        cli_config.network.client_to_server0_port,
    )
    .map_err(|e| format!("Failed to connect to server 0: {}", e))?;
    let mut channel_server1 = connect_to(
        cli_config.network.server1_addr,
        cli_config.network.client_to_server1_port,
    )
    .map_err(|e| format!("Failed to connect to server 1: {}", e))?;

    // Send shares to both servers
    println!("Sending shares to servers...");
    let send_start = std::time::Instant::now();
    client
        .send_client_shares(
            shares_server0,
            shares_server1,
            &mut channel_server0,
            &mut channel_server1,
        )
        .map_err(|e| e.to_string())?;
    let send_time = send_start.elapsed();

    println!("Client sending PC-FSS keys took {:.2?}", send_time);

    let sketch_start = std::time::Instant::now();
    if enable_sketch {
        let (sketches0, sketches1) = client
            .generate_client_sketch_data()
            .map_err(|e| e.to_string())?;

        println!("Client generating sketch data took {:.2?}", sketch_start.elapsed());

        client
            .send_sketch_data(sketches0, sketches1, &mut channel_server0, &mut channel_server1)
            .map_err(|e| e.to_string())?;

        println!("Client sending sketch data took {:.2?}", sketch_start.elapsed());
    }

    let total_time = start_time.elapsed();

    // Calculate communication metrics
    let (bytes_sent_0, _) = channel_server0.get_communication_stats();
    let (bytes_sent_1, _) = channel_server1.get_communication_stats();
    let total_bytes_sent = bytes_sent_0 + bytes_sent_1;

    println!("Client shares sent successfully");

    println!("\n=== Client Performance Summary ===");
    println!("📊 Share generation time: {:.2?}", share_time);
    println!("📊 Share transmission time: {:.2?}", send_time);
    println!("📊 Total client time: {:.2?}", total_time);
    println!("📡 Bytes sent to server 0: {} bytes", bytes_sent_0);
    println!("📡 Bytes sent to server 1: {} bytes", bytes_sent_1);
    println!(
        "📡 Total bytes sent: {} bytes ({:.2} KB)",
        total_bytes_sent,
        total_bytes_sent as f64 / 1024.0
    );

    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
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
