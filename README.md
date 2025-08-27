# MOSAIC

This is a Rust implementation of the Mosaic framework in the paper _Mosaic: A Modular Framework for Private Fuzzy Heavy Hitters_.

# How to use 

To compile, set the Rust flag:
```
$ export RUSTFLAGS+="-C target-cpu=native" 
$ cargo build --release
```

You should prepare four terminals and one config file. First, run server0: 
```
$ cargo run --release --bin fhh_cli server0 --config (path_to_config) --threads (num_threads) 
```

Then, run server1:
```
$ cargo run --release --bin fhh_cli server0 --config (path_to_config) --threads (num_threads) 
```

Now, the servers should be ready to process client requests. 

```
$ cargo run --release --bin fhh_cli client --config (path_to_config)
```

Wait until the client sent through everything, run the dealer:
```
$ cargo run --release --bin fhh_cli dealer --config (path_to_config) --threads (num_threads)
```

# How to set up data files:
Simply create a json file, for example, the following json file contains two client points:
[
  [
    XXX,
    XXX
  ],
  [
    XXX,
    XXX
  ]
]
    
