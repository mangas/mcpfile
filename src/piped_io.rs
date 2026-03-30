use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

/// RAII guard that removes the socket file on drop.
struct SocketGuard {
    path: PathBuf,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Bridges a Unix socket to a pair of async byte streams.
/// No Docker knowledge — works with any (AsyncRead, AsyncWrite) upstream.
pub struct PipedIo {
    listener: UnixListener,
    path: PathBuf,
}

impl PipedIo {
    /// Bind a Unix socket. Removes any stale file at `path` first.
    pub async fn bind(path: &Path) -> Result<Self> {
        let _ = tokio::fs::remove_file(path).await;
        let listener = UnixListener::bind(path)?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }

    /// Accept clients and bridge each to the upstream streams.
    /// One client at a time; a new connection replaces the previous.
    /// Returns when the upstream read side closes (container exited).
    pub async fn run(
        self,
        upstream_read: impl AsyncRead + Unpin + Send + 'static,
        mut upstream_write: impl AsyncWrite + Unpin + Send,
    ) -> Result<()> {
        let _guard = SocketGuard {
            path: self.path.clone(),
        };

        // Shared write half of the current client. The upstream reader task
        // writes to this; the accept loop swaps it on each new connection.
        let client_writer: Arc<Mutex<Option<tokio::net::unix::OwnedWriteHalf>>> =
            Arc::new(Mutex::new(None));

        // Task: read upstream → write to current client
        let writer_ref = client_writer.clone();
        let upstream_task = tokio::spawn(async move {
            let mut reader = upstream_read;
            let mut buf = vec![0u8; 8192];
            loop {
                let n = match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let mut lock = writer_ref.lock().await;
                if let Some(ref mut w) = *lock {
                    // Ignore write errors (client may have disconnected)
                    let _ = w.write_all(&buf[..n]).await;
                }
            }
            // Upstream closed — shut down current client to unblock accept loop
            let mut lock = writer_ref.lock().await;
            if let Some(ref mut w) = *lock {
                let _ = w.shutdown().await;
            }
        });

        // Accept loop: each client gets bridged to upstream_write
        let mut buf = vec![0u8; 8192];
        loop {
            // Check if upstream died between client connections
            if upstream_task.is_finished() {
                break;
            }

            let stream: UnixStream = tokio::select! {
                result = self.listener.accept() => {
                    match result {
                        Ok((s, _)) => s,
                        Err(_) => break,
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => continue,
            };

            let (client_read, client_write) = stream.into_split();

            // Install new client's write half for the upstream reader task
            {
                let mut lock = client_writer.lock().await;
                *lock = Some(client_write);
            }

            // Read from client → upstream_write
            let mut client_read = client_read;
            loop {
                let n = match client_read.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                if upstream_write.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }

            // Client disconnected
            {
                let mut lock = client_writer.lock().await;
                *lock = None;
            }

            // If upstream task is done, container exited
            if upstream_task.is_finished() {
                break;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn bridges_data_between_socket_and_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");

        // In-memory streams standing in for container stdin/stdout
        let (upstream_read, mut write_to_upstream) = tokio::io::duplex(1024);
        let (mut read_from_upstream, upstream_write) = tokio::io::duplex(1024);

        let piped_io = PipedIo::bind(&sock_path).await.unwrap();
        let path = sock_path.clone();
        let handle = tokio::spawn(async move { piped_io.run(upstream_read, upstream_write).await });

        // Wait for socket
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Connect as client
        let mut client = UnixStream::connect(&path).await.unwrap();

        // Client → upstream
        client.write_all(b"hello").await.unwrap();
        let mut buf = vec![0u8; 5];
        read_from_upstream.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");

        // Upstream → client
        write_to_upstream.write_all(b"world").await.unwrap();
        let mut buf = vec![0u8; 5];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"world");

        // Clean shutdown: drop the upstream write side to signal EOF
        drop(write_to_upstream);
        // Give the upstream task time to notice EOF and shut down
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        drop(client);
        let result = handle.await.unwrap();
        assert!(result.is_ok());

        // Socket file should be cleaned up
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn socket_cleaned_up_on_upstream_close() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("cleanup.sock");

        let (upstream_read, write_to_upstream) = tokio::io::duplex(1024);
        let (_, upstream_write) = tokio::io::duplex(1024);

        let piped_io = PipedIo::bind(&sock_path).await.unwrap();
        let path = sock_path.clone();
        let handle = tokio::spawn(async move { piped_io.run(upstream_read, upstream_write).await });

        assert!(path.exists());

        // Close upstream — simulates container exit
        drop(write_to_upstream);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let result = handle.await.unwrap();
        assert!(result.is_ok());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn multiple_sequential_clients() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("multi.sock");

        let (upstream_read, write_to_upstream) = tokio::io::duplex(1024);
        let (mut read_from_upstream, upstream_write) = tokio::io::duplex(1024);

        let piped_io = PipedIo::bind(&sock_path).await.unwrap();
        let path = sock_path.clone();
        let handle = tokio::spawn(async move { piped_io.run(upstream_read, upstream_write).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // First client
        {
            let mut c1 = UnixStream::connect(&path).await.unwrap();
            c1.write_all(b"c1").await.unwrap();
            let mut buf = vec![0u8; 2];
            read_from_upstream.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"c1");
            // c1 dropped here
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Second client
        {
            let mut c2 = UnixStream::connect(&path).await.unwrap();
            c2.write_all(b"c2").await.unwrap();
            let mut buf = vec![0u8; 2];
            read_from_upstream.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"c2");
        }

        // Shut down
        drop(write_to_upstream);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }
}
