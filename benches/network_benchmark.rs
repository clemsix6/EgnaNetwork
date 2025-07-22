use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use egna_network::net::{message::Message, output::send, input::Listener};
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

fn bench_message_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_serialization");
    
    for size_kb in [1, 10, 50, 100, 500].iter() {
        let data = create_test_data(*size_kb);
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));
        
        group.bench_with_input(
            BenchmarkId::new("serialize", format!("{}kb", size_kb)),
            size_kb,
            |b, _| {
                b.iter(|| {
                    let message = Message::new(42, data.clone());
                    message.serialize()
                });
            },
        );
        
        let message = Message::new(42, data.clone());
        let serialized = message.serialize();
        group.bench_with_input(
            BenchmarkId::new("deserialize", format!("{}kb", size_kb)),
            size_kb,
            |b, _| {
                b.iter(|| {
                    Message::deserialize(&serialized).unwrap()
                });
            },
        );
    }
    
    group.finish();
}

fn bench_send_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("send_performance");
    
    for size_kb in [1, 10, 50, 100].iter() {
        let data = create_test_data(*size_kb);
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));
        
        group.bench_function(
            BenchmarkId::new("send_large_msg", format!("{}kb", size_kb)),
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

fn bench_receive_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("receive_performance");
    
    for size_kb in [1, 10, 50, 100].iter() {
        let data = create_test_data(*size_kb);
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));
        
        group.bench_function(
            BenchmarkId::new("full_message_cycle", format!("{}kb", size_kb)),
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
                        while received_messages.lock().unwrap().is_empty() && start.elapsed().as_secs() < 1 {
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

fn bench_throughput_sustained(c: &mut Criterion) {
    let mut group = c.benchmark_group("sustained_throughput");
    group.measurement_time(std::time::Duration::from_secs(5));
    
    for num_messages in [10, 50, 100].iter() {
        group.bench_function(
            BenchmarkId::new("messages_per_second", format!("{}_messages", num_messages)),
            |b| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                b.iter(|| {
                    rt.block_on(async {
                        let (sender, receiver, receiver_addr) = create_socket_pair().await;
                        let data = create_test_data(1);
                        
                        let received_count = Arc::new(Mutex::new(0));
                        let received_clone = received_count.clone();
                        
                        let mut listener = Listener::new(receiver);
                        listener.register_callback(42, move |_msg, _src| {
                            let received = received_clone.clone();
                            async move {
                                *received.lock().unwrap() += 1;
                            }
                        });
                        
                        let handle = listener.start().await;
                        
                        for _ in 0..*num_messages {
                            let message = Message::new(42, data.clone());
                            send(&sender, &receiver_addr, &message).await.unwrap();
                        }
                        
                        let start = std::time::Instant::now();
                        while (*received_count.lock().unwrap() as usize) < *num_messages && start.elapsed().as_secs() < 5 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                        }
                        
                        handle.abort();
                        assert_eq!(*received_count.lock().unwrap() as usize, *num_messages);
                    })
                });
            },
        );
    }
    
    group.finish();
}

fn bench_concurrent_sends(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_performance");
    
    for num_concurrent in [5, 10, 20].iter() {
        group.bench_function(
            BenchmarkId::new("concurrent_sends", format!("{}_concurrent", num_concurrent)),
            |b| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                b.iter(|| {
                    rt.block_on(async {
                        let (sender, _receiver, receiver_addr) = create_socket_pair().await;
                        let sender = Arc::new(sender);
                        let data = create_test_data(1);
                        
                        let mut handles = Vec::new();
                        for _ in 0..*num_concurrent {
                            let sender_clone = sender.clone();
                            let data_clone = data.clone();
                            let addr = receiver_addr;
                            let handle = tokio::spawn(async move {
                                let message = Message::new(42, data_clone);
                                send(&sender_clone, &addr, &message).await.unwrap();
                            });
                            handles.push(handle);
                        }
                        
                        for handle in handles {
                            handle.await.unwrap();
                        }
                    })
                });
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_message_serialization,
    bench_send_performance,
    bench_receive_performance,
    bench_throughput_sustained,
    bench_concurrent_sends
);
criterion_main!(benches);