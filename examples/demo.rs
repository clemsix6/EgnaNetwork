use egna_network::{Listener, Message, send};
use anyhow::Result;
use std::net::SocketAddr;
use tokio::{
    net::UdpSocket,
    time::{Duration, sleep},
};

#[tokio::main]
async fn main() -> Result<()> {
    let recv_socket = UdpSocket::bind("0.0.0.0:7000").await?;
    let recv_addr = recv_socket.local_addr()?;

    let mut listener = Listener::new(recv_socket);
    listener.register_callback(34, |message, src: SocketAddr| async move {
        println!(
            "Received message from {}: id={}, dest={}, data_len={}",
            src,
            message.id,
            message.dest,
            message.data.len()
        );
    });
    listener.start().await;

    sleep(Duration::from_millis(100)).await;

    let send_socket = UdpSocket::bind("0.0.0.0:0").await?;
    let string = "This is a test message to be sent over the network.".repeat(500);
    let message = Message::new(34, string.as_bytes().to_vec());

    loop {
        send(&send_socket, &recv_addr, &message).await?;
        sleep(Duration::from_millis(1)).await;
    }
}