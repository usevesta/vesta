//! One TCP session with a device (port 6668): connect, negotiate the
//! session key (3.4, 3.5), then send requests and read messages.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::crypto::Key;
use crate::frame::{FrameError, MAX_FRAME_LEN};
use crate::protocol::{Codec, Message, ProtocolError, Request, Side, Version, cmd};

/// Devices listen for local sessions here.
pub const DEFAULT_PORT: u16 = 6668;

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("could not connect: {0}")]
    Connect(std::io::Error),
    #[error("the device did not answer in time")]
    Timeout,
    #[error("the device closed the connection")]
    Closed,
    /// A message that does not check out with the local key: the key is
    /// wrong, or another device answers at this address.
    #[error("the device's answer could not be read with its local key")]
    Unreadable,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

pub struct Link {
    stream: TcpStream,
    codec: Codec,
    buf: Vec<u8>,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link")
            .field("codec", &self.codec)
            .finish_non_exhaustive()
    }
}

impl Link {
    /// Connects and, for 3.4 and 3.5, negotiates the session key. Each step
    /// gets `limit`.
    pub async fn open(
        addr: SocketAddr,
        version: Version,
        local_key: Key,
        limit: Duration,
    ) -> Result<Self, LinkError> {
        let stream = timeout(limit, TcpStream::connect(addr))
            .await
            .map_err(|_| LinkError::Timeout)?
            .map_err(LinkError::Connect)?;
        // Small request/response messages: do not wait to fill packets.
        stream.set_nodelay(true)?;
        let mut link = Self {
            stream,
            codec: Codec::new(version, local_key, Side::Client),
            buf: Vec::new(),
        };
        if version.negotiates() {
            let (handshake, start) = link.codec.begin_handshake()?;
            link.write(&start, limit).await?;
            let answer = loop {
                let message = timeout(limit, link.recv())
                    .await
                    .map_err(|_| LinkError::Timeout)??;
                if message.cmd == cmd::SESS_KEY_NEG_RESP {
                    break message;
                }
            };
            let finish = link.codec.complete_handshake(handshake, &answer)?;
            link.write(&finish, limit).await?;
        }
        Ok(link)
    }

    async fn write(&mut self, frame: &[u8], limit: Duration) -> Result<(), LinkError> {
        timeout(limit, self.stream.write_all(frame))
            .await
            .map_err(|_| LinkError::Timeout)??;
        Ok(())
    }

    /// Sends one request.
    pub async fn send(&mut self, request: &Request, limit: Duration) -> Result<(), LinkError> {
        let frame = self.codec.encode(request.cmd, &request.payload)?;
        self.write(&frame, limit).await
    }

    /// The next message from the device. Cancel-safe: nothing read is lost
    /// if the future is dropped.
    pub async fn recv(&mut self) -> Result<Message, LinkError> {
        loop {
            match self.codec.decode(&self.buf) {
                Ok(Some((message, used))) => {
                    self.buf.drain(..used);
                    return Ok(message);
                }
                Ok(None) => {}
                Err(skip) => {
                    self.buf.drain(..skip.consumed.min(self.buf.len()));
                    match skip.error {
                        FrameError::BadCheck | FrameError::Crypto(_) => {
                            return Err(LinkError::Unreadable);
                        }
                        error => {
                            tracing::debug!(%error, "skipped bytes from the device");
                            continue;
                        }
                    }
                }
            }
            if self.buf.len() > 2 * MAX_FRAME_LEN {
                self.buf.clear();
            }
            let mut chunk = [0u8; 4096];
            let read = self.stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(LinkError::Closed);
            }
            self.buf.extend_from_slice(&chunk[..read]);
        }
    }

    /// Closes the session.
    pub async fn close(mut self) {
        let _ = self.stream.shutdown().await;
    }
}
