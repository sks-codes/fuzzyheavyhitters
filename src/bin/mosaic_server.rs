use mosaic::{
    configs::cli_config::CliConfig,
    fuzzy_match::protocol::MosaicProtocol,
    channel::{setup_parallel_channels, listen_to},
};
use std::fs;
use clap::{App, Arg};

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
    
    println!("Server {} starting {} dictionary protocol with {} parallel threads...", 
             server_id, 
             if is_known_dictionary { "known" } else { "unknown" },
             num_threads);

    let server_addr = if is_server1 {
        cli_config.network.server1_addr.clone()
    } else {
        cli_config.network.server0_addr.clone()
    };
    
    // Create protocol configuration
    let protocol_config = cli_config.to_protocol_config(is_server1)?;
    let protocol = MosaicProtocol::new(protocol_config, is_server1);
    
    // Listen for client connection
    let client_to_server_port = if is_server1 { 
        cli_config.network.client_to_server1_port 
    } else { 
        cli_config.network.client_to_server0_port 
    };

    let mut client_channel = listen_to(
        server_addr.clone(),
        client_to_server_port,
    ).expect("Failed to listen for client connection");
    
    println!("Server {}: Receiving shares from client...", server_id);
    let shares = protocol.receive_client_shares(&mut client_channel)
        .map_err(|e| format!("Failed to receive client shares: {}", e))?;
    println!("Server {}: Received {} shares from client", server_id, shares.len());

    // Determine number of parallel channels (use same as num_threads or system parallelism)
    let num_dealer_channels = num_threads;

    // Set up multiple dealer channels
    println!("Server {}: Setting up {} dealer channels...", server_id, num_dealer_channels);
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

    // Set up parallel server-to-server communication channels
    let server0_addr = &cli_config.network.server0_addr;
    let server0_to_server1_port = cli_config.network.server0_to_server1_port;
    
    let mut other_server_channels = {
        println!("Server {}: Setting up {} inter-server channels...", server_id, num_threads);
        
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
        
        println!("Server {}: Successfully established {} inter-server channels", server_id, channels.len());
        channels
    };
    let start_time = std::time::Instant::now();

    let protocol_time = if is_known_dictionary {
        // Load query points for known dictionary
        println!("Server {}: Loading query points from {}", server_id, cli_config.query_file);
        let query_points = load_query_points(&cli_config.query_file)?;
        
        // Run the protocol for known dictionary
        println!("Server {}: Using {} dealer channels and {} server channels for parallel processing", 
                 server_id, signal_dealer_channels.len(), other_server_channels.len());
        
        let results = protocol.run_server_known_dictionary_parallel(
            &shares,
            &query_points,
            &mut signal_dealer_channels,
            &mut check_dealer_channels,
            &mut threshold_dealer_channels,
            &mut other_server_channels,
        )?;
        println!("Server {}: Protocol execution completed", server_id);
        println!("Results: {:?}", results);

        start_time.elapsed()
    } else {
        println!("Server {}: Running unknown dictionary protocol...", server_id);
        
        // Run the protocol for unknown dictionary
        let heavy_hitters = protocol.run_server_unknown_dictionary_parallel(
            &shares,
            &mut signal_dealer_channels,
            &mut check_dealer_channels,
            &mut threshold_dealer_channels,
            &mut other_server_channels,
        )?;
        println!("Server {}: Protocol execution completed", server_id);
        println!("Found {} heavy hitters: {:?}", heavy_hitters.len(), heavy_hitters);
        
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
    println!("📡 Communication with other server ({} channels):", other_server_channels.len());
    println!("   Bytes sent: {} bytes ({:.2} KB)", total_other_server_bytes_sent, total_other_server_bytes_sent as f64 / 1024.0);
    println!("   Bytes received: {} bytes ({:.2} KB)", total_other_server_bytes_received, total_other_server_bytes_received as f64 / 1024.0);
    println!("📡 Communication with dealer ({} channels):", signal_dealer_channels.len());
    println!("   Bytes sent: {} bytes ({:.2} KB)", total_dealer_bytes_sent, total_dealer_bytes_sent as f64 / 1024.0);
    println!("   Bytes received: {} bytes ({:.2} KB)", total_dealer_bytes_received, total_dealer_bytes_received as f64 / 1024.0);
    println!("📡 Total communication: {} bytes ({:.2} KB)", 
                total_other_server_bytes_sent + total_other_server_bytes_received + total_dealer_bytes_sent + total_dealer_bytes_received,
                (total_other_server_bytes_sent + total_other_server_bytes_received + total_dealer_bytes_sent + total_dealer_bytes_received) as f64 / 1024.0);
        
    
    Ok(())
}


fn main() {
    let matches = App::new("Server CLI for Mosaic")
    .version("1.0")
    .about("CLI for running the fuzzy heavy hitters protocol in distributed or local mode")
        .arg(
            Arg::with_name("side")
                .short("s")
                .long("side")
                .value_name("SIDE")
                .help("Server side: 0 or 1")
                .required(true)
        )
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Configuration file path")
                .required(true)
        )
        .arg(
            Arg::with_name("threads")
                .short("t")
                .long("threads")
                .value_name("NUMBER")
                .help("Number of parallel threads/channels to use")
                .default_value("1")
        )
    .get_matches();

    let side = matches.value_of("side").unwrap()
        .parse::<u8>()
        .map_err(|_| "Invalid side, must be 0 or 1").expect("Failed to parse side");
    let config_path = matches.value_of("config").unwrap();
    let num_threads = matches.value_of("threads").unwrap()
        .parse::<usize>()
        .map_err(|_| "Invalid number of threads").expect("Failed to parse number of threads");


   let result = if side == 0 {
        run_server(config_path, false, num_threads)
    } else if side == 1 {
        run_server(config_path, true, num_threads)
    } else {
        Err("Side must be 0 or 1".to_string())
    };

    if let Err(e) = result {
        eprintln!("Error running server: {}", e);
    }
}