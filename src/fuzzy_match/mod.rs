//! Fuzzy Match Module
//! 
//! This module contains the three main phases of the fuzzy heavy hitters protocol:
//! - Share Phase: Clients generate secret shares of their location ranges
//! - Check Phase: Servers check if query points match client ranges using secure computation
//! - Threshold Phase: Servers aggregate match results and compare with threshold
//! - Protocol: High-level protocol implementation
//! - Dealer: FSS key generation and distribution
//! - Client: Client-side functionality for generating and distributing shares

pub mod share_phase;
pub mod check_phase;
pub mod threshold_phase;
pub mod strategies;
pub mod protocol;
pub mod dealer;
pub mod client;
