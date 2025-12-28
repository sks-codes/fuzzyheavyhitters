// Benchmark: Tokio async TCP vs std::net TCP with Rayon
// Run with: cargo run --release --bin tokio_vs_std -- [tokio|std]

use rayon::prelude::*;
use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

const ADDR: &str = "172.31.41.251:3000";
const N_MSGS: usize = 1000;
const MSG: &[u8] = b"ping";
const N_THREADS: usize = 4;

fn std_server() {
    let listener = TcpListener::bind(ADDR).unwrap();
    // Accept N_MSGS connections first
    let mut streams = Vec::with_capacity(N_MSGS);
    for _ in 0..N_MSGS {
        let (stream, _) = listener.accept().unwrap();
        streams.push(stream);
    }
    // Now spawn threads to handle each stream
    for mut stream in streams {
        std::thread::spawn(move || {
            let mut buf = [0u8; 4];
            while let Ok(_) = stream.read_exact(&mut buf) {
                // echo back
                let _ = stream.write_all(&buf);
            }
        });
    }
    // Keep server alive
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn std_client() {
    // Create all streams first
    let streams: Vec<TcpStream> = (0..N_MSGS)
        .map(|_| TcpStream::connect(ADDR).unwrap())
        .collect();
    // Now send/receive in parallel
    let start = Instant::now();
    streams.into_par_iter().for_each(|mut stream| {
        for _ in 0..N_THREADS {
            stream.write_all(MSG).unwrap();
            let mut buf = [0u8; 4];
            stream.read_exact(&mut buf).unwrap();
        }
    });
    let dur = start.elapsed();
    println!(
        "[std+rayon] Sent {} messages in {:?}",
        N_MSGS * N_THREADS,
        dur
    );
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!(
            "Usage: {} [tokio-server|tokio-client|std-server|std-client]",
            args[0]
        );
        return;
    }
    match args[1].as_str() {
        "tokio-server" => tokio_server().await,
        "tokio-client" => tokio_client().await,
        "std-server" => std_server(),
        "std-client" => std_client(),
        _ => println!("Unknown mode"),
    }
}

// --- Tokio async version ---
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener as TokioTcpListener, TcpStream as TokioTcpStream};

async fn tokio_server() {
    let listener = TokioTcpListener::bind(ADDR).await.unwrap();
    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 4];
            loop {
                if socket.read_exact(&mut buf).await.is_err() {
                    break;
                }
                let _ = socket.write_all(&buf).await;
            }
        });
    }
}

async fn tokio_client() {
    let start = Instant::now();
    let mut handles = vec![];
    for _ in 0..N_MSGS {
        handles.push(tokio::spawn(async move {
            let mut stream = TokioTcpStream::connect(ADDR).await.unwrap();
            for _ in 0..N_THREADS {
                stream.write_all(MSG).await.unwrap();
                let mut buf = [0u8; 4];
                stream.read_exact(&mut buf).await.unwrap();
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    let dur = start.elapsed();
    println!("[tokio] Sent {} messages in {:?}", N_MSGS * N_THREADS, dur);
}
