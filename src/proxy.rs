pub mod server {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };
    use super::*;

    pub async fn start_server() -> tokio::io::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:8080").await?;
        println!("Proxy server listening on 127.0.0.1:8080");

        while let Ok((mut client_socket, client_addr)) = listener.accept().await {
            println!("New connection from: {}", client_addr);
            
            tokio::spawn(async move {
                let mut server_socket = match TcpStream::connect("127.0.0.1:25565").await {
                    Ok(socket) => socket,
                    Err(e) => {
                        eprintln!("Failed to connect to MC server: {}", e);
                        return;
                    }
                };

                let (mut client_reader, mut client_writer) = client_socket.split();
                let (mut server_reader, mut server_writer) = server_socket.split();

                // Create separate buffers for each direction
                let mut client_buffer = vec![0u8; 1024];
                let mut server_buffer = vec![0u8; 1024];

                loop {
                    tokio::select! {
                        // Client -> Server
                        n = client_reader.read(&mut client_buffer) => {
                            match n {
                                Ok(0) => break, // Connection closed
                                Ok(n) => {
                                    match Packet::new(&client_buffer[..n]) {
                                        Ok((packet, _)) => {
                                           println!("[CLIENT->SERVER]: packet id: {}, packet length:{}, data:", packet.packet_id, packet.length);
                                           for p in packet.data {
                                            println!("{p}");
                                           }
                                            // Forward raw bytes (you could modify packet here if needed)
                                            if let Err(e) = server_writer.write_all(&client_buffer[..n]).await {
                                                eprintln!("Server write error: {}", e);
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Packet parse error: {}", e);
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Client read error: {}", e);
                                    break;
                                }
                            }
                        }

                        // Server -> Client
                        n = server_reader.read(&mut server_buffer) => {
                            match n {
                                Ok(0) => break, // Connection closed
                                Ok(n) => {
                                    match Packet::new(&server_buffer[..n]) {
                                        Ok((packet, _)) => {
                                            println!("[SERVER->CLIENT]: {packet:?}");
                                            // Forward raw bytes
                                            if let Err(e) = client_writer.write_all(&server_buffer[..n]).await {
                                                eprintln!("Client write error: {}", e);
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Packet parse error: {}", e);
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Server read error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }
                println!("Connection with {} closed", client_addr);
            });
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Packet {
    pub length: i32,      // Total packet length (VarInt)
    pub packet_id: i32,   // Packet ID (VarInt)
    pub data: Vec<u8>,    // Remaining packet data
}

impl Packet {
    pub fn new(raw_data: &[u8]) -> Result<(Self, usize), &'static str> {
        let mut cursor = 0;
        
        // Read packet length (VarInt)
        let (length, bytes_read) = read_varint(&raw_data[cursor..])?;
        cursor += bytes_read;
        
        // Read packet ID (VarInt)
        let (packet_id, bytes_read) = read_varint(&raw_data[cursor..])?;
        cursor += bytes_read;
        
        // Remaining bytes are packet data
        let data = raw_data[cursor..].to_vec();
        
        Ok((Packet { length, packet_id, data }, cursor))
    }
}

// VarInt reader (returns value and bytes consumed)
fn read_varint(data: &[u8]) -> Result<(i32, usize), &'static str> {
    let mut result = 0;
    let mut shift = 0;
    let mut bytes_read = 0;

    for byte in data {
        bytes_read += 1;
        result |= ((byte & 0x7F) as i32) << shift;
        shift += 7;

        if (byte & 0x80) == 0 {
            return Ok((result, bytes_read));
        }

        if bytes_read >= 5 {
            return Err("VarInt too long (max 5 bytes)");
        }
    }

    Err("Incomplete VarInt")
}