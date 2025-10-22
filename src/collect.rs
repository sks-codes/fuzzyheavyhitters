use std::convert::{TryFrom, TryInto};
use std::io::{BufReader, BufWriter};
use crate::{all_bit_vectors, block_to_bits, data_structures::prg, Share};
use rayon::prelude::*;
use scuttlebutt::{AbstractChannel, AesRng, Block, SyncChannel};
use serde::{Deserialize, Serialize};
use crate::fss::ibdcf::{IbDCFKey, EvalState, eval_str};
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use ocelot::ot::{Receiver, Sender};
use crate::garbled_circuits::equality::{multiple_gb_equality_test, multiple_ev_equality_test};
use crate::data_structures::field::BlockPair;
use std::marker::PhantomData;
use std::net::TcpStream;
use std::time::Instant;
use crate::garbled_circuits::greater_than::{multiple_gb_greater_than, multiple_ev_greater_than, BitWidth};
use crate::data_structures::logexperiments::{ServerSide, TimeBreakdown};

#[derive(Clone)]
struct TreeNode {
    path: Vec<Vec<bool>>,
    key_states: Vec<Vec<(EvalState, EvalState)>>,
}

unsafe impl Send for TreeNode {}
unsafe impl Sync for TreeNode {}


#[derive(Clone)]
pub struct KeyCollection<T,U>
{
    _depth: usize,
    pub keys: Vec<(bool, Vec<(IbDCFKey, IbDCFKey)>)>,
    frontier: Vec<TreeNode>,
    frontier_last: Vec<Result<U>>,
    _rand_stream: prg::PrgStream,
    _phantom: PhantomData<(T, U)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result<T> {
    pub path: Vec<Vec<bool>>,
    pub value: T,
}

impl<T,U> KeyCollection<T,U>
where
    T: Share + Clone + std::fmt::Debug + PartialOrd + From<u32> + Send + Sync + TryFrom<Block> + Into<Block>,
    U: Share + Clone + std::fmt::Debug + PartialOrd + From<u32> + Send + Sync + TryFrom<BlockPair> + Into<BlockPair>,
    <U as TryFrom<BlockPair>>::Error: std::fmt::Debug,
{
    pub fn new(seed: &prg::PrgSeed, depth: usize) -> KeyCollection<T,U> {
        KeyCollection::<T,U> {
            _depth: depth,
            keys: vec![],
            frontier: vec![],
            frontier_last: vec![],
            _rand_stream: seed.to_rng(),
            _phantom: PhantomData,
        }
    }

    pub fn add_key(&mut self, key: Vec<(IbDCFKey, IbDCFKey)>) {
        self.keys.push((true, key)); //TODO: come back and remove this bool

    }

    pub fn tree_init(&mut self) {
        let mut root = TreeNode {
            path: vec![],
            // value: T::zero(),
            key_states: vec![],
            // key_values: vec![],
        };

        for k in &self.keys {
            let mut root_states = vec![];
            for interval_key in k.1.clone(){
                root_states.push((interval_key.0.eval_init(), interval_key.1.eval_init()));
            }
            root.key_states.push(root_states);
        }

        assert!(self.keys.len() > 0);
        for _ in 0..self.keys[0].1.len(){
            root.path.push(vec![]);
        }

        self.frontier.clear();
        self.frontier_last.clear();
        self.frontier.push(root);

    }

    fn make_tree_node(&self, parent: &TreeNode, search_string: &Vec<bool>) -> TreeNode {
        let key_states = self
            .keys
            .par_iter()
            .enumerate()
            .map(|(i, key)| {
                let ev = eval_str(&key.1, &parent.key_states[i], search_string);
                ev
            })
            .collect();

        let mut new_path = vec![];
        for (i, dim_path) in parent.path.iter().enumerate(){
            let mut new_dim_path = dim_path.clone();
            new_dim_path.push(search_string[i]);
            new_path.push(new_dim_path)
        }

        let child = TreeNode {
            path: new_path.clone(),
            key_states,
        };
        child
    }

    pub fn tree_crawl(
        &mut self,
        gc_sender: bool,
        channels: &mut [&mut SyncChannel<BufReader<TcpStream>, BufWriter<TcpStream>>],
        threshold: T
    ) -> (Vec<bool>, ServerSide) {
        println!("Crawl");
        let start = Instant::now();

        let next_frontier = self
            .frontier
            .par_iter()
            .map(|node| {
                let mut children = vec![];
                let search_strings = all_bit_vectors(node.key_states[0].len());
                for s in search_strings {
                    children.push(self.make_tree_node(node, &s));
                }
                children
            })
            .flatten()
            .collect::<Vec<TreeNode>>();

        let node_client_string: Vec<Vec<Vec<bool>>> = next_frontier
            .par_iter()
            .map(|node| {
                node.key_states
                    .par_iter()
                    .map(|state| {
                        let left_bits: Vec<bool> = state.iter()
                            .map(|(left, right)| left.y_bit ^ left.bit ^ right.y_bit ^ gc_sender)
                            .collect();
                        // let mut right_bits: Vec<bool> = state.iter()
                        //     .map(|(_, right)| right.y_bit ^ right.bit)
                        //     .collect();
                        // left_bits.append(&mut right_bits);
                        left_bits
                    })
                    .collect()
            })
            .collect();

        let fss = start.elapsed();
        println!("Tree searching and FSS - {:?}", fss);

        let all_client_strings: Vec<Vec<u16>> = node_client_string
            .iter()
            .flat_map(|node| node.iter().map(|client| client.iter().map(|&b| b as u16).collect::<Vec<u16>>()))
            .collect();
        let all_node_vals = crossbeam::scope(|s| {
            let mut results = vec![];
            let mut handles = vec![];

            let chunk_size = (all_client_strings.len() + channels.len() - 1) / channels.len();

            for (i, channel) in channels.iter().enumerate() {
                let start_idx = i * chunk_size;
                let end_idx = std::cmp::min(start_idx + chunk_size, all_client_strings.len());
                let chunk = all_client_strings[start_idx..end_idx].to_vec();

                handles.push(s.spawn(move |_| {
                    let mut rng = AesRng::new();
                    let mut channel = (*channel).clone();
                    let bin_shares = if gc_sender {
                        multiple_gb_equality_test(&mut rng, &mut channel, &chunk)
                    } else {
                        multiple_ev_equality_test(&mut rng, &mut channel, &chunk)
                    };
                    channel.flush().expect("flush failed");
                    let mut node_vals = vec![];
                    if gc_sender{
                        let mut all_shares = Vec::with_capacity(bin_shares.len());
                        for i in 0..bin_shares.len() {
                            let r0 = T::random();
                            let mut r1 = r0.clone();
                            r1.add(&T::one());
                            node_vals.push(r1.clone());
                            let r0_block: Block = r0.try_into().expect("Conversion failed");
                            let r1_block: Block = r1.try_into().expect("Conversion failed");
                            if bin_shares[i] {
                                all_shares.push((r0_block, r1_block));
                            } else {
                                all_shares.push((r1_block, r0_block));
                            }
                        }
                        let mut ot = OtSender::init(&mut channel, &mut rng).unwrap();
                        ot.send(&mut channel, all_shares.as_slice(), &mut rng).map_err(|_| {
                            println!("Error in tree_crawl ot send")
                        }).unwrap();
                    }
                    else{
                        let mut ot = OtReceiver::init(&mut channel, &mut rng).unwrap();
                        let out_blocks = ot.receive(&mut channel, bin_shares.as_slice(), &mut rng).unwrap();
                        node_vals = out_blocks.into_iter()
                            .map(|b| {
                                T::try_from(b)
                                    .map_err(|_| {
                                        // eprintln!("Conversion error: {:?}", e);  // Changed to {:?}
                                        // e
                                    })
                                    .unwrap()
                            })
                            .collect();
                    }
                    channel.flush().expect("flush failed");
                    node_vals
                }));
            }

            for handle in handles {
                results.extend(handle.join().unwrap());
            }

            results
        }).unwrap();

        let gc_and_ot = start.elapsed() - fss;
        println!("Equality Garbled Circuit and OT - {:?}", gc_and_ot);
        let results_by_node: Vec<T> = node_client_string
            .par_iter()
            .enumerate()
            .map(|(node_idx, node)| {
                let num_clients = node.len();
                let start_idx = node_idx * num_clients;
                let node_results = &all_node_vals[start_idx..start_idx + num_clients];

                let mut node_sum = T::zero();
                for (i, v) in node_results.iter().enumerate() {
                    if self.keys[i].0 {
                        node_sum.add_lazy(v);
                    }
                }
                if !gc_sender{
                    node_sum.add_lazy(&threshold);
                }
                node_sum
            })
            .collect();
        let fa = start.elapsed() - (gc_and_ot + fss);
        println!("Field actions - {:?}", fa);

        let final_res = crossbeam::scope(|s| {
            let mut results = vec![];
            let mut handles = vec![];

            let chunk_size = (results_by_node.len() + channels.len() - 1) / channels.len();

            for (i, channel) in channels.iter().enumerate() {
                let start_idx = i * chunk_size;
                if start_idx >= results_by_node.len() {
                    continue;
                }
                let end_idx = std::cmp::min(start_idx + chunk_size, results_by_node.len());
                let chunk = results_by_node[start_idx..end_idx].to_vec();
                let chunk_bits : Vec<Vec<u16>> = chunk
                    .iter()
                    .map(|b| {
                        let block : Block = b.clone().try_into().unwrap();
                        block_to_bits(block, true).iter().map(|b| *b as u16).collect()
                    })
                    .collect::<Vec<Vec<u16>>>();

                handles.push(s.spawn(move |_| {
                    let mut rng = AesRng::new();
                    let mut channel = (*channel).clone();
                    let final_shares = if gc_sender {
                        multiple_gb_greater_than(&mut rng, &mut channel, chunk_bits.as_slice(), BitWidth::Bits128);
                        vec![]
                    } else {
                        multiple_ev_greater_than(&mut rng, &mut channel, chunk_bits.as_slice(), BitWidth::Bits128)
                    };
                    final_shares
                }));
            }

            for handle in handles {
                results.extend(handle.join().unwrap());
            }
            results
        }).unwrap();

        let gc_comp_time = start.elapsed() - (gc_and_ot + fss + fa);
        println!("GEQ garbled circuit - {:?}", gc_comp_time);

        println!("...done");

        self.frontier = next_frontier;
        // results_by_node
        (final_res, ServerSide{
            total_level_time: start.elapsed().as_secs_f64(),
            time_breakdown: TimeBreakdown {
                fss: fss.as_secs_f64(),
                gc_equality: gc_and_ot.as_secs_f64(),
                field_actions: fa.as_secs_f64(),
                gc_compare: gc_comp_time.as_secs_f64(),
            },
            num_threads: channels.len(),
            nodes_searched: results_by_node.len(),
        })
    }

    pub fn tree_crawl_last(
        &mut self,
        gc_sender: bool,
        channels: &mut [&mut SyncChannel<BufReader<TcpStream>, BufWriter<TcpStream>>],
        threshold: U
    ) -> (Vec<bool>, ServerSide) {
        println!("Crawl");
        let start = Instant::now();

        let next_frontier = self
            .frontier
            .par_iter()
            .map(|node| {
                let mut children = vec![];
                let search_strings = all_bit_vectors(node.key_states[0].len());
                for s in search_strings {
                    children.push(self.make_tree_node(node, &s));
                }
                children
            })
            .flatten()
            .collect::<Vec<TreeNode>>();

        let node_client_string: Vec<Vec<Vec<bool>>> = next_frontier
            .par_iter()
            .map(|node| {
                node.key_states
                    .par_iter()
                    .map(|state| {
                        let mut left_bits: Vec<bool> = state.iter()
                            .map(|(left, _)| left.y_bit ^ left.bit)
                            .collect();
                        let mut right_bits: Vec<bool> = state.iter()
                            .map(|(_, right)| right.y_bit ^ right.bit)
                            .collect();
                        left_bits.append(&mut right_bits);
                        left_bits
                    })
                    .collect()
            })
            .collect();

        let fss = start.elapsed();
        println!("Tree searching and FSS - {:?}", fss);

        let all_client_strings: Vec<Vec<u16>> = node_client_string
            .iter()
            .flat_map(|node| node.iter().map(|client| client.iter().map(|&b| b as u16).collect::<Vec<u16>>()))
            .collect();
        let all_node_vals = crossbeam::scope(|s| {
            let mut results = vec![];
            let mut handles = vec![];

            let chunk_size = (all_client_strings.len() + channels.len() - 1) / channels.len();

            for (i, channel) in channels.iter().enumerate() {
                let start_idx = i * chunk_size;
                let end_idx = std::cmp::min(start_idx + chunk_size, all_client_strings.len());
                let chunk = all_client_strings[start_idx..end_idx].to_vec();

                handles.push(s.spawn(move |_| {
                    let mut rng = AesRng::new();
                    let mut channel = (*channel).clone();
                    let bin_shares = if gc_sender {
                        multiple_gb_equality_test(&mut rng, &mut channel, &chunk)
                    } else {
                        multiple_ev_equality_test(&mut rng, &mut channel, &chunk)
                    };
                    let mut node_vals = vec![];
                    if gc_sender{
                        let mut all_shares = Vec::with_capacity(bin_shares.len());
                        for i in 0..bin_shares.len() {
                            let r0 = U::random();
                            let mut r1 = r0.clone();
                            r1.add(&U::one());
                            node_vals.push(r1.clone());
                            let r0_block: BlockPair = r0.try_into().expect("Conversion failed");
                            let r1_block: BlockPair = r1.try_into().expect("Conversion failed");
                            if bin_shares[i] {
                                all_shares.push((r0_block.0[0], r1_block.0[0]));
                                all_shares.push((r0_block.0[1], r1_block.0[1]));
                            } else {
                                all_shares.push((r1_block.0[0], r0_block.0[0]));
                                all_shares.push((r1_block.0[1], r0_block.0[1]));
                            }
                        }
                        let mut ot = OtSender::init(&mut channel, &mut rng).unwrap();
                        ot.send(&mut channel, all_shares.as_slice(), &mut rng).map_err(|_| {
                            println!("Error in tree_crawl ot send")
                        }).unwrap();
                    }
                    else{
                        let mut ot = OtReceiver::init(&mut channel, &mut rng).unwrap();
                        let doubled_binary_shares = bin_shares.iter().flat_map(|&b| [b, b]).collect::<Vec<bool>>();
                        let out_blocks = ot.receive(&mut channel, doubled_binary_shares.as_slice(), &mut rng).unwrap();
                        let mut i = 0;
                        while i < out_blocks.len() - 1 {
                            let val = U::try_from(BlockPair([out_blocks[i], out_blocks[i+1]])).map_err(|_| {}).unwrap();
                            node_vals.push(val);
                            i += 2;
                        }
                    }
                    node_vals
                }));
            }

            for handle in handles {
                results.extend(handle.join().unwrap());
            }

            results
        }).unwrap();


        let gc_and_ot = start.elapsed() - fss;
        println!("Equality Garbled Circuit and OT - {:?}", gc_and_ot);
        let mut results_by_node = Vec::new();
        let mut current_idx = 0;
        for node in &node_client_string {
            let num_clients = node.len();
            let node_results : Vec<U> = all_node_vals[current_idx..current_idx + num_clients].to_vec();
            let mut node_sum = U::zero();
            for (i, v) in node_results.iter().enumerate() {
                if self.keys[i].0 {
                    node_sum.add_lazy(v);
                }
                if !gc_sender{
                    node_sum.add_lazy(&threshold);
                }
            }
            results_by_node.push(node_sum);
            current_idx += num_clients;
        }

        let fa = start.elapsed() - (gc_and_ot + fss);
        println!("Field actions - {:?}", fa);
        let final_res = crossbeam::scope(|s| {
            let mut results = vec![];
            let mut handles = vec![];

            let chunk_size = (results_by_node.len() + channels.len() - 1) / channels.len();

            for (i, channel) in channels.iter().enumerate() {
                let start_idx = i * chunk_size;
                if start_idx >= results_by_node.len() {
                    continue;
                }
                let end_idx = std::cmp::min(start_idx + chunk_size, results_by_node.len());
                let chunk = results_by_node[start_idx..end_idx].to_vec();
                let chunk_bits : Vec<Vec<u16>> = chunk
                    .iter()
                    .map(|b| {
                        let mut x = b.clone();
                        x.reduce();
                        let blocks: BlockPair = x.try_into().expect("Conversion failed");
                        let mut bits : Vec<u16> = block_to_bits(blocks.0[0], true).iter().map(|b| *b as u16).collect();
                        let second_half : Vec<u16>= block_to_bits(blocks.0[1], true).iter().map(|b| *b as u16).collect();
                        bits.extend(second_half);
                        bits
                    })
                    .collect::<Vec<Vec<u16>>>();

                handles.push(s.spawn(move |_| {
                    let mut rng = AesRng::new();
                    let mut channel = (*channel).clone();
                    let final_shares = if gc_sender {
                        multiple_gb_greater_than(&mut rng, &mut channel, chunk_bits.as_slice(), BitWidth::Bits256);
                        vec![]
                    } else {
                        multiple_ev_greater_than(&mut rng, &mut channel, chunk_bits.as_slice(), BitWidth::Bits256)
                    };
                    final_shares
                }));
            }

            for handle in handles {
                results.extend(handle.join().unwrap());
            }
            results
        }).unwrap();

        let gc_comp_time = start.elapsed() - (gc_and_ot + fss + fa);
        println!("GEQ garbled circuit - {:?}", gc_comp_time);
        println!("...done");
        self.frontier_last = next_frontier.par_iter().enumerate().map(|(i,node)| {
                Result::<U> {
                    path: node.path.clone(),
                    value: results_by_node[i].clone(),
                }
            }).collect::<Vec<Result<U>>>();

        (final_res, ServerSide{
            total_level_time: start.elapsed().as_secs_f64(),
            time_breakdown: TimeBreakdown {
                fss: fss.as_secs_f64(),
                gc_equality: gc_and_ot.as_secs_f64(),
                field_actions: fa.as_secs_f64(),
                gc_compare: gc_comp_time.as_secs_f64(),
            },
            num_threads: channels.len(),
            nodes_searched: results_by_node.len(),
        })
    }

    pub fn tree_prune(&mut self, alive_vals: &[bool]) {
        assert_eq!(alive_vals.len(), self.frontier.len());

        // Remove from back to front to preserve indices
        for i in (0..alive_vals.len()).rev() {
            if !alive_vals[i] {
                self.frontier.remove(i);
            }
        }
    }

    pub fn tree_prune_last(&mut self, alive_vals: &[bool]) {
        assert_eq!(alive_vals.len(), self.frontier_last.len());

        // Remove from back to front to preserve indices
        for i in (0..alive_vals.len()).rev() {
            if !alive_vals[i] {
                self.frontier_last.remove(i);
            }
        }
    }


    pub fn final_shares(&self) -> Vec<Result<U>> {
        let mut alive = vec![];
        for n in &self.frontier_last {
            alive.push(Result::<U> {
                path: n.path.clone(),
                value: n.value.clone()
            });

            println!("Final {:?}", n.path);
        }

        alive
    }

    pub fn final_values(res0: &[Result<U>], res1: &[Result<U>]) -> Vec<Result<U>> {
        assert_eq!(res0.len(), res1.len());

        let mut out = vec![];
        for i in 0..res0.len() {
            assert_eq!(res0[i].path, res1[i].path);

            let mut v = U::zero();
            let mut v0 = res0[i].value.clone();
            let mut v1 = res1[i].value.clone();
            v0.reduce();
            v1.reduce();
            v.add(&v0);
            v.sub(&v1);

            out.push(Result {
                path: res0[i].path.clone(),
                value: v,
            });
        }

        out
    }
}

