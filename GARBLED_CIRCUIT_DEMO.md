# Garbled Circuit Socket Demo

This binary demonstrates the use of garbled circuits for computing `(x + y) mod modulus >= t` using socket-based communication between a garbler and evaluator.

## Usage

The binary can be run in two modes:

### Garbler (Server)
```bash
./target/release/garbled_circuit_socket_demo garbler [port]
```

### Evaluator (Client)
```bash
./target/release/garbled_circuit_socket_demo evaluator [port]
```

The default port is 8080 if not specified.

## Communication Tracking

The implementation includes a custom channel wrapper (`CommTrackingChannel`) that tracks:
- Bytes sent
- Bytes received
- Total communication cost
- Execution time

## Test Cases

The demo runs with the following predefined test cases using modulus 8:
- Case 0: (3+2) mod 8 = 5 >= 5 = true
- Case 1: (1+4) mod 8 = 5 >= 3 = true  
- Case 2: (2+1) mod 8 = 3 >= 7 = false
- Case 3: (0+6) mod 8 = 6 >= 2 = true
- Case 4: (7+3) mod 8 = 2 >= 1 = true

## Running the Demo

1. Start the garbler in one terminal:
```bash
RUSTFLAGS="-C target-cpu=native" cargo run --release --bin garbled_circuit_socket_demo -- garbler 8080
```

2. Start the evaluator in another terminal:
```bash
RUSTFLAGS="-C target-cpu=native" cargo run --release --bin garbled_circuit_socket_demo -- evaluator 8080
```

The results from both parties need to be XORed together to get the final answer.

## Implementation Notes

- Uses `scuttlebutt::SyncChannel` for the underlying communication
- Wraps it with `CommTrackingChannel` to measure communication costs
- Supports the `Clone` trait required by the garbled circuit functions
- Automatically tracks bytes sent/received during the protocol execution
- Uses TCP sockets for network communication
