use crate::net::chunk::{Chunk, ChunkView};
use crate::net::consts::HEADER_SIZE;
use crate::net::message::Message;
use bytes::BytesMut;
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::io::Error;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::SeqCst;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio::time::Instant;

type Callback =
    Arc<dyn Fn(Message, SocketAddr) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

struct ReceivingMessage {
    parts: Vec<Option<BytesMut>>,
    received_chunks: Vec<bool>,
    last_seen: Instant,
    total_expected: u16,
}

pub struct Listener {
    pub socket: UdpSocket,
    running: AtomicBool,
    callbacks: HashMap<u16, Callback>,
}

impl Listener {
    pub fn new(socket: UdpSocket) -> Self {
        Listener {
            socket,
            running: AtomicBool::new(true),
            callbacks: HashMap::new(),
        }
    }

    pub async fn start(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            if let Err(e) = self.internal_start().await {
                eprintln!("Listener error: {e}");
            }
        })
    }

    async fn process_chunk(
        &self,
        chunk: &ChunkView<'_>,
        messages: &mut HashMap<u64, ReceivingMessage>,
    ) -> Option<Vec<u8>> {
        let entry = messages
            .entry(chunk.msg_id)
            .or_insert_with(|| ReceivingMessage {
                parts: vec![None; chunk.total as usize],
                received_chunks: vec![false; chunk.total as usize],
                last_seen: Instant::now(),
                total_expected: chunk.total,
            });

        if chunk.total != entry.total_expected {
            messages.remove(&chunk.msg_id);
            return None;
        }

        if !entry.received_chunks[chunk.idx as usize] {
            entry.parts[chunk.idx as usize] = Some(BytesMut::from(chunk.data));
            entry.received_chunks[chunk.idx as usize] = true;
            entry.last_seen = Instant::now();
        }

        if entry.received_chunks.iter().all(|&received| received) {
            Some(Self::assemble_complete_message(entry))
        } else {
            None
        }
    }

    fn assemble_complete_message(entry: &mut ReceivingMessage) -> Vec<u8> {
        let mut full = Vec::new();
        for part_data in entry.parts.drain(..).flatten() {
            full.extend_from_slice(&part_data);
        }
        full
    }

    fn cleanup_old_messages(messages: &mut HashMap<u64, ReceivingMessage>) {
        messages.retain(|_, e| e.last_seen.elapsed() < Duration::from_secs(5));
    }

    async fn handle_complete_message(&self, message: Message, src: SocketAddr) -> Option<Error> {
        if let Some(cb) = self.callbacks.get(&message.dest) {
            let cb = cb.clone();
            tokio::spawn(async move {
                cb(message, src).await;
            });
            None
        } else {
            Some(Error::new(
                io::ErrorKind::NotFound,
                format!("No callback registered for destination {}", &message.dest),
            ))
        }
    }

    async fn internal_start(self) -> io::Result<()> {
        let mut buf = vec![0u8; 1500];
        let mut messages: HashMap<u64, ReceivingMessage> = HashMap::new();

        while self.running.load(SeqCst) {
            // Receive data from the socket
            let (len, src) = self.socket.recv_from(&mut buf).await?;

            if len < HEADER_SIZE {
                continue;
            }
            let chunk = Chunk::deserialize(&buf).expect("Failed to deserialize packet header");

            if let Some(bytes) = self.process_chunk(&chunk, &mut messages).await {
                let message =
                    Message::deserialize(&bytes).expect("Failed to deserialize complete message");

                let err = self.handle_complete_message(message.to_owned(), src).await;
                if let Some(e) = err {
                    eprintln!("Error handling complete message: {e}");
                    continue;
                }
                messages.remove(&chunk.msg_id);
            }

            Self::cleanup_old_messages(&mut messages);
        }
        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(SeqCst)
    }

    pub fn register_callback<F, Fut>(&mut self, dest: u16, f: F)
    where
        F: Fn(Message, SocketAddr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let wrapped: Callback = Arc::new(move |data, src| {
            let fut = f(data, src);
            Box::pin(fut)
        });
        self.callbacks.insert(dest, wrapped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::message::Message;
    use std::sync::Mutex;
    use std::time::Duration;
    use tokio::net::UdpSocket;
    use tokio::time::sleep;

    async fn create_test_socket() -> UdpSocket {
        UdpSocket::bind("127.0.0.1:0").await.unwrap()
    }

    #[test]
    fn test_listener_new() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let socket = create_test_socket().await;
            let listener = Listener::new(socket);
            
            assert!(listener.is_running());
            assert!(listener.callbacks.is_empty());
        });
    }

    #[test]
    fn test_listener_stop() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let socket = create_test_socket().await;
            let listener = Listener::new(socket);
            
            assert!(listener.is_running());
            listener.stop();
            assert!(!listener.is_running());
        });
    }

    #[tokio::test]
    async fn test_register_callback() {
        let socket = create_test_socket().await;
        let mut listener = Listener::new(socket);
        
        let received_messages = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received_messages.clone();
        
        listener.register_callback(42, move |msg, src| {
            let received = received_clone.clone();
            async move {
                received.lock().unwrap().push((msg.id, msg.dest, src));
            }
        });
        
        assert_eq!(listener.callbacks.len(), 1);
        assert!(listener.callbacks.contains_key(&42));
    }

    #[tokio::test]
    async fn test_process_chunk_single_chunk() {
        let socket = create_test_socket().await;
        let listener = Listener::new(socket);
        let mut messages = HashMap::new();
        
        let test_data = b"Hello, World!";
        let chunk_view = ChunkView {
            msg_id: 123,
            idx: 0,
            total: 1,
            data: test_data,
        };
        
        let result = listener.process_chunk(&chunk_view, &mut messages).await;
        
        assert!(result.is_some());
        let assembled = result.unwrap();
        assert_eq!(assembled, test_data);
        // The message should still be in the HashMap - it's removed later in internal_start
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn test_process_chunk_multiple_chunks() {
        let socket = create_test_socket().await;
        let listener = Listener::new(socket);
        let mut messages = HashMap::new();
        
        let data1 = b"Hello, ";
        let data2 = b"World!";
        
        // First chunk
        let chunk1 = ChunkView {
            msg_id: 456,
            idx: 0,
            total: 2,
            data: data1,
        };
        
        let result1 = listener.process_chunk(&chunk1, &mut messages).await;
        assert!(result1.is_none()); // Not complete yet
        assert_eq!(messages.len(), 1);
        
        // Second chunk
        let chunk2 = ChunkView {
            msg_id: 456,
            idx: 1,
            total: 2,
            data: data2,
        };
        
        let result2 = listener.process_chunk(&chunk2, &mut messages).await;
        assert!(result2.is_some());
        let assembled = result2.unwrap();
        assert_eq!(assembled, b"Hello, World!");
    }

    #[tokio::test]
    async fn test_process_chunk_out_of_order() {
        let socket = create_test_socket().await;
        let listener = Listener::new(socket);
        let mut messages = HashMap::new();
        
        let data1 = b"First";
        let data2 = b"Second";
        let data3 = b"Third";
        
        // Receive chunks out of order: 2, 0, 1
        let chunk2 = ChunkView {
            msg_id: 789,
            idx: 2,
            total: 3,
            data: data3,
        };
        
        let result1 = listener.process_chunk(&chunk2, &mut messages).await;
        assert!(result1.is_none());
        
        let chunk0 = ChunkView {
            msg_id: 789,
            idx: 0,
            total: 3,
            data: data1,
        };
        
        let result2 = listener.process_chunk(&chunk0, &mut messages).await;
        assert!(result2.is_none());
        
        let chunk1 = ChunkView {
            msg_id: 789,
            idx: 1,
            total: 3,
            data: data2,
        };
        
        let result3 = listener.process_chunk(&chunk1, &mut messages).await;
        assert!(result3.is_some());
        let assembled = result3.unwrap();
        assert_eq!(assembled, b"FirstSecondThird");
    }

    #[tokio::test]
    async fn test_process_chunk_duplicate() {
        let socket = create_test_socket().await;
        let listener = Listener::new(socket);
        let mut messages = HashMap::new();
        
        let test_data = b"Test data";
        let chunk = ChunkView {
            msg_id: 999,
            idx: 0,
            total: 2,
            data: test_data,
        };
        
        // Process same chunk twice
        let result1 = listener.process_chunk(&chunk, &mut messages).await;
        assert!(result1.is_none());
        
        let result2 = listener.process_chunk(&chunk, &mut messages).await;
        assert!(result2.is_none()); // Should still be incomplete
        
        // Verify the message state is correct
        assert_eq!(messages.len(), 1);
        let receiving_msg = messages.get(&999).unwrap();
        assert_eq!(receiving_msg.received_chunks[0], true);
        assert_eq!(receiving_msg.received_chunks[1], false);
    }

    #[tokio::test]
    async fn test_process_chunk_inconsistent_total() {
        let socket = create_test_socket().await;
        let listener = Listener::new(socket);
        let mut messages = HashMap::new();
        
        // First chunk with total = 2
        let chunk1 = ChunkView {
            msg_id: 111,
            idx: 0,
            total: 2,
            data: b"data1",
        };
        
        listener.process_chunk(&chunk1, &mut messages).await;
        assert_eq!(messages.len(), 1);
        
        // Second chunk with different total = 3 (inconsistent)
        let chunk2 = ChunkView {
            msg_id: 111,
            idx: 1,
            total: 3,
            data: b"data2",
        };
        
        let result = listener.process_chunk(&chunk2, &mut messages).await;
        assert!(result.is_none());
        assert!(messages.is_empty()); // Should be removed due to inconsistency
    }

    #[test]
    fn test_assemble_complete_message() {
        let mut entry = ReceivingMessage {
            parts: vec![
                Some(BytesMut::from("Hello, ")),
                Some(BytesMut::from("World!")),
                Some(BytesMut::from(" Test")),
            ],
            received_chunks: vec![true, true, true],
            last_seen: Instant::now(),
            total_expected: 3,
        };
        
        let result = Listener::assemble_complete_message(&mut entry);
        assert_eq!(result, b"Hello, World! Test");
        assert!(entry.parts.is_empty()); // Should be drained
    }

    #[test]
    fn test_cleanup_old_messages() {
        let mut messages = HashMap::new();
        
        // Recent message
        messages.insert(1, ReceivingMessage {
            parts: vec![None],
            received_chunks: vec![false],
            last_seen: Instant::now(),
            total_expected: 1,
        });
        
        // Old message (simulated by creating a time in the past)
        let old_time = Instant::now() - Duration::from_secs(10);
        messages.insert(2, ReceivingMessage {
            parts: vec![None],
            received_chunks: vec![false],
            last_seen: old_time,
            total_expected: 1,
        });
        
        Listener::cleanup_old_messages(&mut messages);
        
        assert_eq!(messages.len(), 1);
        assert!(messages.contains_key(&1));
        assert!(!messages.contains_key(&2));
    }

    #[tokio::test]
    async fn test_handle_complete_message_with_callback() {
        let socket = create_test_socket().await;
        let mut listener = Listener::new(socket);
        
        let received_messages = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received_messages.clone();
        
        listener.register_callback(100, move |msg, src| {
            let received = received_clone.clone();
            async move {
                received.lock().unwrap().push((msg.id, msg.dest, src.to_string()));
            }
        });
        
        let test_message = Message::new(100, b"test data".to_vec());
        let test_addr = "127.0.0.1:8080".parse().unwrap();
        
        let result = listener.handle_complete_message(test_message.clone(), test_addr).await;
        assert!(result.is_none()); // No error
        
        // Give some time for the async callback to execute
        sleep(Duration::from_millis(10)).await;
        
        let messages = received_messages.lock().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, test_message.id);
        assert_eq!(messages[0].1, 100);
    }

    #[tokio::test]
    async fn test_handle_complete_message_no_callback() {
        let socket = create_test_socket().await;
        let listener = Listener::new(socket);
        
        let test_message = Message::new(200, b"test data".to_vec());
        let test_addr = "127.0.0.1:8080".parse().unwrap();
        
        let result = listener.handle_complete_message(test_message, test_addr).await;
        assert!(result.is_some()); // Should return error
        
        let error = result.unwrap();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("No callback registered for destination 200"));
    }

    #[tokio::test]
    async fn test_receiving_message_initialization() {
        let socket = create_test_socket().await;
        let listener = Listener::new(socket);
        let mut messages = HashMap::new();
        
        let chunk = ChunkView {
            msg_id: 777,
            idx: 0,
            total: 5,
            data: b"chunk data",
        };
        
        listener.process_chunk(&chunk, &mut messages).await;
        
        let receiving_msg = messages.get(&777).unwrap();
        assert_eq!(receiving_msg.parts.len(), 5);
        assert_eq!(receiving_msg.received_chunks.len(), 5);
        assert_eq!(receiving_msg.total_expected, 5);
        assert!(receiving_msg.received_chunks[0]);
        assert!(!receiving_msg.received_chunks[1]);
        assert!(!receiving_msg.received_chunks[2]);
        assert!(!receiving_msg.received_chunks[3]);
        assert!(!receiving_msg.received_chunks[4]);
    }

    #[tokio::test]
    async fn test_empty_chunk_data() {
        let socket = create_test_socket().await;
        let listener = Listener::new(socket);
        let mut messages = HashMap::new();
        
        let empty_chunk = ChunkView {
            msg_id: 888,
            idx: 0,
            total: 1,
            data: &[],
        };
        
        let result = listener.process_chunk(&empty_chunk, &mut messages).await;
        assert!(result.is_some());
        let assembled = result.unwrap();
        assert!(assembled.is_empty());
    }

    #[tokio::test]
    async fn test_large_message_assembly() {
        let socket = create_test_socket().await;
        let listener = Listener::new(socket);
        let mut messages = HashMap::new();
        
        // Create a large message split into many chunks
        let chunk_size = 100;
        let total_chunks = 10;
        let mut expected_data = Vec::new();
        
        for i in 0..total_chunks {
            let chunk_data: Vec<u8> = (0..chunk_size).map(|j| (i * chunk_size + j) as u8).collect();
            expected_data.extend_from_slice(&chunk_data);
            
            let chunk = ChunkView {
                msg_id: 555,
                idx: i as u16,
                total: total_chunks as u16,
                data: &chunk_data,
            };
            
            let result = listener.process_chunk(&chunk, &mut messages).await;
            
            if i == total_chunks - 1 {
                // Last chunk should complete the message
                assert!(result.is_some());
                let assembled = result.unwrap();
                assert_eq!(assembled, expected_data);
            } else {
                assert!(result.is_none());
            }
        }
    }
}
