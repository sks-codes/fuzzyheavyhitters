use anyhow::Result;
use mosaic::{
    data_structures::ringvec::RingVec,
    fss::interval::{IntervalFSSEval, IntervalFSSKey},
    util::{bits_to_u128_msb, u128_to_bits_msb},
};

const ALPHA: u128 = 10;
const BETA: u128 = 20;
const BIT_LENGTH: usize = 5;
const MODULUS: u128 = 1 << 20;

#[test]
fn interval_fss_expand_prefix() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let beta_bits = u128_to_bits_msb(BETA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");
    let c = RingVec::new(vec![5, 6], MODULUS).expect("Cannot create ringvec c");

    let (key0, key1) = IntervalFSSKey::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, MODULUS)?;

    let mut evals0: Vec<IntervalFSSEval> = vec![key0.init_eval(MODULUS)?];
    let mut evals1: Vec<IntervalFSSEval> = vec![key1.init_eval(MODULUS)?];

    for level in 1..BIT_LENGTH {
        let mut next_evals0 = Vec::with_capacity(evals0.len() * 2);
        for eval in evals0.iter() {
            let (left, right) = key0.expand_prefix(eval, MODULUS)?;
            next_evals0.push(left);
            next_evals0.push(right);
        }
        evals0 = next_evals0;
        let mut next_evals1 = Vec::with_capacity(evals1.len() * 2);
        for eval in evals1.iter() {
            let (left, right) = key1.expand_prefix(eval, MODULUS)?;
            next_evals1.push(left);
            next_evals1.push(right);
        }
        evals1 = next_evals1;
        let domain_size = 1usize << level;
        let prefix_alpha_bits: Vec<bool> = alpha_bits[..level].to_vec();
        let prefix_beta_bits: Vec<bool> = beta_bits[..level].to_vec();
        let prefix_alpha = bits_to_u128_msb(&prefix_alpha_bits);
        let prefix_beta = bits_to_u128_msb(&prefix_beta_bits);
        for i in 0..domain_size {
            let res = evals0[i].result() - evals1[i].result();
            if (i as u128) < prefix_alpha {
                assert_eq!(
                    res, a,
                    "[IntervalFSS] Fail at level {level}, position {i}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, a, res
                );
            } else if (i as u128) <= prefix_beta {
                assert_eq!(
                    res, b,
                    "[IntervalFSS] Fail at level {level}, position {i}, prefix alpha: {:?}, prefix beta: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, prefix_beta, b, res
                );
            } else {
                assert_eq!(
                    res, c,
                    "[IntervalFSS] Fail at level {level}, position {i}, prefix beta: {:?}, expected {:?}, got {:?}",
                    prefix_beta, c, res
                );
            }
        }
    }
    Ok(())
}

#[test]
fn interval_fss_eval_interval() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let beta_bits = u128_to_bits_msb(BETA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");
    let c = RingVec::new(vec![5, 6], MODULUS).expect("Cannot create ringvec c");

    let (key0, key1) = IntervalFSSKey::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, MODULUS)?;

    for level in 1..BIT_LENGTH {
        let domain_size = 1usize << level;
        let prefix_alpha_bits: Vec<bool> = alpha_bits[..level].to_vec();
        let prefix_beta_bits: Vec<bool> = beta_bits[..level].to_vec();
        let prefix_alpha = bits_to_u128_msb(&prefix_alpha_bits);
        let prefix_beta = bits_to_u128_msb(&prefix_beta_bits);
        for x in 0..domain_size {
            let x_bits = u128_to_bits_msb(x as u128, level);
            let res0 = key0.eval_interval_fss(&x_bits, MODULUS)?;
            let res1 = key1.eval_interval_fss(&x_bits, MODULUS)?;
            let res = res0 - res1;
            if (x as u128) < prefix_alpha {
                assert_eq!(
                    res, a,
                    "[IntervalFSS] Fail at level {level}, position {x}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, a, res
                );
            } else if (x as u128) <= prefix_beta {
                assert_eq!(
                    res, b,
                    "[IntervalFSS] Fail at level {level}, position {x}, prefix alpha: {:?}, prefix beta: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, prefix_beta, b, res
                );
            } else {
                assert_eq!(
                    res, c,
                    "[IntervalFSS] Fail at level {level}, position {x}, prefix beta: {:?}, expected {:?}, got {:?}",
                    prefix_beta, c, res
                );
            }
        }
    }

    Ok(())
}

#[test]
fn interval_fss_serialization() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let beta_bits = u128_to_bits_msb(BETA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");
    let c = RingVec::new(vec![5, 6], MODULUS).expect("Cannot create ringvec c");

    let (key0, key1) = IntervalFSSKey::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, MODULUS)?;

    let key0_bytes = key0.to_bytes()?;
    let (key0_cmp, size) = IntervalFSSKey::from_bytes(&key0_bytes, MODULUS)?;
    assert_eq!(size, key0_bytes.len(), "Wrong deserialization length for interval_fss key0!");
    assert_eq!(
        key0_cmp.to_bytes()?,
        key0_bytes,
        "Wrong serialization round-trip for interval_fss key0!"
    );

    let key1_bytes = key1.to_bytes()?;
    let (key1_cmp, size1) = IntervalFSSKey::from_bytes(&key1_bytes, MODULUS)?;
    assert_eq!(size1, key1_bytes.len(), "Wrong deserialization length for interval_fss key1!");
    assert_eq!(
        key1_cmp.to_bytes()?,
        key1_bytes,
        "Wrong serialization round-trip for interval_fss key1!"
    );

    Ok(())
}
