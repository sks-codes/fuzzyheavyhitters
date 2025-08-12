use std::time::Instant;
use counttree::fss::ldcf::LdcfKey;
use counttree::data_structures::payload::RingVec;
use rand::Rng;

fn main() {
    println!("LDCF Key Generation Benchmark");
    println!("================================");

    const N: usize = 2;
    let moduli = [1u128 << 1, 1u128 << 2, 1u128 << 4, 1u128 << 8, 1u128 << 20];
    println!("Using N = {}", N);

    // Test different domain sizes (bit lengths)
    let domain_sizes = [8, 12, 16, 20, 24, 28, 32];

    for &modulus in &moduli {
        println!("\n==============================");
        println!("Using modulus: 2^{} = {}", modulus.trailing_zeros(), modulus);
        for &domain_size in &domain_sizes {
            println!("\n--- Domain size: {} bits ---", domain_size);

            // Generate random alpha bit string
            let mut rng = rand::thread_rng();
            let alpha_bits: Vec<bool> = (0..domain_size).map(|_| rng.gen()).collect();

            // Generate random payload vectors
            let a = RingVec::<N>::random(modulus);
            let b = RingVec::<N>::random(modulus);

            // Benchmark key generation
            let iterations = 1000;

            let start = Instant::now();
            for _ in 0..iterations {
                let (_key0, _key1) = LdcfKey::<N>::gen_LdcfKey(
                    &alpha_bits,
                    &a,
                    &b,
                    modulus
                );
            }
            let keygen_duration = start.elapsed();

            println!("Key generation x {}: {:?}", iterations, keygen_duration);
            println!("Average per key generation: {:?}", keygen_duration / iterations);

            // Benchmark a single key generation to get key size and for serialization/deserialization
            let (key0, key1) = LdcfKey::<N>::gen_LdcfKey(
                &alpha_bits,
                &a,
                &b,
                modulus
            );
            let key0_bytes = key0.to_bytes();
            let key1_bytes = key1.to_bytes();
            println!("Key0 size: {} bytes", key0_bytes.len());
            println!("Key1 size: {} bytes", key1_bytes.len());

            // Benchmark serialization
            let start = Instant::now();
            for _ in 0..iterations {
                let _ = key0.to_bytes();
                let _ = key1.to_bytes();
            }
            let ser_duration = start.elapsed();
            println!("Serialization x {}: {:?}", 2*iterations, ser_duration);
            println!("Average per serialization: {:?}", ser_duration / (2*iterations));

            // Benchmark deserialization
            let start = Instant::now();
            for _ in 0..iterations {
                let _ = LdcfKey::<N>::from_bytes(&key0_bytes, modulus);
                let _ = LdcfKey::<N>::from_bytes(&key1_bytes, modulus);
            }
            let deser_duration = start.elapsed();
            println!("Deserialization x {}: {:?}", 2*iterations, deser_duration);
            println!("Average per deserialization: {:?}", deser_duration / (2*iterations));
        }
    }
}
