use anyhow::Result;
use mosaic::{
    data_structures::ringvec::RingVec,
    fss::dpf::DpfKey,
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

    let (dpf_key0, dpf_key1) = LdcfKey::gen_ldcf_key(&alpha_bits, &a, &b, MODULUS)?;

    let mut evals0: Vec<LdcfEval> = vec![dpf_key0.init_eval(MODULUS)?];
    let mut evals1: Vec<LdcfEval> = vec![dpf_key1.init_eval(MODULUS)?];

    for level in 1..BIT_LENGTH {
        let mut next_evals0 = Vec::with_capacity(evals0.len() * 2);
        for eval in evals0.iter() {
            let (left, right) = dpf_key0.expand_prefix(eval, MODULUS)?;
            next_evals0.push(left);
            next_evals0.push(right);
        }
        evals0 = next_evals0;
        let mut next_evals1 = Vec::with_capacity(evals1.len() * 2);
        for eval in evals1.iter() {
            let (left, right) = dpf_key1.expand_prefix(eval, MODULUS)?;
            next_evals1.push(left);
            next_evals1.push(right);
        }
        evals1 = next_evals1;
        let domain_size = 1usize << level;
        let prefix_alpha_bits: Vec<bool> = alpha_bits[..level].to_vec();
        let prefix_alpha = bits_to_u128_msb(&prefix_alpha_bits);
        for i in 0..domain_size {
            let res = evals0[i].y().clone() - evals1[i].y().clone();
            if i as u128 == prefix_alpha {
                assert_eq!(
                    res, a,
                    "[DPF] Fail at level {level}, position {i}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, a, res
                );
            } else {
                assert_eq!(
                    res, b,
                    "[DPF] Fail at level {level}, position {i}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, b, res
                );
            }
        }
    }
    Ok(())
}

#[test]
fn dpf_eval_dpf() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");

    let (dpf_key0, dpf_key1) = DpfKey::gen_dpf_key(&alpha_bits, &a, &b, MODULUS)?;

    for level in 1..BIT_LENGTH {
        let domain_size = 1usize << level;
        let prefix_alpha_bits: Vec<bool> = alpha_bits[..level].to_vec();
        let prefix_alpha = bits_to_u128_msb(&prefix_alpha_bits);
        for x in 0..domain_size {
            let x_bits = u128_to_bits_msb(x as u128, level);
            let res0 = dpf_key0.eval_dpf(&x_bits, MODULUS)?;
            let res1 = dpf_key1.eval_dpf(&x_bits, MODULUS)?;
            let res = res0 - res1;
            if x as u128 == prefix_alpha {
                assert_eq!(
                    res, a,
                    "[DPF] Fail at level {level}, position {x}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, a, res
                );
            } else {
                assert_eq!(
                    res, b,
                    "[DPF] Fail at level {level}, position {x}, prefix alpha: {:?}, expected {:?}, got {:?}",
                    prefix_alpha, b, res
                );
            }
        }
    }

    Ok(())
}

#[test]
fn dpf_serialization() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let a = RingVec::new(vec![1, 2], MODULUS).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], MODULUS).expect("Cannot create ringvec b");

    let (dpf_key0, dpf_key1) = DpfKey::gen_dpf_key(&alpha_bits, &a, &b, MODULUS)?;

    let dpf_key0_bytes = dpf_key0.to_bytes()?;
    let (dpf_key0_cmp, size) = DpfKey::from_bytes(&dpf_key0_bytes, MODULUS)?;
    assert_eq!(dpf_key0, dpf_key0_cmp, "Wrong serialization for dpf_key0!");
    assert_eq!(size, dpf_key0_bytes.len(), "Wrong deserialization length for dpf_key0!");

    let dpf_key1_bytes = dpf_key1.to_bytes()?;
    let (dpf_key1_cmp, _) = DpfKey::from_bytes(&dpf_key1_bytes, MODULUS)?;
    assert_eq!(dpf_key1, dpf_key1_cmp, "Wrong serialization for dpf_key1!");
    assert_eq!(size, dpf_key1_bytes.len(), "Wrong deserialization length for dpf_key1!");

    Ok(())
}
