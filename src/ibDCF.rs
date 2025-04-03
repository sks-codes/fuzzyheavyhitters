use crate::{add_bitstrings, prg, subtract_bitstrings};
use crate::Group;

use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CorWord {
    pub seed: prg::PrgSeed,
    pub bits: (bool, bool),
    pub y_bits: (bool, bool),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ibDCFKey {
    pub key_idx: bool,
    pub root_seed: prg::PrgSeed,
    pub cor_words: Vec<CorWord>,
}


#[derive(Clone)]
pub struct EvalState {
    level: usize,
    seed: prg::PrgSeed,
    pub bit: bool,
    pub y_bit: bool
}

trait TupleMapToExt<T, U> {
    type Output;
    fn map<F: FnMut(&T) -> U>(&self, f: F) -> Self::Output;
}

type TupleMutIter<'a, T> =
std::iter::Chain<std::iter::Once<(bool, &'a mut T)>, std::iter::Once<(bool, &'a mut T)>>;

trait TupleExt<T> {
    fn map_mut<F: Fn(&mut T)>(&mut self, f: F);
    fn get(&self, val: bool) -> &T;
    fn get_mut(&mut self, val: bool) -> &mut T;
    fn iter_mut(&mut self) -> TupleMutIter<T>;
}

impl<T, U> TupleMapToExt<T, U> for (T, T) {
    type Output = (U, U);

    #[inline(always)]
    fn map<F: FnMut(&T) -> U>(&self, mut f: F) -> Self::Output {
        (f(&self.0), f(&self.1))
    }
}

impl<T> TupleExt<T> for (T, T) {
    #[inline(always)]
    fn map_mut<F: Fn(&mut T)>(&mut self, f: F) {
        f(&mut self.0);
        f(&mut self.1);
    }

    #[inline(always)]
    fn get(&self, val: bool) -> &T {
        match val {
            false => &self.0,
            true => &self.1,
        }
    }

    #[inline(always)]
    fn get_mut(&mut self, val: bool) -> &mut T {
        match val {
            false => &mut self.0,
            true => &mut self.1,
        }
    }

    fn iter_mut(&mut self) -> TupleMutIter<T> {
        std::iter::once((false, &mut self.0)).chain(std::iter::once((true, &mut self.1)))
    }
}

fn gen_cor_word(bit: bool, side : bool, bits: &mut (bool, bool), seeds: &mut (prg::PrgSeed, prg::PrgSeed)) -> CorWord
{
    let data = seeds.map(|s| s.expand());
    let keep = bit;
    let lose = !keep;

    let mut cw = CorWord {
        seed: data.0.seeds.get(lose) ^ data.1.seeds.get(lose),
        bits: (
            data.0.bits.0 ^ data.1.bits.0 ^ bit ^ true,
            data.0.bits.1 ^ data.1.bits.1 ^ bit,
        ),
        y_bits: (
            data.0.y_bits.0 ^ data.1.y_bits.0 ^ (bit & !side),
            data.0.y_bits.1 ^ data.1.y_bits.1 ^ (!bit & side)
        ),
        // word: W::zero(),
    };

    for (b, seed) in seeds.iter_mut() {
        *seed = data.get(b).seeds.get(keep).clone();

        if *bits.get(b) {
            *seed = &*seed ^ &cw.seed;
        }

        let mut newbit = *data.get(b).bits.get(keep);
        if *bits.get(b) {
            newbit ^= cw.bits.get(keep);
        }

        *bits.get_mut(b) = newbit;
    }

    cw
}
pub fn eval_str(keys : &Vec<(ibDCFKey, ibDCFKey)>, states: &Vec<(EvalState,EvalState)>, eval_string: &Vec<bool>) -> Vec<(EvalState,EvalState)> {
    let dim = keys.len();
    let mut new_states = Vec::with_capacity(dim);

    for (i, &(ref state_left, ref state_right)) in states.iter().enumerate() {
        let (left_key, right_key) = &keys[i];
        let new_state_left = left_key.eval_bit(state_left, eval_string[i]);
        let new_state_right = right_key.eval_bit(state_right, eval_string[i]);
        new_states.push((new_state_left, new_state_right));
    }
    new_states
}


/// All-prefix DPF implementation.
impl ibDCFKey
{

    pub fn gen_ibDCF(alpha_bits: &[bool], side : bool) -> (ibDCFKey, ibDCFKey) {
        let root_seeds = (prg::PrgSeed::random(), prg::PrgSeed::random());
        let root_bits = (false, true);

        let mut seeds = root_seeds.clone();
        let mut bits = root_bits;

        let mut cor_words: Vec<CorWord> = Vec::new();

        for (_, &bit) in alpha_bits.iter().enumerate() {
            let cw = gen_cor_word(bit, side, &mut bits, &mut seeds);
            cor_words.push(cw);
        }

        (
            ibDCFKey {
                key_idx: false,
                root_seed: root_seeds.0,
                cor_words: cor_words.clone(),
            },
            ibDCFKey {
                key_idx: true,
                root_seed: root_seeds.1,
                cor_words,
            },
        )
    }

    pub fn gen_interval(left_bits: &[bool], right_bits: &[bool]) -> ((ibDCFKey, ibDCFKey), (ibDCFKey, ibDCFKey)){
        // let r = &[false; 512];
        let l_minus_one = left_bits.to_vec();//subtract_bitstrings(left_bits, &[true]);
        let r_plus_one = right_bits.to_vec(); //add_bitstrings(right_bits, &[true]);
        let left_key = Self::gen_ibDCF(l_minus_one.as_slice(), true);
        let right_key = Self::gen_ibDCF(r_plus_one.as_slice(), false);
        ((left_key.0, right_key.0), (left_key.1, right_key.1))
    }

    pub fn gen_l_inf_ball(alpha : &[bool], d : usize) -> (Vec<(ibDCFKey, ibDCFKey)>, Vec<(ibDCFKey, ibDCFKey)>){
        let mut s0_keys = vec![];
        let mut s1_keys = vec![];
        for _ in 0..d {
            let (k0, k1) = Self::gen_interval(alpha, alpha);
            s0_keys.push(k0);
            s1_keys.push(k1);
        }
        (s0_keys, s1_keys)
    }


    pub fn eval_bit(&self, state: &EvalState, dir: bool) -> EvalState {
        let tau = state.seed.expand_dir(!dir, dir);
        let mut seed = tau.seeds.get(dir).clone();
        let mut new_bit = *tau.bits.get(dir);
        let mut new_y_bit = *tau.y_bits.get(dir);

        if state.bit {
            seed = &seed ^ &self.cor_words[state.level].seed;
            new_bit ^= self.cor_words[state.level].bits.get(dir);
            new_y_bit ^= self.cor_words[state.level].y_bits.get(dir);
        }
        new_y_bit ^= state.y_bit;

        EvalState {
            level: state.level + 1,
            seed,
            bit: new_bit,
            y_bit: new_y_bit,
        }
    }

    pub fn eval_init(&self) -> EvalState {
        EvalState {
            level: 0,
            seed: self.root_seed.clone(),
            bit: self.key_idx,
            y_bit: self.key_idx
        }
    }

    pub fn eval_ibDCF(&self, idx: &[bool]) -> bool {
        debug_assert!(idx.len() <= self.domain_size());
        debug_assert!(!idx.is_empty());
        let mut state = self.eval_init();

        for i in 0..idx.len() {
            let bit = idx[i];
            let state_new = self.eval_bit(&state, bit);
            state = state_new;
        }

        state.y_bit ^ state.bit
    }

    pub fn domain_size(&self) -> usize {
        self.cor_words.len()
    }
}