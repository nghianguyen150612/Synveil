use crate::protocol::*;
use std::{
    io::Read,
    os::fd::AsFd,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};
use synveil_server_bootstrap::{
    CanonicalOwners, CoordinatorError, OwnerObservation, PrivilegedEffect,
};

struct DeadlineReader<R> {
    reader: R,
    deadline: Instant,
}
impl<R: Read + AsFd> Read for DeadlineReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        use nix::poll::{PollFd, PollFlags, poll};
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "IPC deadline"))?;
        let mut descriptors = [PollFd::new(self.reader.as_fd(), PollFlags::POLLIN)];
        let timeout =
            u16::try_from(remaining.as_millis().min(u16::MAX as u128)).unwrap_or(u16::MAX);
        if poll(&mut descriptors, timeout).map_err(std::io::Error::other)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC deadline",
            ));
        }
        self.reader.read(buffer)
    }
}

/// Explicitly launched privileged session. Construction does not elevate; the
/// desktop invokes authorize only after the user chooses Host preflight.
#[derive(Default)]
pub struct NativeHostOwners {
    session: Option<(Child, ChildStdin, ChildStdout)>,
    approved: Option<(String, usize)>,
}
impl NativeHostOwners {
    pub fn authorize(&mut self) -> Result<(), &'static str> {
        if crate::supported_platform().is_none() {
            return Err("Managed Host supports Ubuntu 24.04 and Fedora 42 x86_64");
        }
        if let Some((child, _, _)) = self.session.as_mut() {
            if child
                .try_wait()
                .map_err(|_| "Host helper status unavailable")?
                .is_none()
            {
                return Ok(());
            }
            self.session = None;
            self.approved = None;
        }
        #[cfg(target_os = "linux")]
        synveil_server_service::native::root_path(std::path::Path::new(HELPER))?;
        let mut child = Command::new("/usr/bin/pkexec")
            .arg(HELPER)
            .arg("session")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Host helper is not installed or authorization was denied")?;
        let stdin = child.stdin.take().ok_or("Host helper input unavailable")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Host helper output unavailable")?;
        self.session = Some((child, stdin, stdout));
        Ok(())
    }
    pub fn request(&mut self, mut request: Request) -> Result<Response, &'static str> {
        use zeroize::Zeroize;
        let result = (|| {
            let (_, stdin, stdout) = self
                .session
                .as_mut()
                .ok_or("explicit Host authorization required")?;
            write_frame(stdin, &request)?;
            let mut reader = DeadlineReader {
                reader: stdout,
                deadline: Instant::now() + Duration::from_secs(30 * 60),
            };
            read_frame(&mut reader)
        })();
        match &mut request {
            Request::Account { password, .. } | Request::EnrollDesktop { password, .. } => {
                password.zeroize()
            }
            Request::Review {
                external_database_url: Some(url),
                ..
            }
            | Request::Confirm {
                external_database_url: Some(url),
                ..
            } => url.zeroize(),
            _ => {}
        }
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                if let Some((mut child, _, _)) = self.session.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                self.approved = None;
                return Err(error);
            }
        };
        if let Response::Error { .. } = response {
            self.approved = None;
        }
        Ok(response)
    }
    pub fn prepare_review(
        &mut self,
        selection: HostSelection,
        external_database_url: Option<String>,
    ) -> Result<ReviewedContext, String> {
        self.approved = None;
        match self
            .request(Request::Review {
                selection,
                external_database_url,
            })
            .map_err(str::to_owned)?
        {
            Response::Review { context } => Ok(context),
            Response::Error { code } => Err(code),
            _ => Err("invalid helper review response".to_owned()),
        }
    }
    pub fn confirm_context(
        &mut self,
        context: ReviewedContext,
        external_database_url: Option<String>,
    ) -> Result<(), &'static str> {
        let binding = context.binding.clone();
        match self.request(Request::Confirm {
            context,
            external_database_url,
        })? {
            Response::Complete => {
                self.approved = Some((binding, 0));
                Ok(())
            }
            _ => Err("review changed or helper consent was rejected"),
        }
    }
    pub fn inspect_without_elevation() -> Result<Response, &'static str> {
        #[cfg(target_os = "linux")]
        {
            use std::{
                os::unix::{fs::MetadataExt, net::UnixStream},
                time::Duration,
            };
            let metadata = std::fs::symlink_metadata(INSPECT_SOCKET)
                .map_err(|_| "Host inspection service unavailable")?;
            if metadata.uid() != 0 || metadata.file_type().is_symlink() {
                return Err("untrusted Host inspection endpoint");
            }
            let mut socket = UnixStream::connect(INSPECT_SOCKET)
                .map_err(|_| "Host inspection service unavailable")?;
            let peer =
                nix::sys::socket::getsockopt(&socket, nix::sys::socket::sockopt::PeerCredentials)
                    .map_err(|_| "inspection peer unavailable")?;
            if peer.uid() != 0 {
                return Err("untrusted inspection peer");
            }
            socket
                .set_read_timeout(Some(Duration::from_secs(30)))
                .map_err(|_| "inspection timeout unavailable")?;
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|_| "inspection timeout unavailable")?;
            write_frame(&mut socket, &Request::Inspect)?;
            read_frame(&mut socket)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err("Managed Host unsupported on this platform")
        }
    }
}
impl CanonicalOwners for NativeHostOwners {
    fn inspect(&mut self) -> Result<OwnerObservation, CoordinatorError> {
        let response = if self.session.is_some() {
            self.request(Request::Inspect)
        } else {
            Self::inspect_without_elevation()
        }
        .map_err(|_| CoordinatorError::OwnerRejected)?;
        match response {
            Response::Inspection { observation, .. } => Ok(observation),
            _ => Err(CoordinatorError::OwnerRejected),
        }
    }
    fn current_generation(&mut self) -> Result<u64, CoordinatorError> {
        match self
            .request(Request::Inspect)
            .map_err(|_| CoordinatorError::OwnerRejected)?
        {
            Response::Inspection { generation, .. } => Ok(generation),
            _ => Err(CoordinatorError::OwnerRejected),
        }
    }
    fn apply(&mut self, effect: PrivilegedEffect) -> Result<(), CoordinatorError> {
        let (binding, sequence) = self
            .approved
            .clone()
            .ok_or(CoordinatorError::ReviewRequired)?;
        match self
            .request(Request::Apply {
                binding: binding.clone(),
                sequence,
                effect,
            })
            .map_err(|_| CoordinatorError::OwnerRejected)?
        {
            Response::Complete => {
                self.approved = Some((binding, sequence + 1));
                Ok(())
            }
            _ => Err(CoordinatorError::OwnerRejected),
        }
    }
    fn trust_descriptor(&mut self) -> Result<String, CoordinatorError> {
        match self
            .request(Request::Verify)
            .map_err(|_| CoordinatorError::OwnerRejected)?
        {
            Response::Ready { descriptor, .. } => Ok(descriptor),
            _ => Err(CoordinatorError::NotReady),
        }
    }
}
impl Drop for NativeHostOwners {
    fn drop(&mut self) {
        if let Some((mut child, _, _)) = self.session.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
