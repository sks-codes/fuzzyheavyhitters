use clap::Parser;
use mosaic::{
    channel::setup_parallel_channels, configs::cli_config::CliConfig,
    fuzzy_match::dealer::FssDealer, util::get_distance_threshold,
};

/// Run as dealer - generates and distributes FSS keys to servers
fn run_dealer(config_path: &str, num_threads: usize) -> Result<(), String> {
    let start_time = std::time::Instant::now();
    println!("Starting FSS Dealer...");
    let cli_config = CliConfig::from_file(config_path)?;

    let distance_threshold = get_distance_threshold(
        cli_config.protocol.delta,
        &cli_config.protocol.distance_metric,
    );

    let dealer = FssDealer::new(
        distance_threshold,
        cli_config.protocol.match_threshold,
        cli_config.protocol.h2,
        cli_config.protocol.h3,
        cli_config.protocol.num_clients,
        cli_config.protocol.d,
    );

    // Determine number of parallel channels (use specified num_threads or system parallelism)
    let num_channels = num_threads;

    println!(
        "Setting up {} parallel dealer channels to each server...",
        num_channels
    );

    // Set up parallel channels to server 0
    let mut signal_channels_server0 = setup_parallel_channels(
        true,
        num_channels,
        &cli_config.network.server0_addr,
        cli_config.network.dealer_to_server0_port,
    )?;
    let mut check_channels_server0 = setup_parallel_channels(
        true, // is_connector (dealer connects to servers)
        num_channels,
        &cli_config.network.server0_addr,
        cli_config.network.dealer_to_server0_port + num_channels as u16,
    )?;
    let mut threshold_channels_server0 = setup_parallel_channels(
        true, // is_connector (dealer connects to servers)
        num_channels,
        &cli_config.network.server0_addr,
        cli_config.network.dealer_to_server0_port + 2 * num_channels as u16,
    )?;

    // Set up parallel channels to server 1
    let mut signal_channels_server1 = setup_parallel_channels(
        true,
        num_channels,
        &cli_config.network.server1_addr,
        cli_config.network.dealer_to_server1_port,
    )?;
    let mut check_channels_server1 = setup_parallel_channels(
        true, // is_connector (dealer connects to servers)
        num_channels,
        &cli_config.network.server1_addr,
        cli_config.network.dealer_to_server1_port + num_channels as u16,
    )?;
    let mut threshold_channels_server1 = setup_parallel_channels(
        true, // is_connector (dealer connects to servers)
        num_channels,
        &cli_config.network.server1_addr,
        cli_config.network.dealer_to_server1_port + 2 * num_channels as u16,
    )?;

    println!(
        "Connected to both servers with {} channels each",
        num_channels
    );

    let dealer_start = std::time::Instant::now();
    dealer
        .run_parallel(
            &mut signal_channels_server0,
            &mut signal_channels_server1,
            &mut check_channels_server0,
            &mut check_channels_server1,
            &mut threshold_channels_server0,
            &mut threshold_channels_server1,
        )
        .map_err(|e| format!("Failed to run parallel dealer protocol: {}", e))?;
    let dealer_time = dealer_start.elapsed();

    let total_time = start_time.elapsed();

    // Calculate communication metrics from all channels
    let mut total_bytes_sent_0 = 0;
    let mut total_bytes_received_0 = 0;
    let mut total_bytes_sent_1 = 0;
    let mut total_bytes_received_1 = 0;

    for channel in &signal_channels_server0 {
        let (sent, received) = channel.get_communication_stats();
        total_bytes_sent_0 += sent;
        total_bytes_received_0 += received;
    }

    for channel in &signal_channels_server1 {
        let (sent, received) = channel.get_communication_stats();
        total_bytes_sent_1 += sent;
        total_bytes_received_1 += received;
    }

    for channel in &check_channels_server0 {
        let (sent, received) = channel.get_communication_stats();
        total_bytes_sent_0 += sent;
        total_bytes_received_0 += received;
    }

    for channel in &check_channels_server1 {
        let (sent, received) = channel.get_communication_stats();
        total_bytes_sent_1 += sent;
        total_bytes_received_1 += received;
    }

    for channel in &threshold_channels_server0 {
        let (sent, received) = channel.get_communication_stats();
        total_bytes_sent_0 += sent;
        total_bytes_received_0 += received;
    }

    for channel in &threshold_channels_server1 {
        let (sent, received) = channel.get_communication_stats();
        total_bytes_sent_1 += sent;
        total_bytes_received_1 += received;
    }

    let total_bytes =
        total_bytes_sent_0 + total_bytes_received_0 + total_bytes_sent_1 + total_bytes_received_1;

    println!("\n=== Dealer Performance Summary ===");
    println!(
        "📊 FSS key generation and distribution time: {:.2?}",
        dealer_time
    );
    println!("📊 Total dealer time: {:.2?}", total_time);
    println!(
        "📡 Communication with server 0 ({} channels):",
        num_channels
    );
    println!(
        "   Bytes sent: {} bytes ({:.2} KB)",
        total_bytes_sent_0,
        total_bytes_sent_0 as f64 / 1024.0
    );
    println!(
        "   Bytes received: {} bytes ({:.2} KB)",
        total_bytes_received_0,
        total_bytes_received_0 as f64 / 1024.0
    );
    println!(
        "📡 Communication with server 1 ({} channels):",
        num_channels
    );
    println!(
        "   Bytes sent: {} bytes ({:.2} KB)",
        total_bytes_sent_1,
        total_bytes_sent_1 as f64 / 1024.0
    );
    println!(
        "   Bytes received: {} bytes ({:.2} KB)",
        total_bytes_received_1,
        total_bytes_received_1 as f64 / 1024.0
    );
    println!(
        "📡 Total communication: {} bytes ({:.2} KB)",
        total_bytes,
        total_bytes as f64 / 1024.0
    );

    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    config: String,
    #[arg(short, long)]
    threads: usize,
}

fn main() {
    let args = Args::parse();
    let config_path = &args.config;
    let num_threads = args.threads;

    let result = run_dealer(config_path, num_threads);

    if let Err(e) = result {
        eprintln!("Error running dealer: {}", e);
    }
}
