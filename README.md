# Network

A high-performance Rust UDP networking library that provides reliable message transmission over unreliable UDP connections. The library implements chunked message transmission with automatic reassembly, making it ideal for applications that need to send large messages over UDP while maintaining reliability.

## Features

- **Reliable UDP Messaging**: Ensures message integrity with CRC32 validation and deduplication
- **Chunked Transmission**: Automatically splits large messages into optimal UDP packets to avoid fragmentation
- **Zero-Copy Deserialization**: Efficient message parsing without unnecessary memory allocations  
- **Async Message Handling**: Non-blocking callback system with tokio async/await support
- **Out-of-Order Reassembly**: Handles packet reordering and partial message reconstruction
- **Timeout-Based Cleanup**: Automatic cleanup of incomplete messages to prevent memory leaks
- **Multiple Destinations**: Support for registering callbacks for different message destinations

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
egna-network = "0.1.0"
tokio = { version = "1.46", features = ["full"] }
```

## Quick Start

```rust
use network::{Listener, Message, send};
use anyhow::Result;
use std::net::SocketAddr;
use tokio::{net::UdpSocket, time::{Duration, sleep}};

#[tokio::main]
async fn main() -> Result<()> {
    // Create a receiver
    let recv_socket = UdpSocket::bind("0.0.0.0:7000").await?;
    let recv_addr = recv_socket.local_addr()?;

    let mut listener = Listener::new(recv_socket);
    listener.register_callback(34, |message, src: SocketAddr| async move {
        println!(
            "Received message from {}: id={}, dest={}, data_len={}",
            src, message.id, message.dest, message.data.len()
        );
    });
    listener.start().await;

    // Give the listener time to start
    sleep(Duration::from_millis(100)).await;

    // Create a sender
    let send_socket = UdpSocket::bind("0.0.0.0:0").await?;
    let message = Message::new(34, b"Hello, Network!".to_vec());

    // Send the message
    send(&send_socket, &recv_addr, &message).await?;

    Ok(())
}
```

## Architecture

### Core Components

- **Message System**: Core message structure with ID, destination, CRC validation, and data payload
- **Chunking System**: Automatic message splitting into 1450-byte chunks for optimal network transmission
- **Listener**: Asynchronous UDP receiver with callback registration for different destinations
- **Sender**: Reliable message transmission with timeout handling

### Message Structure

Messages are automatically assigned random IDs for deduplication and include CRC32 checksums for integrity validation. Large messages are transparently chunked and reassembled.

### Performance Characteristics

- **Chunk Size**: 1450 bytes (optimal for most networks to avoid IP fragmentation)
- **Header Overhead**: 16 bytes per chunk (message ID, chunk index, total chunks, data length)
- **Send Timeout**: 200ms for reliable transmission attempts
- **Zero-copy**: View structs avoid memory allocation during deserialization

## API Reference

### Message

```rust
// Create a new message
let message = Message::new(destination, data);

// Access message properties
println!("ID: {}, Destination: {}, Data: {:?}", 
         message.id, message.dest, message.data);
```

### Listener

```rust
// Create and configure a listener
let mut listener = Listener::new(socket);

// Register callback for specific destination
listener.register_callback(dest_id, |message, src_addr| async move {
    // Handle received message
});

// Start listening
listener.start().await;
```

### Sender

```rust
// Send a message
send(&socket, &target_addr, &message).await?;
```

## Development

### Building

```bash
cargo build                    # Build the project
cargo test                     # Run all tests
cargo run --example demo       # Run the demo example
```

### Benchmarking

```bash
cargo bench --bench network_benchmark     # Run main network benchmarks
cargo bench --bench simplified_benchmark  # Run simplified benchmarks
```

## Testing

The library includes comprehensive unit tests for all modules and integration benchmarks for performance validation. All networking operations are tested with real UDP sockets to ensure proper behavior in production environments.
