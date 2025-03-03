use counttree::ibDCF::ibDCFKey;
use counttree::{bits_to_u32, u32_to_bits};

// #[test]
// fn ibdcf_complete() {
//     let nbits = 5;
//     let alpha = u32_to_bits(nbits, 21);
//     let (key0, key1) = ibDCFKey::gen(&alpha, true);
//
//     for i in 0..(1 << nbits) {
//         let alpha_eval = u32_to_bits(nbits, i);
//
//         println!("Alpha: {:?}, input: {:?}", alpha, alpha_eval);
//         for j in 0..((nbits-1) as usize) {
//             if j < 2 {
//                 continue;
//             }
//             let eval0 = key0.eval_ibDCF(&alpha_eval[0..j].to_vec());
//             let eval1 = key1.eval_ibDCF(&alpha_eval[0..j].to_vec());
//             let alpha_prefix = bits_to_u32(&alpha[0..j]);
//             let input_prefix = bits_to_u32(&alpha_eval[0..j]);
//             if alpha_prefix >= input_prefix {
//                 println!("1: alpha prefix - {:?}", alpha[0..j].to_vec());
//                 println!("1: input prefix - {:?}", alpha_eval[0..j].to_vec());
//                 assert_eq!(
//                     (eval0 ^ eval1) as u32,
//                     1,
//                     "[Level {:?}] Value incorrect at {:?}",
//                     j,
//                     alpha_eval
//                 );
//                 println!("1: passed");
//             } else {
//                 println!("2: alpha prefix - {:?}", alpha[0..j].to_vec());
//                 println!("2: input prefix - {:?}", alpha_eval[0..j].to_vec());
//                 assert_eq!((eval0 ^ eval1) as u32, 0);
//                 println!("2: passed");
//             }
//         }
//     }
// }