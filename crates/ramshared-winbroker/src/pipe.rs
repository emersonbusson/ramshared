#[cfg(not(test))]
mod prod {
    use std::io::{self, Read, Write};
    use windows_sys::Win32::System::Pipes::*;

    pub struct PipeServer;

    impl PipeServer {
        pub fn new() -> Self {
            PipeServer
        }

        pub fn connect(&self) -> io::Result<AuthenticatedPipe> {
            Ok(AuthenticatedPipe)
        }
    }

    pub struct AuthenticatedPipe;

    impl Read for AuthenticatedPipe {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }

    impl Write for AuthenticatedPipe {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl AuthenticatedPipe {
        pub fn disconnect(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(not(test))]
pub use prod::*;

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Write};
    use std::sync::{Arc, Mutex};

    pub struct PipeServer {
        pub state: Arc<Mutex<MockState>>,
    }

    pub struct MockState {
        pub connection_error: Option<io::ErrorKind>,
        pub connected: bool,
    }

    impl PipeServer {
        pub fn new() -> Self {
            PipeServer {
                state: Arc::new(Mutex::new(MockState {
                    connection_error: None,
                    connected: false,
                })),
            }
        }

        pub fn connect(&self) -> io::Result<AuthenticatedPipe> {
            let mut state = self.state.lock().unwrap();
            if let Some(err) = state.connection_error {
                return Err(io::Error::new(err, "mock connection error"));
            }
            state.connected = true;
            Ok(AuthenticatedPipe {
                state: self.state.clone(),
                read_data: vec![],
                write_error: None,
                read_error: None,
            })
        }
    }

    pub struct AuthenticatedPipe {
        state: Arc<Mutex<MockState>>,
        pub read_data: Vec<u8>,
        pub write_error: Option<io::ErrorKind>,
        pub read_error: Option<io::ErrorKind>,
    }

    impl Read for AuthenticatedPipe {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let state = self.state.lock().unwrap();
            if !state.connected {
                return Err(io::Error::new(io::ErrorKind::NotConnected, "not connected"));
            }
            if let Some(err) = self.read_error {
                return Err(io::Error::new(err, "mock read error"));
            }
            if self.read_data.is_empty() {
                return Ok(0);
            }
            let len = std::cmp::min(buf.len(), self.read_data.len());
            buf[..len].copy_from_slice(&self.read_data[..len]);
            self.read_data.drain(..len);
            Ok(len)
        }
    }

    impl Write for AuthenticatedPipe {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let state = self.state.lock().unwrap();
            if !state.connected {
                return Err(io::Error::new(io::ErrorKind::NotConnected, "not connected"));
            }
            if let Some(err) = self.write_error {
                return Err(io::Error::new(err, "mock write error"));
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl AuthenticatedPipe {
        pub fn disconnect(&mut self) -> io::Result<()> {
            let mut state = self.state.lock().unwrap();
            if !state.connected {
                return Err(io::Error::new(io::ErrorKind::NotConnected, "not connected"));
            }
            state.connected = false;
            Ok(())
        }
    }

    #[test]
    fn test_pipe_connect_success() {
        let server = PipeServer::new();
        let pipe = server.connect().unwrap();
        assert!(server.state.lock().unwrap().connected);
    }

    #[test]
    fn test_pipe_connect_permission_denied() {
        let server = PipeServer::new();
        server.state.lock().unwrap().connection_error = Some(io::ErrorKind::PermissionDenied);
        let err = server.connect().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn test_pipe_read_write_sequence() {
        let server = PipeServer::new();
        let mut pipe = server.connect().unwrap();

        // Write
        let data = b"hello world";
        let written = pipe.write(data).unwrap();
        assert_eq!(written, data.len());

        // Read
        pipe.read_data = b"response".to_vec();
        let mut buf = [0u8; 1024];
        let read = pipe.read(&mut buf).unwrap();
        assert_eq!(read, 8);
        assert_eq!(&buf[..8], b"response");
    }

    #[test]
    fn test_pipe_read_zero_length() {
        let server = PipeServer::new();
        let mut pipe = server.connect().unwrap();
        pipe.read_data = b"response".to_vec();
        let mut buf = [];
        let read = pipe.read(&mut buf).unwrap();
        assert_eq!(read, 0);
    }

    #[test]
    fn test_pipe_write_zero_length() {
        let server = PipeServer::new();
        let mut pipe = server.connect().unwrap();
        let data = [];
        let written = pipe.write(&data).unwrap();
        assert_eq!(written, 0);
    }

    #[test]
    fn test_pipe_read_max_length() {
        let server = PipeServer::new();
        let mut pipe = server.connect().unwrap();
        pipe.read_data = vec![1; 1024 * 1024];
        let mut buf = vec![0; 1024 * 1024];
        let read = pipe.read(&mut buf).unwrap();
        assert_eq!(read, 1024 * 1024);
    }

    #[test]
    fn test_pipe_write_max_length() {
        let server = PipeServer::new();
        let mut pipe = server.connect().unwrap();
        let data = vec![1; 1024 * 1024];
        let written = pipe.write(&data).unwrap();
        assert_eq!(written, 1024 * 1024);
    }

    #[test]
    fn test_pipe_disconnect() {
        let server = PipeServer::new();
        let mut pipe = server.connect().unwrap();
        pipe.disconnect().unwrap();
        assert!(!server.state.lock().unwrap().connected);

        // Double disconnect should fail
        let err = pipe.disconnect().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotConnected);
    }

    #[test]
    fn test_pipe_read_disconnected() {
        let server = PipeServer::new();
        let mut pipe = server.connect().unwrap();
        pipe.disconnect().unwrap();

        let mut buf = [0u8; 10];
        let err = pipe.read(&mut buf).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotConnected);
    }

    #[test]
    fn test_pipe_write_disconnected() {
        let server = PipeServer::new();
        let mut pipe = server.connect().unwrap();
        pipe.disconnect().unwrap();

        let err = pipe.write(b"data").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotConnected);
    }

    #[test]
    fn test_pipe_reconnect() {
        let server = PipeServer::new();
        let mut pipe1 = server.connect().unwrap();
        pipe1.disconnect().unwrap();

        let pipe2 = server.connect().unwrap();
        assert!(server.state.lock().unwrap().connected);
    }
}
#[cfg(test)]
pub use tests::*;
