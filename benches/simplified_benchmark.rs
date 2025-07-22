use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use network::net::{message::Message, output::send, input::Listener};
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;

fn create_test_data(size_kb: usize) -> Vec<u8> {
    vec![0x42; size_kb * 1024]
}

async fn create_socket_pair() -> (UdpSocket, UdpSocket, std::net::SocketAddr) {
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let receiver_addr = receiver.local_addr().unwrap();
    (sender, receiver, receiver_addr)
}

fn bench_message_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_operations");
    
    for size_kb in [1, 10, 100].iter() {
        let data = create_test_data(*size_kb);
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));
        
        group.bench_function(
            BenchmarkId::new("serialize", format!("{}kb", size_kb)),
            |b| {
                b.iter(|| {
                    let message = Message::new(42, data.clone());
                    message.serialize()
                });
            },
        );
        
        let message = Message::new(42, data.clone());
        let serialized = message.serialize();
        group.bench_function(
            BenchmarkId::new("deserialize", format!("{}kb", size_kb)),
            |b| {
                b.iter(|| {
                    Message::deserialize(&serialized).unwrap()
                });
            },
        );
    }
    
    group.finish();
}

fn bench_network_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_roundtrip");
    group.sample_size(20);
    
    for size_kb in [1, 10, 50].iter() {
        let data = create_test_data(*size_kb);
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));
        
        group.bench_function(
            BenchmarkId::new("full_cycle", format!("{}kb", size_kb)),
            |b| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                b.iter(|| {
                    rt.block_on(async {
                        let (sender, receiver, receiver_addr) = create_socket_pair().await;
                        let message = Message::new(42, data.clone());
                        
                        let received_messages = Arc::new(Mutex::new(Vec::new()));
                        let received_clone = received_messages.clone();
                        
                        let mut listener = Listener::new(receiver);
                        listener.register_callback(42, move |msg, _src| {
                            let received = received_clone.clone();
                            async move {
                                received.lock().unwrap().push(msg);
                            }
                        });
                        
                        let handle = listener.start().await;
                        
                        send(&sender, &receiver_addr, &message).await.unwrap();
                        
                        let start = std::time::Instant::now();
                        while received_messages.lock().unwrap().is_empty() && start.elapsed().as_millis() < 500 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                        }
                        
                        handle.abort();
                        assert!(!received_messages.lock().unwrap().is_empty());
                    })
                });
            },
        );
    }
    
    group.finish();
}

fn bench_send_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("send_only");
    
    for size_kb in [1, 10, 100].iter() {
        let data = create_test_data(*size_kb);
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));
        
        group.bench_function(
            BenchmarkId::new("send_message", format!("{}kb", size_kb)),
            |b| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                b.iter(|| {
                    rt.block_on(async {
                        let (sender, _receiver, receiver_addr) = create_socket_pair().await;
                        let message = Message::new(42, data.clone());
                        send(&sender, &receiver_addr, &message).await.unwrap();
                    })
                });
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_message_operations,
    bench_send_only,
    bench_network_roundtrip
);
criterion_main!(benches);