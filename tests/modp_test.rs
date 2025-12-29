use mosaic::data_structures::modp::{BarrettCtx, Modp};
use anyhow::Result;

#[test]
fn test_add() -> Result<()> {
    let barrett_ctx = BarrettCtx::new(23041535061816907443317u128);
    let a = Modp::new(&barrett_ctx, 10743844264059459074950u128);
    let b = Modp::new(&barrett_ctx, 16342564127917503030028u128);
    let c = Modp::new(&barrett_ctx, 4044873330160054661661u128);

    let c_cmp = a + b;

    assert_eq!(c, c_cmp, "Wrong addition, expected {:?}, got {:?}", c, c_cmp);

    Ok(())
}

#[test]
fn test_mul() -> Result<()> {
    let barrett_ctx = BarrettCtx::new(243424607205225985286245u128);
    let a = Modp::new(&barrett_ctx, 1159421699231684420447714u128);
    let b = Modp::new(&barrett_ctx, 477267816795244968367790u128);
    let c = Modp::new(&barrett_ctx, 223472312121238935535775u128);

    let c_cmp = a * b;

    assert_eq!(c, c_cmp, "Wrong multiplication, expected {:?}, got {:?}", c, c_cmp);

    Ok(())
}

#[test]
fn test_inv() -> Result<()> {
    let barrett_ctx = BarrettCtx::new(23041535061816907443317u128);
    let a = Modp::new(&barrett_ctx, 10743844264059459074950u128);
    let a_inv = a.inv().unwrap();
    let b = a * a_inv;

    assert_eq!(b, Modp::one(&barrett_ctx), "Wrong inv");

    Ok(())
}