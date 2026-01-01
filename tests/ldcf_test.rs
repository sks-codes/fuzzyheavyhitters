use anyhow::Result;
use mosaic::{
    data_structures::ringvec::RingVec,
    fss::ldcf::{LdcfEval, LdcfKey},
    util::{bits_to_u128_msb, u128_to_bits_msb},
};

const ALPHA: u128 = 15;
const BIT_LENGTH: usize = 5;
const MODULUS: u128 = 1 << 20;

#[test]
fn ldcf_expand_prefix() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");

    let (ldcf_key0, ldcf_key1) = LdcfKey::gen_ldcf_key(&alpha_bits, &a, &b, MODULUS)?;

    let mut evals0: Vec<LdcfEval> = vec![ldcf_key0.init_eval(MODULUS)?];
    let mut evals1: Vec<LdcfEval> = vec![ldcf_key1.init_eval(MODULUS)?];

    for level in 1..BIT_LENGTH+1 {
        let mut next_evals0 = Vec::with_capacity(evals0.len() * 2);
        for eval in evals0.iter() {
            let (left, right) = ldcf_key0.expand_prefix(eval, MODULUS)?;
            next_evals0.push(left);
            next_evals0.push(right);
        }
        evals0 = next_evals0;
        let mut next_evals1 = Vec::with_capacity(evals1.len() * 2);
        for eval in evals1.iter() {
            let (left, right) = ldcf_key1.expand_prefix(eval, MODULUS)?;
            next_evals1.push(left);
            next_evals1.push(right);
        }
        evals1 = next_evals1;
        let domain_size = 1usize << level;
        let prefix_alpha_bits: Vec<bool> = alpha_bits[..level].to_vec();
        let prefix_alpha = bits_to_u128_msb(&prefix_alpha_bits);
        for i in 0..domain_size {
            let res = evals0[i].y().clone() - evals1[i].y().clone();
            if (i as u128) < prefix_alpha {
                assert_eq!(
                    res, a,
                    "[LDCF] Fail at level {level}, position {i}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, a, res
                );
            } else {
                assert_eq!(
                    res, b,
                    "[LDCF] Fail at level {level}, position {i}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, b, res
                );
            }
        }
    }
    Ok(())
}

#[test]
fn ldcf_eval_ldcf() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");

    let (ldcf_key0, ldcf_key1) = LdcfKey::gen_ldcf_key(&alpha_bits, &a, &b, MODULUS)?;

    for level in 1..BIT_LENGTH+1 {
        let domain_size = 1usize << level;
        let prefix_alpha_bits: Vec<bool> = alpha_bits[..level].to_vec();
        let prefix_alpha = bits_to_u128_msb(&prefix_alpha_bits);
        for x in 0..domain_size {
            let x_bits = u128_to_bits_msb(x as u128, level);
            let res0 = ldcf_key0.eval_ldcf(&x_bits, MODULUS)?;
            let res1 = ldcf_key1.eval_ldcf(&x_bits, MODULUS)?;
            let res = res0 - res1;
            if (x as u128) < prefix_alpha {
                assert_eq!(
                    res, a,
                    "[LDCF] Fail at level {level}, position {x}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, a, res
                );
            } else {
                assert_eq!(
                    res, b,
                    "[LDCF] Fail at level {level}, position {x}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, b, res
                );
            }
        }
    }

    Ok(())
}

#[test]
fn ldcf_full_domain_eval() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");

    let (ldcf_key0, ldcf_key1) = LdcfKey::gen_ldcf_key(&alpha_bits, &a, &b, MODULUS)?;

    let ldcf_evals0 = ldcf_key0.full_domain_eval(MODULUS, BIT_LENGTH)?;
    let ldcf_evals1 = ldcf_key1.full_domain_eval(MODULUS, BIT_LENGTH)?;

    let domain_size = 1 << BIT_LENGTH;
    for x in 0..domain_size {
        let res = ldcf_evals0[x].clone() - ldcf_evals1[x].clone();
        if (x as u128) < ALPHA {
            assert_eq!(
                res, a,
                "[LDCF Full Domain Eval] Fail at position {}, alpha: {:?}, expected {:?}, got {:?}",
                x, ALPHA, a, res
            );
        } else {
            assert_eq!(
                res, b,
                "[LDCF Full Domain Eval] Fail at position {}, alpha: {:?}, expected {:?}, got {:?}",
                x, ALPHA, b, res
            );
        }
    }

    Ok(())
}

#[test]
fn ldcf_full_domain_incremental_eval() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");

    let (ldcf_key0, ldcf_key1) = LdcfKey::gen_ldcf_key(&alpha_bits, &a, &b, MODULUS)?;

    let ldcf_evals0 = ldcf_key0.full_domain_incremental_eval(MODULUS, BIT_LENGTH)?;
    let ldcf_evals1 = ldcf_key1.full_domain_incremental_eval(MODULUS, BIT_LENGTH)?;

    for level in 1..BIT_LENGTH+1 {
        let domain_size = 1 << level;
        let prefix_alpha_bits: Vec<bool> = alpha_bits[..level].to_vec();
        let prefix_alpha = bits_to_u128_msb(&prefix_alpha_bits);
        for x in 0..domain_size {
            let res = ldcf_evals0[level][x].clone() - ldcf_evals1[level][x].clone();
            if (x as u128) < prefix_alpha {
                assert_eq!(
                    res, a,
                    "[LDCF Full Domain Eval] Fail at position {}, alpha: {:?}, expected {:?}, got {:?}",
                    x, ALPHA, a, res
                );
            } else {
                assert_eq!(
                    res, b,
                    "[LDCF Full Domain Eval] Fail at position {}, alpha: {:?}, expected {:?}, got {:?}",
                    x, ALPHA, b, res
                );
            }
        }
    }
    Ok(())
}

#[test]
fn ldcf_serialization() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");

    let (ldcf_key0, ldcf_key1) = LdcfKey::gen_ldcf_key(&alpha_bits, &a, &b, MODULUS)?;

    let ldcf_key0_bytes = ldcf_key0.to_bytes()?;
    let (ldcf_key0_cmp, size) = LdcfKey::from_bytes(&ldcf_key0_bytes, MODULUS)?;
    assert_eq!(ldcf_key0, ldcf_key0_cmp, "Wrong serialization for ldcf_key0!");
    assert_eq!(size, ldcf_key0_bytes.len(), "Wrong deserialization length for ldcf_key0!");

    let ldcf_key1_bytes = ldcf_key1.to_bytes()?;
    let (ldcf_key1_cmp, _) = LdcfKey::from_bytes(&ldcf_key1_bytes, MODULUS)?;
    assert_eq!(ldcf_key1, ldcf_key1_cmp, "Wrong serialization for ldcf_key1!");
    assert_eq!(size, ldcf_key1_bytes.len(), "Wrong deserialization length for ldcf_key1!");

    Ok(())
}
