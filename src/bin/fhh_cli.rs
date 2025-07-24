//! CLI for Fuzzy Heavy Hitters Protocol
//! 
//! This CLI allows running the complete fuzzy heavy hitters protocol locally
//! with both servers in the same process for testing purposes.

use counttree::{
    protocol::{FuzzyHeavyHittersProtocol, generate_fss_keys_for_threshold},
    fuzzy_match::threshold_phase::ThresholdData,
    cli_config::CliConfig,
};
use clap::{App, Arg, SubCommand};
use std::process;
use std::thread;
use std::os::unix::net::UnixStream;
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
    
    // Create initial protocol configurations for both servers
    let protocol_config_server0 = cli_config.to_protocol_config(false)?;
    let protocol_config_server1 = cli_config.to_protocol_config(true)?;

    // Generate FSS keys and create threshold data if using IntervalFSS
    let (threshold_data_list_server0, threshold_data_list_server1) = if cli_config.protocol.threshold_method == "IntervalFSS" {
        println!("Generating FSS keys for IntervalFSS...");
        let (keys0, keys1, random_pairs) = generate_fss_keys_for_threshold(
            cli_config.protocol.threshold,
            cli_config.protocol.check_output_bit_length,
            query_points.len(),
        )?;
        
        println!("Generated {} random pairs for FSS keys", random_pairs.len());
        
        // Create threshold data for each query using corresponding random pairs
        let mut threshold_data_list_0 = Vec::new();
        let mut threshold_data_list_1 = Vec::new();
        
        for ((fss_key_0, fss_key_1), (r0, r1)) in keys0.iter().zip(keys1.iter()).zip(random_pairs.iter()) {
            threshold_data_list_0.push(ThresholdData::IntervalFSS {
                fss_key: fss_key_0.clone(),
                random_value: *r0,
            });
            threshold_data_list_1.push(ThresholdData::IntervalFSS {
                fss_key: fss_key_1.clone(),
                random_value: *r1,
            });
        }
        
        (threshold_data_list_0, threshold_data_list_1)
    } else {
        let threshold_data = ThresholdData::GarbledCircuits;
        let threshold_data_list_0 = vec![threshold_data.clone(); query_points.len()];
        let threshold_data_list_1 = vec![threshold_data; query_points.len()];
        (threshold_data_list_0, threshold_data_list_1)
    };

    let protocol_server0 = FuzzyHeavyHittersProtocol::new(protocol_config_server0);
    let protocol_server1 = FuzzyHeavyHittersProtocol::new(protocol_config_server1);

    println!("Generating client shares...");
    let (shares_server0, shares_server1) = protocol_server0.generate_client_shares(&client_points)?;
    
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
    let threshold_data_list_server1_clone = threshold_data_list_server1.clone();
    
    let server1_handle = thread::spawn(move || {
        protocol_server1.run_server1(
            &shares_server1_clone,
            &query_points_clone,
            &threshold_data_list_server1_clone,
            stream1,
        )
    });
    
    // Run server 0 in main thread
    let final_results = protocol_server0.run_server0(
        &shares_server0,
        &query_points,
        &threshold_data_list_server0,
        stream2,
    )?;
    
    // Wait for server 1 to complete (both servers should return the same final results)
    let _result_server1 = server1_handle.join()
        .map_err(|e| format!("Server 1 thread failed: {:?}", e))??;
    
    // Display results (final_results already contains the XOR of both servers' bits)
    println!("=== Protocol Results ===");
    let mut total_heavy_hitters = 0;
    
    for (i, query_point) in query_points.iter().enumerate() {
        let final_result = final_results[i];
        
        if final_result {
            total_heavy_hitters += 1;
        }
        
        let status = if final_result { "HEAVY HITTER" } else { "not heavy hitter" };
        
        if cli_config.output.verbose {
            println!("Query {}: {:?} -> {}", 
                i + 1, query_point, status);
        } else {
            println!("Query {}: {:?} -> {}", i + 1, query_point, status);
        }
    }
    
    println!();
    println!("Summary: {}/{} query points are heavy hitters", total_heavy_hitters, query_points.len());
    
    // Save results if output file specified
    if let Some(output_file) = &cli_config.output.output_file {
        let results: Vec<_> = query_points.iter().enumerate().map(|(i, query)| {
            let final_result = final_results[i];
            serde_json::json!({
                "query_point": query,
                "is_heavy_hitter": final_result,
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
            check_output_bit_length: 20, // output_bit_length + 4 for aggregation
            dimensions: 2,
            share_method: "OKVS".to_string(), // Can also be "IntervalFSS"
            threshold_method: "GarbledCircuits".to_string(), // Can also be "IntervalFSS"
            okvs: Some(counttree::cli_config::OkvsConfig {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
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
        .about("CLI for running the fuzzy heavy hitters protocol locally")
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
        ("generate-config", Some(sub_matches)) => {
            let output_path = sub_matches.value_of("output").unwrap();
            generate_config(output_path)
        },
        _ => {
            eprintln!("No subcommand specified. Use --help for usage information.");
            eprintln!("Available commands: run-local, generate-config");
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
