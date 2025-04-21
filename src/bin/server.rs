// Starter code from:
//   https://github.com/google/tarpc/blob/master/example-service/src/server.rs

use counttree::{
    collect, config,
    FieldElm,
    fastfield::FE, prg,
    rpc::Collector,
    rpc::{
        AddKeysRequest, FinalSharesRequest, ResetRequest, TreeCrawlRequest, TreeInitRequest,
        TreePruneRequest,
        TreePruneLastRequest,
    },
};

use futures::{
    future::{self, Ready},
    prelude::*,
};
use std::{
    io,
    sync::{Arc, Mutex},
};
use std::convert::TryFrom;
use std::io::{BufReader, BufWriter};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread::available_parallelism;
use std::time::Duration;
use tarpc::{
    context,
    server::{self, Channel},
    tokio_serde::formats::Bincode,
    serde_transport::tcp,
};
use counttree::rpc::TreeCrawlLastRequest;

extern crate num_cpus;
type MyChannel = scuttlebutt::SyncChannel<BufReader<UnixStream>, BufWriter<UnixStream>>;


#[derive(Clone)]
struct CollectorServer {
    seed: prg::PrgSeed,
    data_len: usize,
    server_idx: u16,
    arc: Arc<Mutex<collect::KeyCollection<FE, FieldElm>>>,
    // gc_channel: Option<Arc<Mutex<MyChannel>>>
    gc_channels: Vec<Arc<Mutex<MyChannel>>>
}

impl Collector for CollectorServer {
    type AddKeysFut = Ready<String>;
    type TreeInitFut = Ready<String>;
    type TreeCrawlFut = Ready<Vec<FE>>;
    type TreeCrawlLastFut = Ready<Vec<FieldElm>>;
    type TreePruneFut = Ready<String>;
    type TreePruneLastFut = Ready<String>;
    type FinalSharesFut = Ready<Vec<collect::Result<FieldElm>>>;
    type ResetFut = Ready<String>;

    fn reset(self, _: context::Context, _rst: ResetRequest) -> Self::ResetFut {
        let mut coll = self.arc.lock().unwrap();
        *coll = collect::KeyCollection::new(&self.seed, self.data_len);

        future::ready("Done".to_string())
    }

    fn add_keys(self, _: context::Context, add: AddKeysRequest) -> Self::AddKeysFut {
        let mut coll = self.arc.lock().unwrap();
        for k in add.keys {
            coll.add_key(k);
        }
        future::ready("".to_string())
    }

    fn tree_init(self, _: context::Context, _req: TreeInitRequest) -> Self::TreeInitFut {
        let mut coll = self.arc.lock().unwrap();
        coll.tree_init();
        future::ready("Done".to_string())
    }

    // fn tree_crawl(self, _: context::Context, _req: TreeCrawlRequest) -> Self::TreeCrawlFut {
    //
    //     let mut coll = self.arc.lock().unwrap();
    //     let results = if let Some(gc_chan) = &self.gc_channel {
    //         let mut channel = gc_chan.lock().unwrap();
    //         coll.tree_crawl(_req.gc_sender, Some(&mut *channel))
    //     } else {
    //         coll.tree_crawl(_req.gc_sender, None)
    //     };
    //     future::ready(results)
    // }
    fn tree_crawl(
        self,
        _: context::Context,
        req: TreeCrawlRequest
    ) -> Self::TreeCrawlFut {
        let mut coll = self.arc.lock().unwrap();

        // Lock all channels
        let mut locked_channels: Vec<_> = self.gc_channels
            .iter()
            .map(|c| c.lock().unwrap())
            .collect();

        // Get mutable references to inner channels
        let mut channel_refs: Vec<&mut MyChannel> = locked_channels
            .iter_mut()
            .map(|guard| &mut **guard)
            .collect();

        let results = coll.tree_crawl(req.gc_sender, &mut channel_refs[..]);

        future::ready(results)
    }

    // fn tree_crawl_last(self, _: context::Context, _req: TreeCrawlLastRequest) -> Self::TreeCrawlLastFut {
    //
    //     let mut coll = self.arc.lock().unwrap();
    //     let results = if let Some(gc_chan) = &self.gc_channels[0] {
    //         let mut channel = gc_chan.lock().unwrap();
    //         coll.tree_crawl_last(_req.gc_sender, Some(&mut *channel))
    //     } else {
    //         coll.tree_crawl_last(_req.gc_sender, None)
    //     };
    //     future::ready(results)
    // }
    fn tree_crawl_last(
        self,
        _: context::Context,
        req: TreeCrawlLastRequest
    ) -> Self::TreeCrawlLastFut {
        let mut coll = self.arc.lock().unwrap();

        // Lock all channels
        let mut locked_channels: Vec<_> = self.gc_channels
            .iter()
            .map(|c| c.lock().unwrap())
            .collect();

        // Get mutable references to inner channels
        let mut channel_refs: Vec<&mut MyChannel> = locked_channels
            .iter_mut()
            .map(|guard| &mut **guard)
            .collect();

        let results = coll.tree_crawl_last(req.gc_sender, &mut channel_refs[..]);

        future::ready(results)
    }

    fn tree_prune(self, _: context::Context, req: TreePruneRequest) -> Self::TreePruneFut {
        let mut coll = self.arc.lock().unwrap();
        coll.tree_prune(&req.keep);
        future::ready("Done".to_string())
    }

    fn tree_prune_last(self, _: context::Context, req: TreePruneLastRequest) -> Self::TreePruneLastFut {
        let mut coll = self.arc.lock().unwrap();
        coll.tree_prune_last(&req.keep);
        future::ready("Done".to_string())
    }

    fn final_shares(self, _: context::Context, _req: FinalSharesRequest) -> Self::FinalSharesFut {
        let coll = self.arc.lock().unwrap();
        let out = coll.final_shares();
        future::ready(out)
    }
}

fn setup_unix_socket(server_idx: u16) -> io::Result<MyChannel> {
    const SOCKET_PATH: &str = "/tmp/gc-server-socket";

    if server_idx == 0 {
        for _ in 0..20 {
            match UnixStream::connect(SOCKET_PATH) {
                Ok(stream) => {
                    return Ok(scuttlebutt::SyncChannel::new(
                        BufReader::new(stream.try_clone()?),
                        BufWriter::new(stream),
                    ));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(500)),
            }
        }
        Err(io::Error::new(io::ErrorKind::ConnectionRefused, "Failed to connect after 5 attempts"))
    } else {
        // Server 1 (Evaluator) - Bind and listen
        let _ = std::fs::remove_file(SOCKET_PATH);
        let listener = UnixListener::bind(SOCKET_PATH)?;
        let (stream, _) = listener.accept()?;
        Ok(scuttlebutt::SyncChannel::new(
            BufReader::new(stream.try_clone()?),
            BufWriter::new(stream),
        ))
    }
}

fn setup_unix_sockets(server_idx: u16, num_cpus: usize) -> io::Result<Vec<Arc<Mutex<MyChannel>>>> {

    let mut channels = Vec::with_capacity(num_cpus);

    for i in 0..num_cpus {
        let socket_path = format!("/tmp/gc-server-socket-{}", i);

        let channel_result = if server_idx == 0 {
            // Garbler (client) side - with retries
            connect_with_retries(&socket_path)
        } else {
            // Evaluator (server) side
            create_server_socket(&socket_path)
        };

        // Handle the Result here before pushing to vector
        let channel = channel_result?; // This will return early if error occurs
        channels.push(Arc::new(Mutex::new(channel)));
    }

    Ok(channels)
}

// Helper function for client connection with retries
fn connect_with_retries(socket_path: &str) -> io::Result<MyChannel> {
    let mut retries = 0;
    let mut last_error = None;

    loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => {
                return Ok(scuttlebutt::SyncChannel::new(
                    BufReader::new(stream.try_clone()?),
                    BufWriter::new(stream),
                ));
            }
            Err(e) => {
                last_error = Some(e);
                if retries >= 10 {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionRefused,
                        format!("Failed to connect after {} retries: {:?}",
                                10, last_error)
                    ));
                }
                retries += 1;
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

// Helper function for server socket creation
fn create_server_socket(socket_path: &str) -> io::Result<MyChannel> {
    // Clean up any existing socket file
    let _ = std::fs::remove_file(socket_path);

    // Create parent directory if needed
    if let Some(parent) = Path::new(socket_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(socket_path)?;
    let (stream, _) = listener.accept()?;

    Ok(scuttlebutt::SyncChannel::new(
        BufReader::new(stream.try_clone()?),
        BufWriter::new(stream),
    ))
}


#[tokio::main]
async fn main() -> io::Result<()> {
    env_logger::init();

    let (cfg, sid, _) = config::get_args("Server", true, false);
    let server_addr = match sid {
        0 => cfg.server0,
        1 => cfg.server1,
        _ => panic!("Oh no!"),
    };

    let server_idx = match sid {
        0 => 0,
        1 => 1,
        _ => panic!("Oh no!"),
    };

    // XXX This is bogus
    let seed = prg::PrgSeed { key: [1u8; 16] };

    let coll = collect::KeyCollection::new(&seed, cfg.data_len);
    let arc = Arc::new(Mutex::new(coll));

    // let gc_channel = match setup_unix_socket(server_idx) {
    //     Ok(channel) => Some(Arc::new(Mutex::new(channel))),
    //     Err(e) => {
    //         eprintln!("Warning: Failed to setup GC channel: {}", e);
    //         None
    //     }
    // };
    let num_cpus = available_parallelism().unwrap().get();


    let gc_channels = setup_unix_sockets(server_idx, num_cpus).unwrap_or_else(|e| {
        eprintln!("Warning: Failed to setup GC channels: {}", e);
        vec![] // Fallback to no channels
    });

    let mut server_addr = server_addr;
    // Listen on any IP
    server_addr.set_ip("0.0.0.0".parse().expect("Could not parse"));
    tcp::listen(&server_addr, Bincode::default)
        .await?
        .filter_map(|r| future::ready(r.ok()))
        .map(server::BaseChannel::with_defaults)
        .map(|channel| {
            let coll_server = CollectorServer {
                server_idx,
                seed: seed.clone(),
                data_len: cfg.data_len,
                arc: arc.clone(),
                gc_channels: gc_channels.clone(),
            };

            channel.execute(coll_server.serve())
        })
        .buffer_unordered(100)
        .for_each(|_| async {})
        .await;

    Ok(())
}
