//! CLI for Fuzzy Heavy Hitters Protocol
//! 
//! This CLI allows running the fuzzy heavy hitters protocol with two servers
//! testing on cluster centers from synthetic data.

use counttree::{
    protocol::{FuzzyHeavyHittersProtocol, generate_fss_keys_for_threshold},
    cli_config::CliConfig,
};
use clap::{App, Arg, SubCommand};
use std::process;
use std::thread;
use std::os::unix::net::{UnixStream, UnixListener};
use std::fs;
use serde_json;

/// Data structure for synthetic data
#[derive(serde::Deserialize)]
struct Cluster {
    center: Vec<u128>,
    points: Vec<Vec<u128>>,
}

/// Load cluster data from JSON file
fn load_clusters(file_path: &str) -> Result<Vec<Cluster>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read data file {}: {}", file_path, e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse data file {}: {}", file_path, e))
}

/// Load query points from JSON file
fn load_query_points(file_path: &str) -> Result<Vec<Vec<u128>>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read query file {}: {}", file_path, e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse query file {}: {}", file_path, e))
}

/// Run the protocol with both servers in the same process (for testing)
fn run_local_protocol(config_path: &str) -> Result<(), String> {
    println!("Loading configuration from {}", config_path);
    let cli_config = CliConfig::from_file(config_path)?;
    
    println!("Loading synthetic data from {}", cli_config.data_file);
    let clusters = load_clusters(&cli_config.data_file)?;
    
    println!("Loading query points from {}", cli_config.query_file);
    let query_points = load_query_points(&cli_config.query_file)?;
    
    // Extract client points from clusters
    let mut client_points = Vec::new();
    for cluster in &clusters {
        for point in &cluster.points {
            client_points.push(point.clone());
        }
    }
    
    println!("Loaded {} client points from {} clusters", client_points.len(), clusters.len());
    println!("Testing {} query points", query_points.len());
    
    // Create protocol configurations for both servers
    let protocol_config_server0 = cli_config.to_protocol_config(false)?;
    let protocol_config_server1 = cli_config.to_protocol_config(true)?;
    
    let protocol_server0 = FuzzyHeavyHittersProtocol::new(protocol_config_server0);
    let protocol_server1 = FuzzyHeavyHittersProtocol::new(protocol_config_server1);
    
    println!("Generating client shares...");
    let (shares_server0, shares_server1) = protocol_server0.generate_client_shares(&client_points)?;
    
    // Generate FSS keys if using IntervalFSS
    let (fss_keys_server0, fss_keys_server1) = if cli_config.protocol.threshold_method == "IntervalFSS" {
        let fss_config = cli_config.protocol.interval_fss.as_ref()
            .ok_or("IntervalFSS configuration required")?;
        
        let random_values = (
            fss_config.random_value_server0,
            fss_config.random_value_server1,
        );
        
        println!("Generating FSS keys for IntervalFSS...");
        let (keys0, keys1) = generate_fss_keys_for_threshold(
            cli_config.protocol.threshold,
            random_values,
            cli_config.protocol.output_bit_length + 4,
            query_points.len(),
        )?;
        
        (Some(keys0), Some(keys1))
    } else {
        (None, None)
    };
    
    println!("Starting protocol execution...");
    println!("Using {} threshold method", cli_config.protocol.threshold_method);
    println!("Threshold: {}, Delta: {}", cli_config.protocol.threshold, cli_config.protocol.delta);
    println!();
    
    // Create Unix socket pair for communication
    let (stream1, stream2) = UnixStream::pair()
        .map_err(|e| format!("Failed to create Unix socket pair: {}", e))?;
    
    // Run server 1 in a separate thread
    let shares_server1_clone = shares_server1.clone();
    let query_points_clone = query_points.clone();
    let fss_keys_server1_clone = fss_keys_server1.clone();
    
    let server1_handle = thread::spawn(move || {
        protocol_server1.run_server1(
            &shares_server1_clone,
            &query_points_clone,
            fss_keys_server1_clone.as_ref().map(|keys| keys.as_slice()),
            stream1,
        )
    });
    
    // Run server 0 in main thread
    let result_server0 = protocol_server0.run_server0(
        &shares_server0,
        &query_points,
        fss_keys_server0.as_ref().map(|keys| keys.as_slice()),
        stream2,
    )?;
    
    // Wait for server 1 to complete
    let result_server1 = server1_handle.join()
        .map_err(|e| format!("Server 1 thread failed: {:?}", e))??;
    
    // Combine results (XOR the threshold bits)
    println!("=== Protocol Results ===");
    let mut total_heavy_hitters = 0;
    
    for (i, query_point) in query_points.iter().enumerate() {
        let final_result = result_server0.threshold_exceeded[i] ^ result_server1.threshold_exceeded[i];
        
        if final_result {
            total_heavy_hitters += 1;
        }
        
        let status = if final_result { "HEAVY HITTER" } else { "not heavy hitter" };
        
        if cli_config.output.verbose {
            println!("Query {}: {:?} -> {} (server0: {}, server1: {})", 
                i + 1, query_point, status, 
                result_server0.threshold_exceeded[i], 
                result_server1.threshold_exceeded[i]);
        } else {
            println!("Query {}: {:?} -> {}", i + 1, query_point, status);
        }
    }
    
    println!();
    println!("Summary: {}/{} query points are heavy hitters", total_heavy_hitters, query_points.len());
    
    // Save results if output file specified
    if let Some(output_file) = &cli_config.output.output_file {
        let results: Vec<_> = query_points.iter().enumerate().map(|(i, query)| {
            let final_result = result_server0.threshold_exceeded[i] ^ result_server1.threshold_exceeded[i];
            serde_json::json!({
                "query_point": query,
                "is_heavy_hitter": final_result,
                "server0_bit": result_server0.threshold_exceeded[i],
                "server1_bit": result_server1.threshold_exceeded[i]
            })
        }).collect();
        
        let output = serde_json::json!({
            "protocol_config": cli_config.protocol,
            "total_heavy_hitters": total_heavy_hitters,
            "total_queries": query_points.len(),
            "results": results
        });
        
        fs::write(output_file, serde_json::to_string_pretty(&output).unwrap())
            .map_err(|e| format!("Failed to write output file: {}", e))?;
        
        println!("Results saved to {}", output_file);
    }
    
    Ok(())
}

/// Run as server 0
fn run_server0(config_path: &str, listen_addr: &str) -> Result<(), String> {
    println!("Starting server 0, listening on {}", listen_addr);
    let cli_config = CliConfig::from_file(config_path)?;
    
    // Load data
    let clusters = load_clusters(&cli_config.data_file)?;
    let query_points = load_query_points(&cli_config.query_file)?;
    
    let mut client_points = Vec::new();
    for cluster in &clusters {
        for point in &cluster.points {
            client_points.push(point.clone());
        }
    }
    
    let protocol_config = cli_config.to_protocol_config(false)?;
    let protocol = FuzzyHeavyHittersProtocol::new(protocol_config);
    
    let (shares_server0, _) = protocol.generate_client_shares(&client_points)?;
    
    // Generate FSS keys if needed
    let fss_keys = if cli_config.protocol.threshold_method == "IntervalFSS" {
        let fss_config = cli_config.protocol.interval_fss.as_ref()
            .ok_or("IntervalFSS configuration required")?;
        
        let random_values = (
            fss_config.random_value_server0,
            fss_config.random_value_server1,
        );
        
        let (keys0, _) = generate_fss_keys_for_threshold(
            cli_config.protocol.threshold,
            random_values,
            cli_config.protocol.output_bit_length + 4,
            query_points.len(),
        )?;
        
        Some(keys0)
    } else {
        None
    };
    
    // Listen for connection from server 1
    let listener = UnixListener::bind(listen_addr)
        .map_err(|e| format!("Failed to bind to {}: {}", listen_addr, e))?;
    
    println!("Waiting for server 1 to connect...");
    let (stream, _) = listener.accept()
        .map_err(|e| format!("Failed to accept connection: {}", e))?;
    
    println!("Server 1 connected, starting protocol...");
    let result = protocol.run_server0(
        &shares_server0,
        &query_points,
        fss_keys.as_ref().map(|keys| keys.as_slice()),
        stream,
    )?;
    
    println!("Protocol completed on server 0");
    println!("Server 0 results: {:?}", result.threshold_exceeded);
    
    Ok(())
}

/// Run as server 1
fn run_server1(config_path: &str, connect_addr: &str) -> Result<(), String> {
    println!("Starting server 1, connecting to {}", connect_addr);
    let cli_config = CliConfig::from_file(config_path)?;
    
    // Load data
    let clusters = load_clusters(&cli_config.data_file)?;
    let query_points = load_query_points(&cli_config.query_file)?;
    
    let mut client_points = Vec::new();
    for cluster in &clusters {
        for point in &cluster.points {
            client_points.push(point.clone());
        }
    }
    
    let protocol_config = cli_config.to_protocol_config(true)?;
    let protocol = FuzzyHeavyHittersProtocol::new(protocol_config);
    
    let (_, shares_server1) = protocol.generate_client_shares(&client_points)?;
    
    // Generate FSS keys if needed
    let fss_keys = if cli_config.protocol.threshold_method == "IntervalFSS" {
        let fss_config = cli_config.protocol.interval_fss.as_ref()
            .ok_or("IntervalFSS configuration required")?;
        
        let random_values = (
            fss_config.random_value_server0,
            fss_config.random_value_server1,
        );
        
        let (_, keys1) = generate_fss_keys_for_threshold(
            cli_config.protocol.threshold,
            random_values,
            cli_config.protocol.output_bit_length + 4,
            query_points.len(),
        )?;
        
        Some(keys1)
    } else {
        None
    };
    
    // Connect to server 0
    println!("Connecting to server 0...");
    let stream = UnixStream::connect(connect_addr)
        .map_err(|e| format!("Failed to connect to {}: {}", connect_addr, e))?;
    
    println!("Connected to server 0, starting protocol...");
    let result = protocol.run_server1(
        &shares_server1,
        &query_points,
        fss_keys.as_ref().map(|keys| keys.as_slice()),
        stream,
    )?;
    
    println!("Protocol completed on server 1");
    println!("Server 1 results: {:?}", result.threshold_exceeded);
    
    Ok(())
}

/// Generate a sample configuration file
fn generate_config(output_path: &str) -> Result<(), String> {
    let sample_config = CliConfig {
        data_file: "data/synthetic/clusters.json".to_string(),
        query_file: "data/synthetic/server_query_points.json".to_string(),
        protocol: counttree::cli_config::ProtocolParameters {
            delta: 5,
            threshold: 3,
            input_bit_length: 8,
            output_bit_length: 16,
            dimensions: 2,
            share_method: "OKVS".to_string(), // Can also be "IntervalFSS"
            threshold_method: "GarbledCircuits".to_string(), // Can also be "IntervalFSS"
            okvs: Some(counttree::cli_config::OkvsConfig {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            }),
            interval_fss: Some(counttree::cli_config::IntervalFssConfig {
                random_value_server0: 12345,
                random_value_server1: 67890,
            }),
        },
        network: counttree::cli_config::NetworkConfig {
            server0_addr: "127.0.0.1".to_string(),
            server1_addr: "127.0.0.1".to_string(),
            server0_port: 8000,
            server1_port: 8001,
        },
        output: counttree::cli_config::OutputConfig {
            verbose: true,
            show_intermediate: false,
            output_file: Some("results.json".to_string()),
        },
    };
    
    sample_config.to_file(output_path)?;
    println!("Sample configuration written to {}", output_path);
    Ok(())
}

fn main() {
    let matches = App::new("Fuzzy Heavy Hitters CLI")
        .version("1.0")
        .about("CLI for running the fuzzy heavy hitters protocol")
        .subcommand(
            SubCommand::with_name("run-local")
                .about("Run both servers locally for testing")
                .arg(
                    Arg::with_name("config")
                        .short("c")
                        .long("config")
                        .value_name("FILE")
                        .help("Configuration file path")
                        .required(true)
                )
        )
        .subcommand(
            SubCommand::with_name("server0")
                .about("Run as server 0")
                .arg(
                    Arg::with_name("config")
                        .short("c")
                        .long("config")
                        .value_name("FILE")
                        .help("Configuration file path")
                        .required(true)
                )
                .arg(
                    Arg::with_name("listen")
                        .short("l")
                        .long("listen")
                        .value_name("ADDR")
                        .help("Address to listen on (Unix socket path)")
                        .default_value("/tmp/fhh_server0.sock")
                )
        )
        .subcommand(
            SubCommand::with_name("server1")
                .about("Run as server 1")
                .arg(
                    Arg::with_name("config")
                        .short("c")
                        .long("config")
                        .value_name("FILE")
                        .help("Configuration file path")
                        .required(true)
                )
                .arg(
                    Arg::with_name("connect")
                        .short("a")
                        .long("connect")
                        .value_name("ADDR")
                        .help("Address to connect to (Unix socket path)")
                        .default_value("/tmp/fhh_server0.sock")
                )
        )
        .subcommand(
            SubCommand::with_name("generate-config")
                .about("Generate a sample configuration file")
                .arg(
                    Arg::with_name("output")
                        .short("o")
                        .long("output")
                        .value_name("FILE")
                        .help("Output configuration file path")
                        .default_value("fhh_config.json")
                )
        )
        .get_matches();

    let result = match matches.subcommand() {
        ("run-local", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            run_local_protocol(config_path)
        },
        ("server0", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            let listen_addr = sub_matches.value_of("listen").unwrap();
            run_server0(config_path, listen_addr)
        },
        ("server1", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            let connect_addr = sub_matches.value_of("connect").unwrap();
            run_server1(config_path, connect_addr)
        },
        ("generate-config", Some(sub_matches)) => {
            let output_path = sub_matches.value_of("output").unwrap();
            generate_config(output_path)
        },
        _ => {
            eprintln!("No subcommand specified. Use --help for usage information.");
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
