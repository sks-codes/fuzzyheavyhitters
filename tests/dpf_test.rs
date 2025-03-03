use counttree::*;
use counttree::ibDCF::ibDCFKey;

// #[test]
// fn dcf_complete() {
//     let nbits = 5;
//     let alpha = MSB_u32_to_bits(nbits, 18);
//     let betas = vec![
//         FieldElm::from(7u32),
//         FieldElm::from(7u32),
//         FieldElm::from(7u32),
//         FieldElm::from(7u32),
//         // FieldElm::from(5u32)
//     ];
//     let beta_last = fastfield::FE::from(32u32);
//     let (key0, key1) = ibDCFKey::gen(&alpha, true, &betas, &beta_last);
//
//     // let mut leaves = vec![];
//     for i in 0..(1 << nbits) {
//         let alpha_eval = MSB_u32_to_bits(nbits, i);
//         let a = bits_to_u32(&alpha.clone());
//         let b = bits_to_u32(&alpha_eval.clone());
//         println!("Alpha: {:?} vs Eval number: {:?}",a.clone(), b.clone());
//         // let eval0 = key0.eval(&alpha_eval.to_vec());
//         // let eval1 = key1.eval(&alpha_eval.to_vec());
//         // leaves.push(eval0.2 ^ eval1.2);
//         // println!("Alpha: {:?}", alpha);
//         for j in 2..((nbits - 1) as usize) {
//             let a_prime = bits_to_u32(&alpha[0..j].to_vec().clone());
//             let b_prime = bits_to_u32(&alpha_eval[0..j].to_vec().clone());
//
//             let eval0 = key0.eval(&alpha_eval[0..j].to_vec());
//             let eval1 = key1.eval(&alpha_eval[0..j].to_vec());
//             let mut tmp = FieldElm::zero();
//
//             tmp.add(&eval0.0[j - 2]);
//             tmp.add(&eval1.0[j - 2]);
//             let semi_eval = &alpha_eval[0..j];
//             println!("[{:?}] Tmp {:?} = {:?}", semi_eval, j, tmp);
//             if a >= b {
//                 // println!("[Level {:?}] Value incorrect at {:?}", j, alpha_eval);
//                 assert_eq!((eval0.2 ^ eval1.2) as u32, 1);
//                 // assert_eq!(
//                 //     betas[j - 2],
//                 //     tmp,
//                 //     "[Level {:?}] Value incorrect at {:?}",
//                 //     j,
//                 //     alpha_eval
//                 // );
//             } else {
//                 // assert_eq!((eval0.2 ^ eval1.2) as u32, 0);
//                 // assert_eq!(FieldElm::zero(), tmp);
//             }
//         }
//     }
//     assert_eq!(0,1);
//     // println!("{:?}", leaves);
//
// }
#[test]
fn dcf_complete() {
    let nbits = 5;
    let alpha = u32_to_bits(nbits, 21);
    let betas = vec![
        FieldElm::from(1u32),
        FieldElm::from(1u32),
        FieldElm::from(1u32),
        FieldElm::from(1u32),
    ];
    let beta_last = fastfield::FE::from(1u32);
    let (key0, key1) = ibDCFKey::gen(&alpha, true, &betas, &beta_last);

    for i in 0..(1 << nbits) {
        let alpha_eval = u32_to_bits(nbits, i);

        println!("Alpha: {:?}", alpha);
        for j in 0..((nbits-1) as usize) {
            if j < 2 {
                continue;
            }

            let eval0 = key0.eval(&alpha_eval[0..j].to_vec());
            let eval1 = key1.eval(&alpha_eval[0..j].to_vec());
            let mut tmp = FieldElm::zero();

            tmp.add(&eval0.0[j - 2]);
            tmp.add(&eval1.0[j - 2]);
            println!("[{:?}] Tmp {:?} = {:?}", alpha_eval, j, tmp);
            if bits_to_u32(&alpha[0..j-1]) >= bits_to_u32(&alpha_eval[0..j-1]) {
                assert_eq!((eval0.2 ^ eval1.2) as u32, 1);
                assert_eq!(
                    betas[j - 2],
                    tmp,
                    "[Level {:?}] Value incorrect at {:?}",
                    j,
                    alpha_eval
                );
            } else {
                assert_eq!((eval0.2 ^ eval1.2) as u32, 0);
                assert_eq!(FieldElm::zero(), tmp);
            }
        }
    }
    // assert_eq!(0, 1);
}
