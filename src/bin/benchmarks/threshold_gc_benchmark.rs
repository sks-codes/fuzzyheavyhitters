use mosaic::{
    channel::{connect_to, listen_to},
    configs::property_test_config::BenchmarkConfig,
    fuzzy_match::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdMethod},
    data_structures::modint::ModInt,
};
use scuttlebutt::{AesRng, AbstractChannel};
use std::time::Instant;
use rand::Rng;
use clap::Parser;

fn generate_test_inputs(num_inputs: usize, modulus: u128) -> Vec<ModInt> {
    let mut rng = rand::rng();
    (0..num_inputs)
        .map(|_| {
            ModInt::new(rng.random::<u128>(), modulus)
        })
        .collect()
}

fn run_server_benchmark(config_path: &str, server: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config = BenchmarkConfig::from_file(config_path)?;
    if server {
        println!("Running as server 1");
    } else {
        println!("Running as server 0");
    }

    // Create channels
    let mut other_server_channel = if server {
        connect_to(config.server0_addr.clone(), config.server0_to_server1_port.parse::<u16>().unwrap())?
    } else {
        listen_to(config.server0_addr.clone(), config.server0_to_server1_port.parse::<u16>().unwrap())?
    };

    let threshold_config = ThresholdConfig {
        h3: config.h3,
        is_garbler_side: server,
        method: ThresholdMethod::GC,
    };

    let threshold_phase = ThresholdPhase::new(threshold_config);

    // Generate test inputs - 1000 Vec<bool> with h2 bits each
    println!("Generating {} test inputs with bit length {}", config.num_clients, config.h3);
    let inputs = generate_test_inputs(config.num_clients, 1u128 << config.h3 as u128);

    println!("Starting server benchmark...");
    if server {
        other_server_channel.write_bytes(&[1u8]).unwrap();
        other_server_channel.flush().unwrap();
    } else {
        let mut ack = [0u8; 1];
        other_server_channel.flush().unwrap();
        other_server_channel.read_bytes(&mut ack).unwrap();
    }

    let start_time = Instant::now();
    let mut rng = AesRng::new();
    let threshold = ModInt::new(config.threshold, 1u128 << config.h3);
    let _results = threshold_phase.compare_with_threshold_gc(
        &inputs,
        threshold,
        &mut other_server_channel,
        &mut rng,
    ).map_err(|e| format!("ThresholdPhase error: {:?}", e))?;
    
    let elapsed = start_time.elapsed();
    
    // Print results
    println!("\n=== Server Benchmark Results ===");
    println!("Server time: {:?}", elapsed);
    let (sent, received) = other_server_channel.get_communication_stats();
    println!("Communication sent: {} bytes", sent);
    println!("Communication received: {} bytes", received);
    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    role: String,
    #[arg(short, long)]
    config: String,
}

fn main() {
    let args = Args::parse();
    let role = args.role;
    let config_path = &args.config;
    let result = match role.to_lowercase().as_str() {
        "server0" => run_server_benchmark(config_path, false),
        "server1" => run_server_benchmark(config_path, true),
        _ => {
            eprintln!("Invalid role '{}'. Must be 'server0' or 'server1'", role);
            std::process::exit(1);
        }
    };

    match result {
        Ok(()) => {
            println!("Benchmark completed successfully!");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Benchmark failed: {}", e);
            std::process::exit(1);
        }
    }
}
