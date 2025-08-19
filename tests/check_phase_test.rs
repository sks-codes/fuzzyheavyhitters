use counttree::{
    fuzzy_match::share_phase::{SharePhase, ShareConfig, ShareMethod, ShareData, SharedRange, DictionaryType, DistanceMetric},
    fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckMethod, CheckData},
    data_structures::modint::ModInt,
    data_structures::payload::RingVec,
    util::{u128_to_bits, u128_to_bits_msb},
    channel::CommTrackingChannel,
    fss::interval::IntervalFSSKey,
};
use scuttlebutt::AesRng;
use rand::Rng;
use std::net::{TcpListener, TcpStream};
use std::io::{BufReader, BufWriter};
use std::thread;
use std::sync::mpsc;
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linf_check_garbled_circuits() {
        
    }

    #[test]
    fn test_linf_check_dpf() {
        unimplemented!()
    }


    /// Test Lp garbled circuits check phase with Lp metric
    /// Uses FSS and DistanceFSSL2 for sharing with 3-dimensional points
    #[test]
    fn test_lpgarbledcircuits_check_lp() {
        unimplemented!()
    }

    /// Test Lp IntervalFSS check phase with Lp metric
    /// Uses FSS and DistanceFSSL2 for sharing with 3-dimensional points
    #[test]
    fn test_lpintervalfss_check_lp() {
        unimplemented!()
    }
}
