//! Garbled Circuit implementations and utilities
//! 
//! This module contains garbled circuit-based secure computation protocols,
//! including equality tests and threshold comparisons.

pub mod equality;
pub mod greater_than;
pub mod less_than_or_equal_threshold;
pub mod greater_than_or_equal_threshold;
pub mod equality_full;
pub mod batch_equality_full;