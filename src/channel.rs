use std::io::{BufReader, BufWriter};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
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

/// Utility functions for result exchange in secret sharing protocols
pub mod result_exchange {
    use super::CommTrackingChannel;
    use scuttlebutt::AbstractChannel;

    /// Send boolean results over the channel
    pub fn send_results(channel: &mut CommTrackingChannel, results: &[bool]) -> std::io::Result<()> {
        // Send number of results first
        let num_results = results.len() as u32;
        channel.write_bytes(&num_results.to_le_bytes())?;
        
        // Pack results into bytes (8 bools per byte)
        let mut bytes = Vec::new();
        for chunk in results.chunks(8) {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                if bit {
                    byte |= 1 << i;
                }
            }
            bytes.push(byte);
        }
        
        // Send packed bytes
        channel.write_bytes(&bytes)?;
        channel.flush()?;
        Ok(())
    }

    /// Receive boolean results from the channel
    pub fn receive_results(channel: &mut CommTrackingChannel) -> std::io::Result<Vec<bool>> {
        // Receive number of results
        let mut num_bytes = [0u8; 4];
        channel.read_bytes(&mut num_bytes)?;
        let num_results = u32::from_le_bytes(num_bytes) as usize;
        
        // Calculate number of bytes needed
        let bytes_needed = (num_results + 7) / 8; // Ceiling division
        
        // Receive packed bytes
        let mut bytes = vec![0u8; bytes_needed];
        channel.read_bytes(&mut bytes)?;
        
        // Unpack bytes to bools
        let mut results = Vec::new();
        for (_byte_idx, &byte) in bytes.iter().enumerate() {
            for bit_idx in 0..8 {
                if results.len() >= num_results {
                    break;
                }
                let bit = (byte >> bit_idx) & 1 == 1;
                results.push(bit);
            }
        }
        
        results.truncate(num_results);
        Ok(results)
    }

    /// Exchange results between two parties and combine them with XOR
    pub fn exchange_and_combine_results(
        channel: &mut CommTrackingChannel, 
        local_results: &[bool],
        send_first: bool
    ) -> std::io::Result<Vec<bool>> {
        let remote_results = if send_first {
            send_results(channel, local_results)?;
            receive_results(channel)?
        } else {
            let remote = receive_results(channel)?;
            send_results(channel, local_results)?;
            remote
        };

        // Combine results (XOR for secret sharing)
        let final_results: Vec<bool> = local_results
            .iter()
            .zip(remote_results.iter())
            .map(|(&local, &remote)| local ^ remote)
            .collect();

        Ok(final_results)
    }
}
