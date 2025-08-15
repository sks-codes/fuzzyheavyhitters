use counttree::{
    fuzzy_match::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, DictionaryType, DistanceMetric},
    util::u128_to_bits,
    fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckMethod, CheckData},
    fuzzy_match::threshold_phase::{ThresholdPhase, ThresholdConfig, ThresholdMethod, ThresholdData},
    data_structures::payload::RingVec,
    fss::interval::IntervalFSSKey,
    channel::CommTrackingChannel,
};
use scuttlebutt::AesRng;
use std::net::{TcpListener, TcpStream};
use std::io::{BufReader, BufWriter};
use std::thread;
use std::sync::mpsc;
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test the complete threshold phase pipeline with brute force testing:
    /// 1. Multiple clients generate shares using SharePhase
    /// 2. Two servers evaluate at MULTIPLE query points with all client shares
    /// 3. Run check phase to get n ring shares (one per client) for each query
    /// 4. Run threshold phase to aggregate and compare with threshold for each query
    /// 5. Exchange final bits to reconstruct result for each query
    #[test]
    fn test_threshold_phase_garbled_circuits() {
        unimplemented!()
    }

    /// Test the IntervalFSS-based threshold phase implementation
    #[test]
    fn test_threshold_phase_intervalfss() {
        unimplemented!()
    }
    
}
