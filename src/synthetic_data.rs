//! Synthetic Data Generator for Fuzzy Heavy Hitters
//! 
//! This module generates synthetic location data with clustered patterns.
//! It creates multiple clusters of varying sizes, where each cluster contains
//! points that are close to each other geographically.

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Configuration for synthetic data generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticDataConfig {
    /// Number of clusters to generate
    pub num_clusters: usize,
    /// Minimum number of points per cluster
    pub min_cluster_size: usize,
    /// Maximum number of points per cluster
    pub max_cluster_size: usize,
    /// Coordinate space bounds [min, max] for cluster centers
    pub coordinate_bounds: (u128, u128),
    /// Maximum radius for points within a cluster
    pub max_cluster_radius: u128,
    /// Number of dimensions (typically 2 for lat/lon)
    pub dimensions: usize,
}

impl Default for SyntheticDataConfig {
    fn default() -> Self {
        Self {
            num_clusters: 10,
            min_cluster_size: 5,
            max_cluster_size: 50,
            coordinate_bounds: (0, 1000),
            max_cluster_radius: 20,
            dimensions: 2,
        }
    }
}

/// A cluster of points
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    /// Center point of the cluster
    pub center: Vec<u128>,
    /// All points in this cluster
    pub points: Vec<Vec<u128>>,
    /// Size of this cluster
    pub size: usize,
}

/// Generated synthetic dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticDataset {
    /// Configuration used to generate this dataset
    pub config: SyntheticDataConfig,
    /// All clusters in the dataset
    pub clusters: Vec<Cluster>,
    /// All client points (flattened from clusters)
    pub client_points: Vec<Vec<u128>>,
    /// Server query points (cluster centers)
    pub server_query_points: Vec<Vec<u128>>,
    /// Total number of client points
    pub total_client_points: usize,
}

/// Synthetic data generator
pub struct SyntheticDataGenerator {
    config: SyntheticDataConfig,
}

impl SyntheticDataGenerator {
    /// Create a new synthetic data generator with the given configuration
    pub fn new(config: SyntheticDataConfig) -> Self {
        Self { config }
    }

    /// Create a new generator with default configuration
    pub fn default() -> Self {
        Self::new(SyntheticDataConfig::default())
    }

    /// Generate a complete synthetic dataset
    pub fn generate(&self) -> SyntheticDataset {
        let mut rng = rand::thread_rng();
        let mut clusters = Vec::new();
        let mut all_client_points = Vec::new();
        let mut server_query_points = Vec::new();

        println!("=== Generating Synthetic Dataset ===");
        println!("Configuration: {:?}", self.config);

        for cluster_id in 0..self.config.num_clusters {
            // Generate random cluster center
            let center = self.generate_random_point(&mut rng);
            
            // Generate random cluster size
            let cluster_size = rng.gen_range(
                self.config.min_cluster_size..=self.config.max_cluster_size
            );

            // Generate points around the cluster center
            let mut cluster_points = Vec::new();
            for _ in 0..cluster_size {
                let point = self.generate_point_near_center(&center, &mut rng);
                cluster_points.push(point.clone());
                all_client_points.push(point);
            }

            let cluster = Cluster {
                center: center.clone(),
                points: cluster_points,
                size: cluster_size,
            };

            server_query_points.push(center);
            clusters.push(cluster);

            println!("Generated cluster {}: center={:?}, size={}", 
                cluster_id + 1, clusters[cluster_id].center, cluster_size);
        }

        let total_points = all_client_points.len();
        println!("Total client points generated: {}", total_points);
        println!("Server query points (cluster centers): {}", server_query_points.len());

        SyntheticDataset {
            config: self.config.clone(),
            clusters,
            client_points: all_client_points,
            server_query_points,
            total_client_points: total_points,
        }
    }

    /// Generate a random point within the coordinate bounds
    fn generate_random_point(&self, rng: &mut impl Rng) -> Vec<u128> {
        let mut point = Vec::with_capacity(self.config.dimensions);
        for _ in 0..self.config.dimensions {
            let coord = rng.gen_range(
                self.config.coordinate_bounds.0..=self.config.coordinate_bounds.1
            );
            point.push(coord);
        }
        point
    }

    /// Generate a point near the given center, within the cluster radius
    fn generate_point_near_center(&self, center: &[u128], rng: &mut impl Rng) -> Vec<u128> {
        let mut point = Vec::with_capacity(self.config.dimensions);
        
        for i in 0..self.config.dimensions {
            // Generate random offset within cluster radius
            let offset = rng.gen_range(0..=self.config.max_cluster_radius);
            let direction = if rng.gen_bool(0.5) { 1 } else { -1 };
            
            // Apply offset to center coordinate, ensuring we stay within bounds
            let new_coord = if direction > 0 {
                std::cmp::min(
                    center[i] + offset,
                    self.config.coordinate_bounds.1
                )
            } else {
                std::cmp::max(
                    center[i].saturating_sub(offset),
                    self.config.coordinate_bounds.0
                )
            };
            
            point.push(new_coord);
        }
        
        point
    }
}

impl SyntheticDataset {
    /// Print a summary of the dataset
    pub fn print_summary(&self) {
        println!("=== Synthetic Dataset Summary ===");
        println!("Total clusters: {}", self.clusters.len());
        println!("Total client points: {}", self.total_client_points);
        println!("Server query points: {}", self.server_query_points.len());
        println!();

        for (i, cluster) in self.clusters.iter().enumerate() {
            println!("Cluster {}: center={:?}, size={}", 
                i + 1, cluster.center, cluster.size);
        }
        println!();
    }

    /// Get all client points as a flat vector
    pub fn get_client_points(&self) -> &Vec<Vec<u128>> {
        &self.client_points
    }

    /// Get all server query points (cluster centers)
    pub fn get_server_query_points(&self) -> &Vec<Vec<u128>> {
        &self.server_query_points
    }

    /// Get cluster information
    pub fn get_clusters(&self) -> &Vec<Cluster> {
        &self.clusters
    }

    /// Find which cluster a given client point belongs to
    pub fn find_cluster_for_point(&self, point: &[u128]) -> Option<usize> {
        for (cluster_idx, cluster) in self.clusters.iter().enumerate() {
            if cluster.points.iter().any(|p| p == point) {
                return Some(cluster_idx);
            }
        }
        None
    }

    /// Get expected matches for a query point with given delta
    pub fn get_expected_matches(&self, query_point: &[u128], delta: u128) -> Vec<usize> {
        let mut matches = Vec::new();
        
        for (client_idx, client_point) in self.client_points.iter().enumerate() {
            let is_match = client_point.iter().zip(query_point.iter()).all(|(c, q)| {
                let diff = if *c > *q { *c - *q } else { *q - *c };
                diff <= delta
            });
            
            if is_match {
                matches.push(client_idx);
            }
        }
        
        matches
    }
}
