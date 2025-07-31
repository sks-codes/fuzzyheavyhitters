use std::io::{BufReader, BufWriter};
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::thread;
use std::time::{Duration, Instant};
use scuttlebutt::AesRng;
use counttree::data_structures::modint::ModInt;
use counttree::garbled_circuits::greater_than_or_equal_threshold::{
    multiple_gb_greater_than_ss, multiple_ev_greater_than_ss
};
use counttree::channel::{CommTrackingChannel, result_exchange};

type SocketChannel = CommTrackingChannel;

fn create_socket_channel_server(port: u16) -> std::io::Result<SocketChannel> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))?;
    println!("Server listening on port {}", port);
    
    let (stream, addr) = listener.accept()?;
    println!("Accepted connection from {}", addr);
    
    stream.set_nodelay(true)?;
    let reader = BufReader::new(stream.try_clone()?);
    let writer = BufWriter::new(stream);
    
    Ok(CommTrackingChannel::new(reader, writer))
}

fn create_socket_channel_client(addr: SocketAddr) -> std::io::Result<SocketChannel> {
    let stream = TcpStream::connect(addr)?;
    println!("Connected to server at {}", addr);
    
    stream.set_nodelay(true)?;
    let reader = BufReader::new(stream.try_clone()?);
    let writer = BufWriter::new(stream);
    
    Ok(CommTrackingChannel::new(reader, writer))
}

fn run_garbler(port: u16, test_cases: Vec<(u128, u128, u128)>, modulus: u128) -> std::io::Result<Vec<bool>> {
    let mut channel = create_socket_channel_server(port)?;
    
    // Reset communication stats
    channel.reset_stats();
    
    let start_time = Instant::now();
    
    // Extract y and t values for garbler
    let y_values: Vec<ModInt> = test_cases.iter().map(|(_, y, _)| ModInt::new(*y, modulus)).collect();
    let t_values: Vec<ModInt> = test_cases.iter().map(|(_, _, t)| ModInt::new(*t, modulus)).collect();
    
    let mut rng = AesRng::new();
    let garbler_results = multiple_gb_greater_than_ss(&mut rng, &mut channel, &y_values, &t_values);
    
    let elapsed = start_time.elapsed();
    let (bytes_sent, bytes_received) = channel.get_communication_stats();
    
    println!("Garbler Results:");
    println!("  Time elapsed: {:?}", elapsed);
    println!("  Bytes sent: {}", bytes_sent);
    println!("  Bytes received: {}", bytes_received);
    println!("  Total communication: {} bytes", bytes_sent + bytes_received);
    println!("  Garbler shares: {:?}", garbler_results);
    
    // Exchange results with evaluator
    println!("\nExchanging results with evaluator...");
    result_exchange::send_results(&mut channel, &garbler_results)?;
    let evaluator_results = result_exchange::receive_results(&mut channel)?;
    
    println!("  Evaluator shares: {:?}", evaluator_results);
    
    // Combine results (XOR)
    let final_results: Vec<bool> = garbler_results
        .iter()
        .zip(evaluator_results.iter())
        .map(|(&g, &e)| g ^ e)
        .collect();
    
    println!("  Final results: {:?}", final_results);
    
    // Verify against expected results
    println!("\nVerification:");
    for (i, &(x, y, t)) in test_cases.iter().enumerate() {
        let sum_mod = (x + y) % modulus;
        let expected = sum_mod >= t;
        let actual = final_results[i];
        let status = if expected == actual { "✓" } else { "✗" };
        println!("  Case {}: ({}+{}) mod {} = {} >= {} = {} {} ({})", 
                 i, x, y, modulus, sum_mod, t, expected, status, 
                 if expected == actual { "PASS" } else { "FAIL" });
    }
    
    Ok(final_results)
}

fn run_evaluator(addr: SocketAddr, test_cases: Vec<(u128, u128, u128)>, modulus: u128) -> std::io::Result<Vec<bool>> {
    // Wait a bit for server to start
    thread::sleep(Duration::from_millis(1000));
    
    let mut channel = create_socket_channel_client(addr)?;
    
    // Reset communication stats
    channel.reset_stats();
    
    let start_time = Instant::now();
    
    // Extract x values for evaluator
    let x_values: Vec<ModInt> = test_cases.iter().map(|(x, _, _)| ModInt::new(*x, modulus)).collect();
    
    let mut rng = AesRng::new();
    let evaluator_results = multiple_ev_greater_than_ss(&mut rng, &mut channel, &x_values);
    
    let elapsed = start_time.elapsed();
    let (bytes_sent, bytes_received) = channel.get_communication_stats();
    
    println!("Evaluator Results:");
    println!("  Time elapsed: {:?}", elapsed);
    println!("  Bytes sent: {}", bytes_sent);
    println!("  Bytes received: {}", bytes_received);
    println!("  Total communication: {} bytes", bytes_sent + bytes_received);
    println!("  Evaluator shares: {:?}", evaluator_results);
    
    // Exchange results with garbler
    println!("\nExchanging results with garbler...");
    let garbler_results = result_exchange::receive_results(&mut channel)?;
    result_exchange::send_results(&mut channel, &evaluator_results)?;
    
    println!("  Garbler shares: {:?}", garbler_results);
    
    // Combine results (XOR)
    let final_results: Vec<bool> = garbler_results
        .iter()
        .zip(evaluator_results.iter())
        .map(|(&g, &e)| g ^ e)
        .collect();
    
    println!("  Final results: {:?}", final_results);
    
    // Verify against expected results
    println!("\nVerification:");
    for (i, &(x, y, t)) in test_cases.iter().enumerate() {
        let sum_mod = (x + y) % modulus;
        let expected = sum_mod >= t;
        let actual = final_results[i];
        let status = if expected == actual { "✓" } else { "✗" };
        println!("  Case {}: ({}+{}) mod {} = {} >= {} = {} {} ({})", 
                 i, x, y, modulus, sum_mod, t, expected, status, 
                 if expected == actual { "PASS" } else { "FAIL" });
    }
    
    Ok(final_results)
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: {} <role> [port]", args[0]);
        eprintln!("  role: 'garbler' or 'evaluator'");
        eprintln!("  port: port number (default: 8080)");
        std::process::exit(1);
    }
    
    let role = &args[1];
    let port: u16 = args.get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    
    // Use small modulus for demonstration
    let modulus = 8u128;
    
    // Create test cases: (x, y, t) where we compute (x + y) mod modulus >= t
    let test_cases = vec![
        (3, 2, 5),  // (3+2) mod 8 = 5 >= 5 = true
        (1, 4, 3),  // (1+4) mod 8 = 5 >= 3 = true  
        (2, 1, 7),  // (2+1) mod 8 = 3 >= 7 = false
        (0, 6, 2),  // (0+6) mod 8 = 6 >= 2 = true
        (7, 3, 1),  // (7+3) mod 8 = 2 >= 1 = true
    ];
    
    println!("Running garbled circuit demo with {} test cases", test_cases.len());
    println!("Test cases (x, y, t): {:?}", test_cases);
    
    // Print expected results for verification
    println!("Expected results:");
    for (i, &(x, y, t)) in test_cases.iter().enumerate() {
        let sum_mod = (x + y) % modulus;
        let expected = sum_mod >= t;
        println!("  Case {}: ({}+{}) mod {} = {} >= {} = {}", 
                 i, x, y, modulus, sum_mod, t, expected);
    }
    
    match role.as_str() {
        "garbler" => {
            println!("Starting as garbler on port {}", port);
            let _final_results = run_garbler(port, test_cases.clone(), modulus)?;
            
            println!("\nGarbler finished.");
        },
        "evaluator" => {
            println!("Starting as evaluator, connecting to localhost:{}", port);
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            let _final_results = run_evaluator(addr, test_cases.clone(), modulus)?;
            
            println!("\nEvaluator finished.");
        },
        _ => {
            eprintln!("Invalid role: {}. Use 'garbler' or 'evaluator'", role);
            std::process::exit(1);
        }
    }
    
    Ok(())
}