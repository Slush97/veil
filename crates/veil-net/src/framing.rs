use crate::NetError;

/// Send a length-prefixed message over a QUIC unidirectional stream.
pub async fn send_framed(conn: &quinn::Connection, data: &[u8]) -> Result<(), NetError> {
    let mut stream = conn
        .open_uni()
        .await
        .map_err(|e| NetError::Connection(e.to_string()))?;

    let len = (data.len() as u32).to_be_bytes();
    stream
        .write_all(&len)
        .await
        .map_err(|e| NetError::Connection(e.to_string()))?;
    stream
        .write_all(data)
        .await
        .map_err(|e| NetError::Connection(e.to_string()))?;
    stream
        .finish()
        .map_err(|e| NetError::Connection(e.to_string()))?;

    Ok(())
}

/// Receive a length-prefixed message from a QUIC unidirectional stream.
pub async fn recv_framed(conn: &quinn::Connection, max_size: u64) -> Result<Vec<u8>, NetError> {
    let mut stream = conn
        .accept_uni()
        .await
        .map_err(|e| NetError::Connection(e.to_string()))?;

    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| NetError::Connection(e.to_string()))?;
    let len = u32::from_be_bytes(len_buf) as u64;

    if len > max_size {
        return Err(NetError::Protocol(format!(
            "message too large: {len} bytes (max {max_size})"
        )));
    }

    let mut data = vec![0u8; len as usize];
    stream
        .read_exact(&mut data)
        .await
        .map_err(|e| NetError::Connection(e.to_string()))?;

    Ok(data)
}
