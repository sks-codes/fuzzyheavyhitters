use clap::Parser;
use mosaic::{
    channel::{listen_to, setup_parallel_channels},
    configs::cli_config::CliConfig,
    fuzzy_match::protocol::MosaicProtocol,
    randomness::prg::PRG,
};
use std::fs;

/// Load query points from JSON file
fn load_query_points(file_path: &str) -> Result<Vec<Vec<u128>>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read query file {}: {}", file_path, e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse query file {}: {}", file_path, e))
}

/// Run server for both known and unknown dictionary cases
fn run_server(config_path: &str, is_server1: bool, num_threads: usize) -> Result<(), String> {
    let cli_config = CliConfig::from_file(config_path)?;
    let server_id = if is_server1 { 1 } else { 0 };
    let is_known_dictionary = cli_config.protocol.dictionary_type == "Known";

    println!(
        "Server {} starting {} dictionary protocol with {} parallel threads...",
        server_id,
        if is_known_dictionary {
            "known"
        } else {
            "unknown"
        },
        num_threads
    );

    let server_addr = if is_server1 {
        cli_config.network.server1_addr.clone()
    } else {
        cli_config.network.server0_addr.clone()
    };

    // Create protocol configuration
    let protocol_parameters = cli_config.protocol.clone();
    let mut protocol = MosaicProtocol::new(
        protocol_parameters.clone(), 
        is_server1,
        protocol_parameters.clone().enable_sketch,
        protocol_parameters.clone().num_clients,
        protocol_parameters.clone().match_threshold,
    );

    // Listen for client connection
    let client_to_server_port = if is_server1 {
        cli_config.network.client_to_server1_port
    } else {
        cli_config.network.client_to_server0_port
    };

    let mut client_channel = listen_to(server_addr.clone(), client_to_server_port)
        .expect("Failed to listen for client connection");

    println!("Server {}: Receiving shares from client...", server_id);
    let mut shares = protocol
        .receive_client_shares(&mut client_channel)
        .map_err(|e| format!("Failed to receive client shares: {}", e))?;
    println!(
        "Server {}: Received {} shares from client",
        server_id,
        shares.len()
    );
    let mut sketch_data = None;
    if protocol_parameters.enable_sketch {
        println!("Server {}: Receiving sketch data from client...", server_id);
        let data = protocol
            .receive_client_sketch_data(&mut client_channel)
            .map_err(|e| format!("Failed to receive client sketch data: {}", e))?;
        println!(
            "Server {}: Received sketch data for {} shares",
            server_id,
            data.len()
        );
        sketch_data = Some(data);
    }

    // Set up parallel server-to-server communication channels
    let server0_addr = &cli_config.network.server0_addr;
    let server0_to_server1_port = cli_config.network.server0_to_server1_port;

    let mut other_server_channels = {
        println!(
            "Server {}: Setting up {} inter-server channels...",
            server_id, num_threads
        );

        let channels = if is_server1 {
            // Server 1 connects to Server 0's channels
            setup_parallel_channels(
                true, // is_connector
                num_threads,
                server0_addr,
                server0_to_server1_port,
            )?
        } else {
            // Server 0 listens for Server 1's connections
            setup_parallel_channels(
                false, // is_connector
                num_threads,
                server0_addr, // listen on all interfaces
                server0_to_server1_port,
            )?
        };

        println!(
            "Server {}: Successfully established {} inter-server channels",
            server_id,
            channels.len()
        );
        channels
    };

    println!("Server {}: Verifying client shares using sketches...", server_id);

    let start_sketch = std::time::Instant::now();

    if protocol_parameters.enable_sketch {
        let (malicious_flags, bad_count) = {
            let sketch_data = sketch_data
                .take()
                .ok_or_else(|| "Sketch data missing while sketching is enabled".to_string())?;
            let prg_seed = [0u8; 16]; // Change later, need to exchange seed between servers
            let mut prg = PRG::new(Some(&prg_seed), 0);
            let flags = protocol
                .verify_client_shared_ranges(
                    &shares,
                    &sketch_data,
                    &mut prg,
                    &mut other_server_channels,
                )
                .map_err(|e| format!("Failed to verify client shares: {}", e))?;
            let bad_count = flags.iter().filter(|&&flag| flag).count();
            (flags, bad_count)
        };
        let total = malicious_flags.len();
        if bad_count > 0 {
            println!(
                "Server {}: Removing {} of {} shares flagged by sketch verification",
                server_id, bad_count, total
            );
        }
        shares = shares
            .into_iter()
            .zip(malicious_flags.iter())
            .filter_map(|(share, flag)| if *flag { None } else { Some(share) })
            .collect();
        if bad_count > 0 {
            protocol = MosaicProtocol::new(
                protocol_parameters.clone(),
                is_server1,
                protocol_parameters.enable_sketch,
                shares.len(),
                protocol_parameters.match_threshold,
            );
        }
    }

    println!(
        "Server {}: Sketch verification completed in {:.2?}",
        server_id,
        start_sketch.elapsed()
    );

    // Determine number of parallel channels (use same as num_threads or system parallelism)
    let num_dealer_channels = num_threads;

    // Set up multiple dealer channels
    println!(
        "Server {}: Setting up {} dealer channels...",
        server_id, num_dealer_channels
    );
    let dealer_to_server_port = if is_server1 {
        cli_config.network.dealer_to_server1_port
    } else {
        cli_config.network.dealer_to_server0_port
    };

    let mut signal_dealer_channels = setup_parallel_channels(
        false,
        num_dealer_channels,
        &server_addr,
        dealer_to_server_port,
    )?;
    let mut check_dealer_channels = setup_parallel_channels(
        false, // is_connector (server listens for dealer connections)
        num_dealer_channels,
        &server_addr, // listen on all interfaces
        dealer_to_server_port + num_dealer_channels as u16,
    )?;
    let mut threshold_dealer_channels = setup_parallel_channels(
        false, // is_connector (server listens for dealer connections)
        num_dealer_channels,
        &server_addr, // listen on all interfaces
        dealer_to_server_port + 2 * num_dealer_channels as u16, // Next set of ports for threshold
    )?;

    println!("Server {}: Successfully established {} check dealer channels and {} threshold dealer channels",
             server_id, check_dealer_channels.len(), threshold_dealer_channels.len());

    println!("Server {}: Running protocol...", server_id);

    let start_time = std::time::Instant::now();

    let protocol_time = if is_known_dictionary {
        // Load query points for known dictionary
        println!(
            "Server {}: Loading query points from {}",
            server_id, cli_config.query_file
        );
        let query_points = load_query_points(&cli_config.query_file)?;

        // Run the protocol for known dictionary
        println!(
            "Server {}: Using {} dealer channels and {} server channels for parallel processing",
            server_id,
            signal_dealer_channels.len(),
            other_server_channels.len()
        );

        let results = protocol.run_server_known_dictionary_parallel(
            &shares,
            &query_points,
            &mut signal_dealer_channels,
            &mut check_dealer_channels,
            &mut threshold_dealer_channels,
            &mut other_server_channels,
        )?;
        println!("Server {}: Protocol execution completed", server_id);
        println!(
            "Found {} heavy hitters.",
            results.len(),
        );

        start_time.elapsed()
    } else {
        println!(
            "Server {}: Running unknown dictionary protocol...",
            server_id
        );

        // Run the protocol for unknown dictionary
        let heavy_hitters = protocol.run_server_unknown_dictionary_parallel(
            &shares,
            &mut signal_dealer_channels,
            &mut check_dealer_channels,
            &mut threshold_dealer_channels,
            &mut other_server_channels,
        )?;
        println!("Server {}: Protocol execution completed", server_id);
        println!(
            "Found {} heavy hitters.",
            heavy_hitters.len(),
        );

        start_time.elapsed()
    };

    // Calculate communication metrics from all channels
    let mut total_other_server_bytes_sent = 0;
    let mut total_other_server_bytes_received = 0;
    let mut total_dealer_bytes_sent = 0;
    let mut total_dealer_bytes_received = 0;

    for channel in &other_server_channels {
        let (sent, received) = channel.get_communication_stats();
        total_other_server_bytes_sent += sent;
        total_other_server_bytes_received += received;
    }

    for channel in &signal_dealer_channels {
        let (sent, received) = channel.get_communication_stats();
        total_dealer_bytes_sent += sent;
        total_dealer_bytes_received += received;
    }
    for channel in &check_dealer_channels {
        let (sent, received) = channel.get_communication_stats();
        total_dealer_bytes_sent += sent;
        total_dealer_bytes_received += received;
    }
    for channel in &threshold_dealer_channels {
        let (sent, received) = channel.get_communication_stats();
        total_dealer_bytes_sent += sent;
        total_dealer_bytes_received += received;
    }

    println!("\n=== Server {} Performance Summary ===", server_id);
    println!("📊 Protocol execution time: {:.2?}", protocol_time);
    println!(
        "📡 Communication with other server ({} channels):",
        other_server_channels.len()
    );
    println!(
        "   Bytes sent: {} bytes ({:.2} KB)",
        total_other_server_bytes_sent,
        total_other_server_bytes_sent as f64 / 1024.0
    );
    println!(
        "   Bytes received: {} bytes ({:.2} KB)",
        total_other_server_bytes_received,
        total_other_server_bytes_received as f64 / 1024.0
    );
    println!(
        "📡 Communication with dealer ({} channels):",
        signal_dealer_channels.len()
    );
    println!(
        "   Bytes sent: {} bytes ({:.2} KB)",
        total_dealer_bytes_sent,
        total_dealer_bytes_sent as f64 / 1024.0
    );
    println!(
        "   Bytes received: {} bytes ({:.2} KB)",
        total_dealer_bytes_received,
        total_dealer_bytes_received as f64 / 1024.0
    );
    println!(
        "📡 Total communication: {} bytes ({:.2} KB)",
        total_other_server_bytes_sent
            + total_other_server_bytes_received
            + total_dealer_bytes_sent
            + total_dealer_bytes_received,
        (total_other_server_bytes_sent
            + total_other_server_bytes_received
            + total_dealer_bytes_sent
            + total_dealer_bytes_received) as f64
            / 1024.0
    );

    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    side: u8,
    #[arg(short, long)]
    config: String,
    #[arg(short, long, default_value_t = 1)]
    threads: usize,
}

fn main() {
    let args = Args::parse();
    let side = args.side;
    let config_path = args.config;
    let num_threads = args.threads;

    let result = if side == 0 {
        run_server(&config_path, false, num_threads)
    } else if side == 1 {
        run_server(&config_path, true, num_threads)
    } else {
        Err("Side must be 0 or 1".to_string())
    };

    if let Err(e) = result {
        eprintln!("Error running server: {}", e);
    }
}
