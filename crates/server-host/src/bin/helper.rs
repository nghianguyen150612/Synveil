#![forbid(unsafe_code)]
#[cfg(target_os = "linux")]
fn main() {
    if run().is_err() {
        std::process::exit(1);
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), &'static str> {
    use nix::{
        poll::{PollFd, PollFlags, poll},
        sys::socket::{getsockopt, sockopt::PeerCredentials},
        unistd::geteuid,
    };
    use std::{
        fs,
        io::{self, Read},
        os::{
            fd::AsFd,
            unix::{fs::PermissionsExt, net::UnixListener},
        },
        path::Path,
        time::{Duration, Instant},
    };
    use synveil_server_host::{native::NativeOwners, protocol::*};
    struct DeadlineReader<R> {
        reader: R,
        deadline: Instant,
    }
    impl<R: Read + AsFd> Read for DeadlineReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let remaining = self
                .deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "IPC deadline"))?;
            let mut descriptors = [PollFd::new(self.reader.as_fd(), PollFlags::POLLIN)];
            let timeout =
                u16::try_from(remaining.as_millis().min(u16::MAX as u128)).unwrap_or(u16::MAX);
            if poll(&mut descriptors, timeout).map_err(io::Error::other)? == 0 {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "IPC deadline"));
            }
            self.reader.read(buffer)
        }
    }
    if !geteuid().is_root() {
        return Err("root helper required");
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["session"] {
        let actor: u32 = std::env::var("PKEXEC_UID")
            .map_err(|_| "polkit actor required")?
            .parse()
            .map_err(|_| "invalid polkit actor")?;
        let mut owners = NativeOwners::new(actor)?;
        let input = io::stdin();
        let mut output = io::stdout();
        for _ in 0..256 {
            // Idle elevation expires. Each complete frame has its own short
            // deadline, including a peer which trickles an incomplete body.
            let mut descriptors = [PollFd::new(input.as_fd(), PollFlags::POLLIN)];
            let mut elapsed = 0;
            while poll(&mut descriptors, 60_000u16).map_err(|_| "IPC poll failed")? == 0 {
                elapsed += 60;
                if elapsed >= 300 {
                    return Ok(());
                }
            }
            let mut reader = DeadlineReader {
                reader: io::stdin(),
                deadline: Instant::now() + Duration::from_secs(15),
            };
            let request: Request = read_frame(&mut reader)?;
            let cancel = matches!(request, Request::Cancel);
            let mut response = owners.dispatch(request);
            let written = write_frame(&mut output, &response);
            if let Response::Enrollment { token } = &mut response {
                use zeroize::Zeroize;
                token.zeroize();
            }
            written?;
            if cancel {
                break;
            }
        }
        Ok(())
    } else if args == ["inspect-service"] {
        let directory = Path::new("/run/synveil-host");
        synveil_server_service::native::directory(directory, 0o755)?;
        if Path::new(INSPECT_SOCKET).exists() {
            return Err("inspection socket already exists; systemd must own its lifetime");
        }
        let listener =
            UnixListener::bind(INSPECT_SOCKET).map_err(|_| "inspection socket unavailable")?;
        fs::set_permissions(INSPECT_SOCKET, fs::Permissions::from_mode(0o666))
            .map_err(|_| "inspection socket permissions failed")?;
        for connection in listener.incoming() {
            let Ok(mut socket) = connection else {
                continue;
            };
            let Ok(peer) = getsockopt(&socket, PeerCredentials) else {
                continue;
            };
            // Inspection has exactly one request variant, never a mutation.
            let mut reader = DeadlineReader {
                reader: socket,
                deadline: Instant::now() + Duration::from_secs(2),
            };
            if !matches!(read_frame::<_, Request>(&mut reader), Ok(Request::Inspect)) {
                continue;
            }
            let mut socket = reader.reader;
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|_| "inspection timeout unavailable")?;
            let response = match NativeOwners::new(peer.uid()) {
                Ok(mut owner) => owner.dispatch(Request::Inspect),
                Err(error) => Response::Error {
                    code: error.to_owned(),
                },
            };
            let _ = write_frame(&mut socket, &response);
        }
        Ok(())
    } else {
        Err("unsupported purpose-specific helper operation")
    }
}
