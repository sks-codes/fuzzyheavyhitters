use mosaic::data_structures::mod2k::Mod2k;
use anyhow::Result;

#[test]
fn test_mul() -> Result<()> {
    let modulus = 1u128 << 20;
    let a = Mod2k::new(248583, modulus);
    let b = Mod2k::new(206530, modulus);
    let c = Mod2k::new(517454, modulus);

    let c_cmp = a * b;

    assert_eq!(c, c_cmp, "Multiplication test failed!");

    Ok(())
}