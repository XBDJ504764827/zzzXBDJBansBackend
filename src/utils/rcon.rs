use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Duration;
use byteorder::{LittleEndian, WriteBytesExt, ReadBytesExt};
use std::io::Cursor;

pub async fn check_rcon(address: &str, password: &str) -> Result<(), String> {
    // Connect with timeout
    let stream = tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect(address)
    ).await
    .map_err(|_| "Connection timed out")?
    .map_err(|e| format!("Failed to connect: {}", e))?;

    let mut stream = stream;

    // Build Auth Packet
    // Size (4) + ID (4) + Type (4) + Body (Str + \0) + Empty (1)
    // Packet Size = 4 + 4 + BodyLen + 1 + 1
    // Type 3 = SERVERDATA_AUTH
    
    let req_id = 999;
    let packet_type = 3;
    let body = password.as_bytes();
    let body_len = body.len() as i32;
    // Total Size field value = 4 (ID) + 4 (Type) + body_len + 1 (null) + 1 (null)
    // Note: The Size field itself is NOT included in the size value.
    let packet_size = 4 + 4 + body_len + 1 + 1;
    
    let mut buffer = Vec::new();
    WriteBytesExt::write_i32::<LittleEndian>(&mut buffer, packet_size).unwrap();
    WriteBytesExt::write_i32::<LittleEndian>(&mut buffer, req_id).unwrap();
    WriteBytesExt::write_i32::<LittleEndian>(&mut buffer, packet_type).unwrap();
    buffer.extend_from_slice(body);
    buffer.push(0x00); // Body null terminator
    buffer.push(0x00); // Empty string null terminator (Packet padding?) - Protocol says "Body... null... null"

    // Send
    stream.write_all(&buffer).await.map_err(|e| format!("Write failed: {}", e))?;

    // Read Response
    // First response usually SERVERDATA_RESPONSE_VALUE (Type 0) (optional?), then SERVERDATA_AUTH_RESPONSE (Type 2)
    // Or just Auth Response. 
    // We loop reading packets until we get Type 2.
    
    let mut read_buf = [0u8; 4096];
    
    // Simple read loop with timeout
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let n = stream.read(&mut read_buf).await.map_err(|e| e.to_string())?;
            if n < 4 { return Err("Connection closed or invalid response".to_string()); }
            
            // Parse packet(s)
            let mut cursor = Cursor::new(&read_buf[..n]);
            while (cursor.position() as usize) < n {
                if n - (cursor.position() as usize) < 4 { break; } // Not enough for size
                let size = ReadBytesExt::read_i32::<LittleEndian>(&mut cursor).unwrap() as usize;
                
                // Check if we have the full packet
                if n - (cursor.position() as usize) < size { 
                    // In a real implementation we would buffer remaining and read more. 
                    // For auth check, usually response is small and comes at once.
                    // Assuming simplified scenario for now.
                    break; 
                }
                
                let _id = ReadBytesExt::read_i32::<LittleEndian>(&mut cursor).unwrap();
                let type_ = ReadBytesExt::read_i32::<LittleEndian>(&mut cursor).unwrap();
                
                // Skip Body (read until null) + Empty null
                // We just seek forward essentially, or read to verify
                // Size includes ID(4) + Type(4) + Body + null.
                // Current position is after Type. 
                // Remaining to skip = Size - 4 - 4.
                let advance = size - 8;
                cursor.set_position(cursor.position() + advance as u64);
                
                if type_ == 2 { // SERVERDATA_AUTH_RESPONSE
                    if _id == -1 {
                        return Err("Authentication failed (Bad Password)".to_string());
                    } else if _id == req_id {
                        return Ok(()); // Success
                    }
                }
            }
        }
    }).await;

    match result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("Response timed out".to_string()),
    }
}
