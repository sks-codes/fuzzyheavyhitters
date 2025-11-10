use uint::construct_uint;

// makes a U256 type
construct_uint! {
    pub struct U256(4);
}

pub fn mul_mod(a: u128, b: u128, m: u128) -> u128 {
    let a256 = U256::from(a);
    let b256 = U256::from(b);
    let m256 = U256::from(m);

    let r = (a256 * b256) % m256;
    r.as_u128()
}