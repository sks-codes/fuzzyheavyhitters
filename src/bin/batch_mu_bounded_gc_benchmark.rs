use counttree::channel::CommTrackingChannel;
use counttree::data_structures::modint::ModInt;
use counttree::fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckMethod, CheckProperty};
use counttree::fuzzy_match::share_phase::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod, SharePhase};
use scuttlebutt::{AesRng, Channel};
use std::net::{TcpListener, TcpStream};
use std::io::{BufReader, BufWriter};
use std::thread;
use std::time::{Duration, Instant};
use rand::Rng;
use clap::{Arg, App};

fn setup_garbler_channel(port: u16) -> Result<CommTrackingChannel, Box<dyn std::error::Error>> {
    let addr = format!("127.0.0.1:{}", port);
    println!("Garbler connecting to evaluator at {}", addr);
    
    // Give evaluator time to start listening
    thread::sleep(Duration::from_millis(500));
    
    let stream = TcpStream::connect(&addr)?;
    stream.set_nodelay(true)?;
    let reader = BufReader::new(stream.try_clone()?);
    let writer = BufWriter::new(stream);
    Ok(CommTrackingChannel::new(reader, writer))
}

fn setup_evaluator_channel(port: u16) -> Result<CommTrackingChannel, Box<dyn std::error::Error>> {
    let addr = format!("127.0.0.1:{}", port);
    println!("Evaluator listening on {}", addr);
    
    let listener = TcpListener::bind(&addr)?;
    let (stream, _) = listener.accept()?;
    stream.set_nodelay(true)?;
    let reader = BufReader::new(stream.try_clone()?);
    let writer = BufWriter::new(stream);
    Ok(CommTrackingChannel::new(reader, writer))
}

fn generate_test_inputs(num_inputs: usize, modulus: u128) -> Vec<ModInt> {
    let mut rng = rand::thread_rng();
    (0..num_inputs)
        .map(|_| {
            ModInt::new(rng.random::<u128>(), modulus)
        })
        .collect()
}

fn run_garbler_benchmark(port: u16, h2: usize, h3: usize, d: usize, num_tests: usize, mu: u128) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running as GARBLER");
    
    println!("Parameters:");
    println!("  Input bit length (h2): {}", h2);
    println!("  Output bit length (h3): {}", h3);
    println!("  Dimensions (d): {}", d);
    println!("  Number of tests: {}", num_tests);
    
    // Create CheckConfig for garbler
    let config = CheckConfig {
        h2,
        h3,
        d,
        is_garbler_side: true,
        property: CheckProperty::MuBounded,
        method: CheckMethod::GC,
    };
    
    // Create SharePhase (dummy configuration since we're not using it for generation)
    let share_config = ShareConfig {
        method: ShareMethod::OKVS,
        metric: DistanceMetric::LInfinity,
        dictionary_type: DictionaryType::Known,
        h1: 11,
        h2: 20,
        d: 2,
    };
    
    let share_phase = SharePhase::new(share_config);
    let check_phase = CheckPhase::new(config, share_phase);
    
    // Generate test inputs - 1000 Vec<bool> with h2 bits each
    let modulus = 1u128 << (h2 as u128);
    println!("Generating {} test inputs with modulus {}", num_tests, modulus);
    let inputs = generate_test_inputs(num_tests, modulus);
    
    // Setup communication channel
    let mut channel = setup_garbler_channel(port)?;
    
    println!("Starting garbler benchmark...");
    let mut rng = AesRng::new();
    let start_time = Instant::now();
    
    let _results = check_phase.batch_mu_bounded_testing_gc(
        &inputs,
        &ModInt::new(mu, modulus),
        &mut channel,
        &mut rng,
    ).map_err(|e| format!("CheckPhase error: {:?}", e))?;
    
    let elapsed = start_time.elapsed();
    
    // Print results
    println!("\n=== Garbler Benchmark Results ===");
    println!("Number of equality tests: {}", num_tests);
    println!("Input bit length: {}", h2);
    println!("Garbler time: {:?}", elapsed);
    println!("Average time per test: {:?}", elapsed / num_tests as u32);
    let (sent, received) = channel.get_communication_stats();
    println!("Communication sent: {} bytes", sent);
    println!("Communication received: {} bytes", received);
    
    println!("\nGarbler benchmark completed successfully!");
    Ok(())
}

fn run_evaluator_benchmark(port: u16, h2: usize, h3: usize, d: usize, num_tests: usize, mu: u128) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running as EVALUATOR");
    
    println!("Parameters:");
    println!("  Input bit length (h2): {}", h2);
    println!("  Output bit length (h3): {}", h3);
    println!("  Dimensions (d): {}", d);
    println!("  Number of tests: {}", num_tests);
    
    // Create CheckConfig for evaluator
    let config = CheckConfig {
        h2,
        h3,
        d,
        is_garbler_side: false,
        property: CheckProperty::MuBounded,
        method: CheckMethod::GC,
    };
    
    // Create SharePhase (dummy configuration since we're not using it for generation)
    let share_config = ShareConfig {
        method: ShareMethod::OKVS,
        metric: DistanceMetric::LInfinity,
        dictionary_type: DictionaryType::Known,
        h1: 11,
        h2: 20,
        d: 2,
    };
    
    let share_phase = SharePhase::new(share_config);
    let check_phase = CheckPhase::new(config, share_phase);
    
    // Generate test inputs - 1000 Vec<bool> with h2 bits each
    let modulus = 1u128 << (h2 as u128);
    println!("Generating {} test inputs with modulus {}", num_tests, modulus);
    let inputs = generate_test_inputs(num_tests, modulus);

    // Setup communication channel
    let mut channel = setup_evaluator_channel(port)?;
    
    println!("Starting evaluator benchmark...");
    let mut rng = AesRng::new();
    let start_time = Instant::now();
    
    let _results = check_phase.batch_mu_bounded_testing_gc(
        &inputs,
        &ModInt::new(mu, modulus),
        &mut channel,
        &mut rng,
    ).map_err(|e| format!("CheckPhase error: {:?}", e))?;
    
    let elapsed = start_time.elapsed();
    
    // Print results
    println!("\n=== Evaluator Benchmark Results ===");
    println!("Number of equality tests: {}", num_tests);
    println!("Input bit length: {}", h2);
    println!("Evaluator time: {:?}", elapsed);
    println!("Average time per test: {:?}", elapsed / num_tests as u32);
    let (sent, received) = channel.get_communication_stats();
    println!("Communication sent: {} bytes", sent);
    println!("Communication received: {} bytes", received);
    
    println!("\nEvaluator benchmark completed successfully!");
    Ok(())
}

fn main() {
    let matches = App::new("Batch Equality GC Benchmark")
        .version("1.0")
        .author("Your Name")
        .about("Benchmarks batch equality testing using garbled circuits")
        .arg(Arg::with_name("role")
            .short("r")
            .long("role")
            .value_name("ROLE")
            .help("Role to play: 'garbler' or 'evaluator'")
            .required(true)
            .takes_value(true))
        .arg(Arg::with_name("port")
            .short("p")
            .long("port")
            .value_name("PORT")
            .help("Port number for communication")
            .default_value("8080")
            .takes_value(true))
        .get_matches();

    let role = matches.value_of("role").unwrap();
    let port: u16 = matches.value_of("port").unwrap()
        .parse()
        .expect("Port must be a valid number");

    // Configuration parameters
    let h2 = 16; // Input bit length
    let h3 = 20;  // Output bit length
    let d = 3;   // Number of dimensions
    let num_tests = 21000; // Number of equality tests to perform
    let mu = 1000;
    

    let result = match role.to_lowercase().as_str() {
        "garbler" => run_garbler_benchmark(port, h2, h3, d, num_tests, mu),
        "evaluator" => run_evaluator_benchmark(port, h2, h3, d, num_tests, mu),
        _ => {
            eprintln!("Invalid role '{}'. Must be 'garbler' or 'evaluator'", role);
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
