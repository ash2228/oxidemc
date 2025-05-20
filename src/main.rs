use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, Result},
    net::{TcpListener, TcpStream},
};
mod proxy;
use proxy::server::start_server;
#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:25565").await?;
    println!("Listening on 127.0.0.1:25565");
    tokio::spawn(start_server());
    loop {
        let (mut socket, client_addr) = listener.accept().await?;
        println!("Client connected from: {client_addr}");
        tokio::spawn(async move {
            if let Err(e) = handle_connection(&mut socket).await {
                eprintln!("Connection error: {}", e);
            }
        });
    }
}

async fn handle_connection(stream: &mut TcpStream) -> Result<()> {
    let mut buffer = vec![0u8; 1024];
    let n = stream.read(&mut buffer).await?;
    if n == 0 {
        return Ok(());
    }

    let mut cursor = 0;

    // Read packet length (skip)
    let (_len, len_size) = read_varint(&buffer[cursor..]);
    cursor += len_size;

    // Read packet ID
    let (packet_id, id_size) = read_varint(&buffer[cursor..]);
    cursor += id_size;

    if packet_id == 0x00 {
        // Handshake
        let (_, proto_size) = read_varint(&buffer[cursor..]);
        cursor += proto_size;

        // Skip server address
        let addr_len = buffer[cursor] as usize;
        cursor += 1 + addr_len;

        cursor += 2; // Skip port

        // Next state (1=status, 2=login)
        let (next_state, _) = read_varint(&buffer[cursor..]);

        match next_state {
            1 => handle_status(stream).await?,
            2 => handle_login(stream).await?,
            _ => return Ok(()), // Unknown state
        }
    }

    Ok(())
}

async fn handle_status(stream: &mut TcpStream) -> tokio::io::Result<()> {
    let mut buffer = vec![0u8; 1024];

    // Wait for status request (packet ID 0x00)
    let n = stream.read(&mut buffer).await?;
    if n == 0 {
        return Ok(());
    }

    // Send status response
    let response_json = r#"{
        "version": { "name": "1.21", "protocol": 770 },
        "players": { "max": 100, "online": 0 },
        "description": { "text": "Hello from OxideMC" }
    }"#;

    let mut packet = write_varint(0x00); // Packet ID
    packet.extend(write_varint(response_json.len() as i32));
    packet.extend(response_json.as_bytes());

    let mut final_packet = Vec::new();
    final_packet.extend(write_varint(packet.len() as i32));
    final_packet.extend(packet);

    stream.write_all(&final_packet).await?;

    // Wait for ping (packet ID 0x01)
    let n = stream.read(&mut buffer).await?;
    if n == 0 {
        return Ok(());
    }

    // Respond with pong (last 8 bytes are timestamp)
    let payload = &buffer[n.saturating_sub(8)..n]; // Safe slice

    let mut pong = write_varint(0x01); // Pong packet ID
    pong.extend(payload);

    let mut final_pong = write_varint(pong.len() as i32);
    final_pong.extend(pong);

    stream.write_all(&final_pong).await?;

    Ok(())
}

async fn handle_login(stream: &mut TcpStream) -> tokio::io::Result<()> {
    let mut buffer = vec![0u8; 1024];

    // 1. Read Login Start packet
    let n = stream.read(&mut buffer).await?;
    if n == 0 {
        return Ok(());
    }

    let mut cursor = 0;
    let (_len, len_size) = read_varint(&buffer[cursor..]);
    cursor += len_size;

    let (packet_id, id_size) = read_varint(&buffer[cursor..]);
    cursor += id_size;

    if packet_id != 0x00 {
        return Err(tokio::io::Error::new(
            tokio::io::ErrorKind::InvalidData,
            "Expected Login Start packet",
        ));
    }

    // 2. Read username
    let (username_len, len_size) = read_varint(&buffer[cursor..]);
    cursor += len_size;
    let username = String::from_utf8_lossy(&buffer[cursor..cursor + username_len as usize]);
    let (uuid_len, _) = read_varint(&buffer[cursor..]);
    // Read 16 raw bytes (no UTF-8 conversion!)
    let uuid_bytes = &buffer[cursor..cursor + 16];
    cursor += 16;

    // Optionally convert to hex string for logging
    let uuid_hex = uuid_bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    println!("UUID: {}", uuid_hex);

    // Write raw bytes to packet
    // 3. Build CORRECT Login Success packet
    let mut packet = Vec::new();
    packet.extend(write_varint(0x02));
    packet.extend(uuid_bytes);

    // Packet ID (0x02)

    // Username
    packet.extend(write_varint(username.len() as i32));
    packet.extend(username.as_bytes());

    // Properties (empty array)
    packet.extend(write_varint(0)); // Zero properties

    // 4. Prepend packet length
    let mut final_packet = write_varint(packet.len() as i32);
    final_packet.extend(packet);

    stream.write_all(&final_packet).await?;
    handle_play_state(stream).await?;
    Ok(())
}

async fn handle_play_state(stream: &mut TcpStream) -> tokio::io::Result<()> {
    // Send Join Game packet (simplified)
    let join_game_packet = vec![
        0x26, // Join Game packet ID
        0x01, // Player entity ID
        0x00, // Hardcore flag
        0x01, // Gamemode
        0x01, // Previous Gamemode
        0x01, // World count
        0x00, // World names (empty for simplicity)
    ];
    stream.write_all(&join_game_packet).await?;

    // 2. Spawn Position (critical!)
    let mut spawn_pos = Vec::new();
    spawn_pos.extend(write_varint(0x4E)); // Packet ID
    spawn_pos.extend(0x00_i64.to_be_bytes()); // X
    spawn_pos.extend(0x00_i64.to_be_bytes()); // Y
    spawn_pos.extend(0x00_i64.to_be_bytes()); // Z

    stream.write_all(&spawn_pos).await?;

    // 4. Empty Chunk Data
    // let mut chunk_data = Vec::new();
    // chunk_data.extend(write_varint(0x22)); // Packet ID
    // chunk_data.extend(0_i32.to_be_bytes()); // Chunk X
    // chunk_data.extend(0_i32.to_be_bytes()); // Chunk Z

    // // Heightmaps (simplified)
    // let mut heightmaps = Vec::new();
    // heightmaps.extend(write_varint(1)); // Compound tag
    // heightmaps.extend(b"MOTION_BLOCKING");
    // heightmaps.extend(write_varint(0x0C)); // Long array tag
    // heightmaps.extend(write_varint(36)); // Length
    // heightmaps.extend(vec![0; 36 * 8]); // Empty heightmap

    // chunk_data.extend(write_varint(heightmaps.len() as i32));
    // chunk_data.extend(heightmaps);

    // // 5. Time Update
    // let mut time_update = Vec::new();
    // time_update.extend(write_varint(0x5E)); // Packet ID
    // time_update.extend(0_i64.to_be_bytes()); // World age
    // time_update.extend(6000_i64.to_be_bytes()); // Time of day

    // stream.write_all(&time_update).await?;

    // // 6. Player Position
    // let mut player_pos = Vec::new();
    // player_pos.extend(write_varint(0x38)); // Packet ID
    // player_pos.extend(0.0_f64.to_be_bytes()); // X
    // player_pos.extend(64.0_f64.to_be_bytes()); // Y
    // player_pos.extend(0.0_f64.to_be_bytes()); // Z
    // player_pos.extend(0.0_f32.to_be_bytes()); // Yaw
    // player_pos.extend(0.0_f32.to_be_bytes()); // Pitch
    // player_pos.push(0); // Flags

    // stream.write_all(&player_pos).await?;

    // // Empty sections
    // chunk_data.extend(write_varint(0)); // 0 sections

    // stream.write_all(&chunk_data).await?;

    // Keep connection open
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

fn write_varint(mut value: i32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        if (value & !0x7F) == 0 {
            out.push(value as u8);
            return out;
        }
        out.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
}

fn read_varint(data: &[u8]) -> (i32, usize) {
    let mut result = 0;
    let mut shift = 0;
    let mut bytes_read = 0;

    for byte in data {
        let val = (byte & 0x7F) as i32;
        result |= val << shift;
        bytes_read += 1;

        if byte & 0x80 == 0 {
            break;
        }

        shift += 7;
    }

    (result, bytes_read)
}
