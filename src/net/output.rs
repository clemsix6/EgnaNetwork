use crate::net::chunk::Chunk;
use crate::net::consts::{CHUNK_SIZE, HEADER_SIZE, SEND_TIMEOUT};
use crate::net::message::Message;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

pub async fn send(
    socket: &UdpSocket,
    peer: &SocketAddr,
    message: &Message,
) -> io::Result<()> {
    let max_payload = CHUNK_SIZE - HEADER_SIZE;
    let data = message.serialize();
    let total = ((data.len() + max_payload - 1) / max_payload) as u16;

    for (i, chunk) in data.chunks(max_payload).enumerate() {
        let chunk = Chunk::new(message.id, i as u16, total, chunk.to_vec());
        let buf = chunk.serialize();

        let _ = timeout(
            Duration::from_millis(SEND_TIMEOUT),
            socket.send_to(&buf, peer),
        )
        .await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::chunk::Chunk;
    use crate::net::message::Message;
    use std::collections::HashMap;
    use tokio::net::UdpSocket;

    async fn create_test_socket() -> UdpSocket {
        UdpSocket::bind("127.0.0.1:0").await.unwrap()
    }

    async fn create_bound_socket_pair() -> (UdpSocket, UdpSocket, SocketAddr, SocketAddr) {
        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sender_addr = sender.local_addr().unwrap();
        let receiver_addr = receiver.local_addr().unwrap();
        (sender, receiver, sender_addr, receiver_addr)
    }

    #[tokio::test]
    async fn test_send_small_message() {
        let (sender, receiver, _, receiver_addr) = create_bound_socket_pair().await;
        
        let small_data = b"Hello, World!".to_vec();
        let message = Message::new(42, small_data.clone());
        
        let send_result = send(&sender, &receiver_addr, &message).await;
        assert!(send_result.is_ok());
        
        let mut buf = vec![0u8; 1500];
        let (len, _) = receiver.recv_from(&mut buf).await.unwrap();
        
        buf.resize(len, 0);
        let chunk = Chunk::deserialize(&buf).unwrap();
        
        assert_eq!(chunk.msg_id, message.id);
        assert_eq!(chunk.idx, 0);
        assert_eq!(chunk.total, 1);
        
        let received_message = Message::deserialize(chunk.data).unwrap();
        assert_eq!(received_message.dest, 42);
        assert_eq!(received_message.data, small_data);
    }

    #[tokio::test]
    async fn test_send_large_message_multiple_chunks() {
        let (sender, receiver, _, receiver_addr) = create_bound_socket_pair().await;
        
        let max_payload = CHUNK_SIZE - HEADER_SIZE;
        let large_data = vec![0x42u8; max_payload * 3 + 100]; // Will require 4 chunks
        let message = Message::new(100, large_data.clone());
        
        let send_result = send(&sender, &receiver_addr, &message).await;
        assert!(send_result.is_ok());
        
        let mut received_chunks = HashMap::new();
        let mut buf = vec![0u8; 1500];
        
        for _ in 0..4 {
            let (len, _) = receiver.recv_from(&mut buf).await.unwrap();
            buf.resize(len, 0);
            let chunk = Chunk::deserialize(&buf).unwrap();
            
            assert_eq!(chunk.msg_id, message.id);
            assert_eq!(chunk.total, 4);
            
            received_chunks.insert(chunk.idx, chunk.data.to_vec());
            buf.resize(1500, 0);
        }
        
        assert_eq!(received_chunks.len(), 4);
        
        let mut reassembled = Vec::new();
        for i in 0..4 {
            reassembled.extend_from_slice(&received_chunks[&i]);
        }
        
        let received_message = Message::deserialize(&reassembled).unwrap();
        assert_eq!(received_message.dest, 100);
        assert_eq!(received_message.data, large_data);
    }

    #[tokio::test]
    async fn test_send_empty_message() {
        let (sender, receiver, _, receiver_addr) = create_bound_socket_pair().await;
        
        let empty_data = Vec::new();
        let message = Message::new(0, empty_data);
        
        let send_result = send(&sender, &receiver_addr, &message).await;
        assert!(send_result.is_ok());
        
        let mut buf = vec![0u8; 1500];
        let (len, _) = receiver.recv_from(&mut buf).await.unwrap();
        
        buf.resize(len, 0);
        let chunk = Chunk::deserialize(&buf).unwrap();
        
        assert_eq!(chunk.msg_id, message.id);
        assert_eq!(chunk.idx, 0);
        assert_eq!(chunk.total, 1);
        
        let received_message = Message::deserialize(chunk.data).unwrap();
        assert_eq!(received_message.dest, 0);
        assert!(received_message.data.is_empty());
    }

    #[tokio::test]
    async fn test_chunk_calculation() {
        let max_payload = CHUNK_SIZE - HEADER_SIZE;
        
        let message_1_byte = Message::new(1, vec![0x42]);
        let data_1 = message_1_byte.serialize();
        let chunks_1 = ((data_1.len() + max_payload - 1) / max_payload) as u16;
        assert_eq!(chunks_1, 1);
        
        let message_exact = Message::new(2, vec![0x42; max_payload - 18]); // -18 for message header
        let data_exact = message_exact.serialize();
        let chunks_exact = ((data_exact.len() + max_payload - 1) / max_payload) as u16;
        assert_eq!(chunks_exact, 1);
        
        let message_overflow = Message::new(3, vec![0x42; max_payload]); // +18 message header = overflow
        let data_overflow = message_overflow.serialize();
        let chunks_overflow = ((data_overflow.len() + max_payload - 1) / max_payload) as u16;
        assert_eq!(chunks_overflow, 2);
    }

    #[tokio::test]
    async fn test_concurrent_sends() {
        let (sender, receiver, _, receiver_addr) = create_bound_socket_pair().await;
        
        let sender = std::sync::Arc::new(sender);
        let mut handles = Vec::new();
        
        for i in 0..5 {
            let sender_clone = sender.clone();
            let addr = receiver_addr;
            let handle = tokio::spawn(async move {
                let data = vec![i as u8; 100];
                let message = Message::new(i as u16, data);
                send(&sender_clone, &addr, &message).await
            });
            handles.push(handle);
        }
        
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }
        
        let mut received_count = 0;
        let mut buf = vec![0u8; 1500];
        
        for _ in 0..5 {
            match tokio::time::timeout(Duration::from_millis(100), receiver.recv_from(&mut buf)).await {
                Ok(_) => received_count += 1,
                Err(_) => break, 
            }
        }
        
        assert_eq!(received_count, 5);
    }

    #[tokio::test]
    async fn test_very_large_message() {
        let (sender, receiver, _, receiver_addr) = create_bound_socket_pair().await;
        
        let max_payload = CHUNK_SIZE - HEADER_SIZE;
        let very_large_data = vec![0xAAu8; max_payload * 10]; 
        let message = Message::new(255, very_large_data.clone());
        
        let send_result = send(&sender, &receiver_addr, &message).await;
        assert!(send_result.is_ok());
        
        let mut received_chunks = HashMap::new();
        let mut buf = vec![0u8; 1500];
        let expected_chunks = ((message.serialize().len() + max_payload - 1) / max_payload) as u16;
        
        for _ in 0..expected_chunks {
            let (len, _) = receiver.recv_from(&mut buf).await.unwrap();
            buf.resize(len, 0);
            let chunk = Chunk::deserialize(&buf).unwrap();
            
            assert_eq!(chunk.msg_id, message.id);
            assert_eq!(chunk.total, expected_chunks);
            assert!(chunk.idx < expected_chunks);
            
            received_chunks.insert(chunk.idx, chunk.data.to_vec());
            buf.resize(1500, 0);
        }
        
        assert_eq!(received_chunks.len(), expected_chunks as usize);
        
        let mut reassembled = Vec::new();
        for i in 0..expected_chunks {
            reassembled.extend_from_slice(&received_chunks[&i]);
        }
        
        let received_message = Message::deserialize(&reassembled).unwrap();
        assert_eq!(received_message.dest, 255);
        assert_eq!(received_message.data, very_large_data);
    }

    #[tokio::test]
    async fn test_message_with_different_destinations() {
        let (sender, receiver, _, receiver_addr) = create_bound_socket_pair().await;
        
        let destinations = [1u16, 42u16, 255u16, 1000u16];
        
        for &dest in &destinations {
            let data = format!("Message for destination {}", dest).into_bytes();
            let message = Message::new(dest, data.clone());
            
            let send_result = send(&sender, &receiver_addr, &message).await;
            assert!(send_result.is_ok());
            
            let mut buf = vec![0u8; 1500];
            let (len, _) = receiver.recv_from(&mut buf).await.unwrap();
            
            buf.resize(len, 0);
            let chunk = Chunk::deserialize(&buf).unwrap();
            let received_message = Message::deserialize(chunk.data).unwrap();
            
            assert_eq!(received_message.dest, dest);
            assert_eq!(received_message.data, data);
        }
    }

    #[tokio::test]
    async fn test_chunk_indices_sequential() {
        let (sender, receiver, _, receiver_addr) = create_bound_socket_pair().await;
        
        let max_payload = CHUNK_SIZE - HEADER_SIZE;
        let large_data = vec![0x55u8; max_payload * 5]; 
        let message = Message::new(77, large_data);
        
        let send_result = send(&sender, &receiver_addr, &message).await;
        assert!(send_result.is_ok());
        
        let mut received_indices = Vec::new();
        let mut buf = vec![0u8; 1500];
        let expected_chunks = ((message.serialize().len() + max_payload - 1) / max_payload) as u16;
        
        for _ in 0..expected_chunks {
            let (len, _) = receiver.recv_from(&mut buf).await.unwrap();
            buf.resize(len, 0);
            let chunk = Chunk::deserialize(&buf).unwrap();
            received_indices.push(chunk.idx);
            buf.resize(1500, 0);
        }
        
        received_indices.sort();
        let expected_indices: Vec<u16> = (0..expected_chunks).collect();
        assert_eq!(received_indices, expected_indices);
    }

    #[tokio::test]
    async fn test_send_timeout_behavior() {
        let socket = create_test_socket().await;
        let fake_addr = "192.0.2.1:12345".parse().unwrap(); 
        
        let message = Message::new(123, b"test data".to_vec());
        
        let start = std::time::Instant::now();
        let result = send(&socket, &fake_addr, &message).await;
        let elapsed = start.elapsed();
        
        assert!(result.is_ok());
        assert!(elapsed < Duration::from_millis(SEND_TIMEOUT * 2));
    }
}
