use counttree::okvs_f2k::{RbOkvsF2k, OkvsError};
use rand::Rng;

#[test]
fn test_okvs_basic_functionality() {
    // Test parameters as requested
    let kv_count = 10;
    let band_width = 40;
    let columns = 41;
    
    // Generate random seeds for the hash functions
    let r1: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let r2: [u8; 16] = [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    
    // Create OKVS instance
    let okvs: RbOkvsF2k<u128> = RbOkvsF2k::new(kv_count, columns, band_width, &r1, &r2);
    
    // Generate test data
    let mut rng = rand::thread_rng();
    let mut keys = Vec::new();
    let mut values = Vec::new();
    
    // Generate random keys and values
    for _ in 0..kv_count {
        let key_length = rng.gen_range(10..50); // Random key length
        let key: Vec<bool> = (0..key_length).map(|_| rng.gen_bool(0.5)).collect();
        let value: u128 = rng.gen();
        
        keys.push(key);
        values.push(value);
    }
    
    // Test encoding
    let encoding_result = okvs.encode(&keys, &values);
    assert!(encoding_result.is_ok(), "Encoding should succeed");
    
    let encoding = encoding_result.unwrap();
    assert_eq!(encoding.len(), columns, "Encoding length should match columns");
    
    // Test decoding
    let decoded_values = okvs.decode(&encoding, &keys);
    assert_eq!(decoded_values.len(), kv_count, "Decoded values length should match kv_count");
    
    // Verify that decoding returns the original values
    for i in 0..kv_count {
        assert_eq!(decoded_values[i], values[i], 
                   "Decoded value at index {} should match original value", i);
    }
    
    println!("✓ Basic OKVS functionality test passed!");
}
