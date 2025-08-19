use std::io::{BufReader, BufWriter};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::net::TcpListener;
use scuttlebutt::{AbstractChannel, SyncChannel};

/// A wrapper around scuttlebutt's SyncChannel that tracks communication costs
#[derive(Clone)]
pub struct CommTrackingChannel {
    inner: SyncChannel<BufReader<TcpStream>, BufWriter<TcpStream>>,
    bytes_sent: Arc<Mutex<usize>>,
    bytes_received: Arc<Mutex<usize>>,
}

impl CommTrackingChannel {
    pub fn new(reader: BufReader<TcpStream>, writer: BufWriter<TcpStream>) -> Self {
        Self {
            inner: SyncChannel::new(reader, writer),
            bytes_sent: Arc::new(Mutex::new(0)),
            bytes_received: Arc::new(Mutex::new(0)),
        }
    }

    pub fn get_communication_stats(&self) -> (usize, usize) {
        let sent = *self.bytes_sent.lock().unwrap();
        let received = *self.bytes_received.lock().unwrap();
        (sent, received)
    }

    pub fn reset_stats(&self) {
        *self.bytes_sent.lock().unwrap() = 0;
        *self.bytes_received.lock().unwrap() = 0;
    }

    /// Get total communication cost (sent + received)
    pub fn get_total_communication(&self) -> usize {
        let (sent, received) = self.get_communication_stats();
        sent + received
    }

    /// Print communication statistics
    pub fn print_stats(&self, label: &str) {
        let (sent, received) = self.get_communication_stats();
        println!("{} Communication Stats:", label);
        println!("  Bytes sent: {}", sent);
        println!("  Bytes received: {}", received);
        println!("  Total: {} bytes", sent + received);
    }
}

impl AbstractChannel for CommTrackingChannel {
    fn write_bytes(&mut self, data: &[u8]) -> std::io::Result<()> {
        *self.bytes_sent.lock().unwrap() += data.len();
        self.inner.write_bytes(data)
    }

    fn read_bytes(&mut self, data: &mut [u8]) -> std::io::Result<()> {
        let result = self.inner.read_bytes(data);
        if result.is_ok() {
            *self.bytes_received.lock().unwrap() += data.len();
        }
        result
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn connect_to(ip: String, port: u16) -> Result<CommTrackingChannel, Box<dyn std::error::Error>> {
    // Give evaluator time to start listening
    thread::sleep(Duration::from_millis(100));
    
    let addr = format!("{}:{}", ip, port);
    println!("Connecting to {}", addr);
    let stream = TcpStream::connect(&addr)?;
    stream.set_nodelay(true)?;
    let reader = BufReader::new(stream.try_clone()?);
    let writer = BufWriter::new(stream);
    Ok(CommTrackingChannel::new(reader, writer))
}

fn listen_to(ip: String, port: u16) -> Result<CommTrackingChannel, Box<dyn std::error::Error>> {
    let addr = format!("{}:{}", ip, port);
    println!("Listening on {}", addr);
    
    let listener = TcpListener::bind(&addr)?;
    let (stream, _) = listener.accept()?;
    stream.set_nodelay(true)?;
    let reader = BufReader::new(stream.try_clone()?);
    let writer = BufWriter::new(stream);
    Ok(CommTrackingChannel::new(reader, writer))
}
