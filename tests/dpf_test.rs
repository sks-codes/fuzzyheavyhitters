extern crate mosaic;
use anyhow::Result;
use mosaic::{
    data_structures::ringvec::RingVec,
    fss::dpf::{DpfEval, DpfKey},
    util::{bits_to_u128_msb, u128_to_bits_msb},
};

fn main() -> Result<()> {
    let alpha = 15u128;
    let bit_length = 5;
    let alpha_bits = u128_to_bits_msb(alpha, bit_length);
    let modulus = 1u128 << 20;
    let a = RingVec::new(vec![1, 2], modulus).expect("Cannot create ringvec a");
    let b = RingVec::new(vec![3, 4], modulus).expect("Cannot create ringvec b");

    let (dpf_key, _) = DpfKey::gen_dpf_key(&alpha_bits, &a, &b, modulus)?;

    let mut evals: Vec<DpfEval> = Vec::new();
    evals.push(dpf_key.init_eval(modulus)?);

    for level in 1..bit_length {
        let mut next_evals = Vec::with_capacity(evals.len() * 2);
        for eval in evals.iter() {
            let (left, right) = dpf_key.expand_prefix(eval, modulus)?;
            next_evals.push(left);
            next_evals.push(right);
        }
        evals = next_evals;
        let domain_size = 1usize << level;
        let prefix_alpha_bits = alpha_bits[..level];
        let prefix_alpha = bits_to_u128_msb(&prefix_alpha_bits);
        for i in 0..domain_size {
            let res = evals[i].result();
            if i as u128 != prefix_alpha {
                assert_eq!(
                    res, a,
                    "[DPF] Payload for prefix of alpha should be a, got {}",
                    res
                );
            } else {
                assert_eq!(
                    res, b,
                    "[DPF] Payload for non-prefix of alpha should be b, got {}",
                    res
                );
            }
        }
    }
    Ok(())
}
