use counttree::*;
use counttree::ibDCF::ibDCFKey;

// #[test]
// fn dcf_complete() {
//     let nbits = 5;
//     let alpha = u32_to_bits(nbits, 21);
//     // let betas = vec![
//     //     FieldElm::from(1u32),
//     //     FieldElm::from(1u32),
//     //     FieldElm::from(1u32),
//     //     FieldElm::from(1u32),
//     // ];
//     // let beta_last = fastfield::FE::from(1u32);
//     let (key0, key1) = ibDCFKey::gen_ibDCF(&alpha, true);
//
//     for i in 0..(1 << nbits) {
//         let alpha_eval = u32_to_bits(nbits, i);
//
//         let eval0 = key0.eval_ibDCF(&alpha_eval);
//         let eval1 = key1.eval_ibDCF(&alpha_eval);
//
//         let mut tmp = FieldElm::zero();
//         tmp.add(&eval0.0[2]);
//         tmp.add(&eval1.0[2]);
//
//         println!("EvalStr: {:?}, Result: {:?}", i, tmp);
//     }
//     // assert_eq!(0, 1);
// }

// #[test]
// fn dcf_complete() {
//     let nbits = 5;
//     let alpha = u32_to_bits(nbits, 21);
//     let betas = vec![
//         FieldElm::from(1u32),
//         FieldElm::from(1u32),
//         FieldElm::from(1u32),
//         FieldElm::from(1u32),
//     ];
//     let beta_last = fastfield::FE::from(1u32);
//     let (key0, key1) = ibDCFKey::gen(&alpha, true, &betas, &beta_last);
//
//     for i in 0..(1 << nbits) {
//         let alpha_eval = u32_to_bits(nbits, i);
//
//         println!("Alpha: {:?}", alpha);
//         for j in 0..((nbits-2) as usize) {
//             if j < 2 {
//                 continue;
//             }
//
//             let eval0 = key0.eval(&alpha_eval[0..j].to_vec());
//             let eval1 = key1.eval(&alpha_eval[0..j].to_vec());
//             let mut tmp = FieldElm::zero();
//
//             tmp.add(&eval0.0[j - 2]);
//             tmp.add(&eval1.0[j - 2]);
//             println!("[{:?}] Tmp {:?} = {:?}", alpha_eval, j, tmp);
//             if bits_to_u32(&alpha[0..j-1]) >= bits_to_u32(&alpha_eval[0..j-1]) {
//                 assert_eq!((eval0.2 ^ eval1.2) as u32, 1);
//                 assert_eq!(
//                     betas[j - 2],
//                     tmp,
//                     "[Level {:?}] Value incorrect at {:?}",
//                     j,
//                     alpha_eval
//                 );
//             } else {
//                 assert_eq!((eval0.2 ^ eval1.2) as u32, 0);
//                 assert_eq!(FieldElm::zero(), tmp);
//             }
//         }
//     }
//     assert_eq!(0, 1);
// }
