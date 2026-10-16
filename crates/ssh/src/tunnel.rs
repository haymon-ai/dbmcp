//! Loopback listener forwarding each socket over a `direct-tcpip` channel.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use dbmcp_config::{SshConfig, StrictHostKeyChecking};
use russh::client;
use russh::keys::PublicKeyOrCertificate;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::{JoinHandle, JoinSet};
use tracing::{debug, error, info, warn};

use crate::error::SshError;

/// Deadline for TCP dial, SSH handshake, and authentication together.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Pause after a failed `accept` before retrying.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);

/// Entry point for starting an SSH tunnel.
#[derive(Debug)]
pub struct Tunnel;

impl Tunnel {
    /// Binds the local listener and starts forwarding to `db_host:db_port`.
    ///
    /// Binds `127.0.0.1` on `config.local_port` (ephemeral when unset). The
    /// SSH session is opened lazily on the first accepted socket, so an
    /// unreachable SSH server does not fail startup. `db_host` is resolved
    /// by the SSH server, not locally.
    ///
    /// # Errors
    ///
    /// - [`SshError::NotImplemented`] for key authentication or host-key
    ///   checking other than `no`.
    /// - [`SshError::MissingPassword`] when no password is configured.
    /// - [`SshError::Bind`] when the local listener cannot be bound.
    pub async fn start(config: &SshConfig, db_host: &str, db_port: u16) -> Result<TunnelHandle, SshError> {
        if config.key.is_some() {
            return Err(SshError::NotImplemented(
                "SSH private-key authentication (DB_SSH_KEY)".into(),
            ));
        }
        if config.strict_host_key_checking != StrictHostKeyChecking::No {
            return Err(SshError::NotImplemented(format!(
                "SSH host-key verification (DB_SSH_STRICT_HOST_KEY_CHECKING={})",
                config.strict_host_key_checking
            )));
        }
        let password = config.password.clone().ok_or(SshError::MissingPassword)?;

        warn!(
            "SSH host-key verification is DISABLED (DB_SSH_STRICT_HOST_KEY_CHECKING=no); \
             the tunnel is open to man-in-the-middle attacks"
        );

        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, config.local_port.unwrap_or(0)));
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|source| SshError::Bind { addr, source })?;
        let local_addr = listener
            .local_addr()
            .map_err(|source| SshError::Bind { addr, source })?;

        let inner = Arc::new(Inner {
            client_config: Arc::new(client::Config {
                inactivity_timeout: None,
                nodelay: true,
                ..client::Config::default()
            }),
            ssh_target: format!("{}:{}", config.host, config.port),
            ssh_host: config.host.clone(),
            ssh_port: config.port,
            user: config.user.clone(),
            password,
            db_target: format!("{db_host}:{db_port}"),
            db_host: db_host.to_owned(),
            db_port,
            session: Mutex::new(None),
        });

        info!(
            "SSH tunnel listening on {local_addr} via {}@{}",
            inner.user, inner.ssh_target
        );

        let accept_task = tokio::spawn(accept_loop(listener, Arc::clone(&inner)));

        Ok(TunnelHandle {
            local_addr,
            inner,
            accept_task,
        })
    }
}

/// Running tunnel; dropping it stops forwarding.
///
/// Drop aborts the accept loop and every in-flight channel task, then
/// sends a best-effort SSH disconnect without blocking.
#[derive(Debug)]
pub struct TunnelHandle {
    local_addr: SocketAddr,
    inner: Arc<Inner>,
    accept_task: JoinHandle<()>,
}

impl TunnelHandle {
    /// Returns the loopback address database drivers should connect to.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

impl Drop for TunnelHandle {
    fn drop(&mut self) {
        self.accept_task.abort();

        let session = self.inner.session.try_lock().ok().and_then(|mut guard| guard.take());
        if let Some(session) = session
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                let _ = session
                    .disconnect(russh::Disconnect::ByApplication, "dbmcp shutting down", "en")
                    .await;
            });
        }
        info!("SSH tunnel on {} closed", self.local_addr);
    }
}

/// State shared between the accept loop and channel tasks.
struct Inner {
    client_config: Arc<client::Config>,
    ssh_target: String,
    ssh_host: String,
    ssh_port: u16,
    user: String,
    password: String,
    db_target: String,
    db_host: String,
    db_port: u16,
    session: Mutex<Option<Arc<client::Handle<Client>>>>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("ssh_target", &self.ssh_target)
            .field("user", &self.user)
            .field("password", &"[REDACTED]")
            .field("db_target", &self.db_target)
            .finish_non_exhaustive()
    }
}

impl Inner {
    /// Returns the shared SSH session, opening it on first use.
    async fn session(&self) -> Result<Arc<client::Handle<Client>>, SshError> {
        let mut guard = self.session.lock().await;
        if let Some(session) = guard.as_ref() {
            return Ok(Arc::clone(session));
        }

        let session = tokio::time::timeout(CONNECT_TIMEOUT, self.connect())
            .await
            .map_err(|_| SshError::Timeout {
                target: self.ssh_target.clone(),
                secs: CONNECT_TIMEOUT.as_secs(),
            })??;
        info!("SSH session established to {}@{}", self.user, self.ssh_target);

        let session = Arc::new(session);
        *guard = Some(Arc::clone(&session));
        Ok(session)
    }

    /// Dials the SSH server and authenticates with the password.
    async fn connect(&self) -> Result<client::Handle<Client>, SshError> {
        let mut session = client::connect(
            Arc::clone(&self.client_config),
            (self.ssh_host.as_str(), self.ssh_port),
            Client,
        )
        .await
        .map_err(|source| SshError::Connect {
            target: self.ssh_target.clone(),
            source,
        })?;

        let auth = session
            .authenticate_password(self.user.as_str(), self.password.as_str())
            .await
            .map_err(|source| SshError::Auth {
                user: self.user.clone(),
                target: self.ssh_target.clone(),
                source,
            })?;
        if !auth.success() {
            return Err(SshError::AuthRejected {
                user: self.user.clone(),
                target: self.ssh_target.clone(),
            });
        }
        Ok(session)
    }
}

/// Accepts loopback sockets and forwards each on its own task.
///
/// Channel tasks live in a [`JoinSet`] owned by this future, so aborting
/// the accept task also aborts every in-flight channel.
async fn accept_loop(listener: TcpListener, inner: Arc<Inner>) {
    let mut channels = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((socket, peer)) => {
                    channels.spawn(forward(Arc::clone(&inner), socket, peer));
                }
                Err(e) => {
                    // Persistent errors (e.g. EMFILE) would otherwise spin.
                    warn!("SSH tunnel accept failed: {e}");
                    tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                }
            },
            Some(_) = channels.join_next() => {}
        }
    }
}

/// Pipes one accepted socket through a new `direct-tcpip` channel.
async fn forward(inner: Arc<Inner>, mut socket: TcpStream, peer: SocketAddr) {
    let session = match inner.session().await {
        Ok(session) => session,
        Err(e) => {
            error!("SSH tunnel unavailable: {e}");
            return;
        }
    };

    let channel = match session
        .channel_open_direct_tcpip(
            inner.db_host.as_str(),
            u32::from(inner.db_port),
            peer.ip().to_string(),
            u32::from(peer.port()),
        )
        .await
    {
        Ok(channel) => channel,
        Err(source) => {
            let e = SshError::ChannelOpen {
                target: inner.db_target.clone(),
                source,
            };
            error!("{e}");
            return;
        }
    };
    debug!("SSH channel opened for {peer} to {}", inner.db_target);

    let mut stream = channel.into_stream();
    match tokio::io::copy_bidirectional(&mut socket, &mut stream).await {
        Ok((sent, received)) => {
            debug!("SSH channel for {peer} closed ({sent} bytes sent, {received} bytes received)");
        }
        Err(e) => debug!("SSH channel for {peer} closed with error: {e}"),
    }
}

/// russh client callbacks.
#[derive(Debug)]
struct Client;

impl client::Handler for Client {
    type Error = russh::Error;

    /// Accepts every host key; only reachable with checking set to `no`.
    async fn check_server_key(&mut self, _server_public_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use russh::keys::PrivateKey;
    use russh::keys::ssh_key::private::Ed25519Keypair;
    use russh::server::{self, Auth, ChannelOpenHandle, Msg, Server as _, Session};
    use russh::{Channel, MethodKind, MethodSet};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    const USER: &str = "tunnel";
    const PASSWORD: &str = "s3cret";

    /// In-process SSH server accepting password auth and `direct-tcpip`.
    #[derive(Clone, Default)]
    struct TestServer {
        sessions: Arc<AtomicUsize>,
        targets: Arc<std::sync::Mutex<Vec<(String, u32)>>>,
    }

    impl server::Server for TestServer {
        type Handler = Self;

        fn new_client(&mut self, _: Option<SocketAddr>) -> Self {
            self.sessions.fetch_add(1, Ordering::SeqCst);
            self.clone()
        }
    }

    impl server::Handler for TestServer {
        type Error = russh::Error;

        async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
            if user == USER && password == PASSWORD {
                Ok(Auth::Accept)
            } else {
                Ok(Auth::reject())
            }
        }

        async fn channel_open_direct_tcpip(
            &mut self,
            channel: Channel<Msg>,
            host_to_connect: &str,
            port_to_connect: u32,
            _originator_address: &str,
            _originator_port: u32,
            reply: ChannelOpenHandle,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            self.targets
                .lock()
                .expect("targets lock")
                .push((host_to_connect.to_owned(), port_to_connect));
            let port = u16::try_from(port_to_connect).expect("port fits u16");
            let mut upstream = TcpStream::connect((host_to_connect, port)).await?;
            reply.accept().await;
            tokio::spawn(async move {
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
            });
            Ok(())
        }
    }

    /// Starts the SSH test server on an ephemeral loopback port.
    async fn start_ssh_server() -> (SocketAddr, TestServer) {
        let host_key = PrivateKey::from(Ed25519Keypair::from_seed(&[7; 32]));
        let config = Arc::new(server::Config {
            keys: vec![host_key],
            methods: MethodSet::from(&[MethodKind::Password][..]),
            auth_rejection_time: Duration::from_millis(10),
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..server::Config::default()
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind ssh");
        let addr = listener.local_addr().expect("ssh addr");
        let mut server = TestServer::default();
        let handle = server.clone();
        tokio::spawn(async move {
            let _ = server.run_on_socket(config, &listener).await;
        });
        (addr, handle)
    }

    /// Starts a TCP echo server standing in for the database.
    async fn start_echo_server() -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind echo");
        let addr = listener.local_addr().expect("echo addr");
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (mut reader, mut writer) = socket.split();
                    let _ = tokio::io::copy(&mut reader, &mut writer).await;
                });
            }
        });
        addr
    }

    fn ssh_config(ssh: SocketAddr) -> SshConfig {
        SshConfig {
            host: ssh.ip().to_string(),
            port: ssh.port(),
            user: USER.into(),
            password: Some(PASSWORD.into()),
            key: None,
            key_passphrase: None,
            known_hosts: None,
            strict_host_key_checking: StrictHostKeyChecking::No,
            keepalive_interval: Duration::ZERO,
            local_port: None,
        }
    }

    async fn round_trip(addr: SocketAddr, payload: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut socket = TcpStream::connect(addr).await?;
        socket.write_all(payload).await?;
        let mut buf = vec![0; payload.len()];
        socket.read_exact(&mut buf).await?;
        Ok(buf)
    }

    #[tokio::test]
    async fn forwards_bytes_through_direct_tcpip_channel() {
        let (ssh, server) = start_ssh_server().await;
        let echo = start_echo_server().await;
        let tunnel = Tunnel::start(&ssh_config(ssh), "127.0.0.1", echo.port())
            .await
            .expect("tunnel starts");

        assert!(tunnel.local_addr().ip().is_loopback());
        assert_eq!(round_trip(tunnel.local_addr(), b"ping").await.expect("echo"), b"ping");
        assert_eq!(round_trip(tunnel.local_addr(), b"pong").await.expect("echo"), b"pong");

        assert_eq!(server.sessions.load(Ordering::SeqCst), 1, "sockets share one session");
        let targets = server.targets.lock().expect("targets lock").clone();
        assert_eq!(
            targets,
            vec![
                ("127.0.0.1".to_owned(), u32::from(echo.port())),
                ("127.0.0.1".to_owned(), u32::from(echo.port()))
            ],
            "one channel per socket"
        );
    }

    #[tokio::test]
    async fn opens_session_lazily_on_first_socket() {
        let (ssh, server) = start_ssh_server().await;
        let echo = start_echo_server().await;
        let tunnel = Tunnel::start(&ssh_config(ssh), "127.0.0.1", echo.port())
            .await
            .expect("tunnel starts");

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            server.sessions.load(Ordering::SeqCst),
            0,
            "no session before first socket"
        );

        round_trip(tunnel.local_addr(), b"x").await.expect("echo");
        assert_eq!(server.sessions.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn starts_even_when_ssh_server_is_unreachable() {
        let closed = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind")
            .local_addr()
            .expect("addr");
        let tunnel = Tunnel::start(&ssh_config(closed), "127.0.0.1", 5432)
            .await
            .expect("listener binds without contacting ssh");

        let mut socket = TcpStream::connect(tunnel.local_addr()).await.expect("connect");
        let mut buf = [0; 1];
        let read = socket.read(&mut buf).await;
        assert!(matches!(read, Ok(0) | Err(_)), "socket must close: {read:?}");
    }

    #[tokio::test]
    async fn closes_socket_when_password_is_rejected() {
        let (ssh, _server) = start_ssh_server().await;
        let echo = start_echo_server().await;
        let config = SshConfig {
            password: Some("wrong".into()),
            ..ssh_config(ssh)
        };
        let tunnel = Tunnel::start(&config, "127.0.0.1", echo.port())
            .await
            .expect("tunnel starts");

        let read = round_trip(tunnel.local_addr(), b"ping").await;
        assert!(read.is_err(), "rejected auth must not forward: {read:?}");
    }

    #[tokio::test]
    async fn binds_requested_local_port() {
        let (ssh, _server) = start_ssh_server().await;
        let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();
        let config = SshConfig {
            local_port: Some(free),
            ..ssh_config(ssh)
        };
        let tunnel = Tunnel::start(&config, "127.0.0.1", 5432).await.expect("tunnel starts");
        assert_eq!(tunnel.local_addr(), SocketAddr::from((Ipv4Addr::LOCALHOST, free)));
    }

    #[tokio::test]
    async fn bind_failure_is_an_error() {
        let (ssh, _server) = start_ssh_server().await;
        let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind");
        let config = SshConfig {
            local_port: Some(taken.local_addr().expect("addr").port()),
            ..ssh_config(ssh)
        };
        let err = Tunnel::start(&config, "127.0.0.1", 5432)
            .await
            .expect_err("port in use");
        assert!(matches!(err, SshError::Bind { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn drop_stops_listener() {
        let (ssh, _server) = start_ssh_server().await;
        let tunnel = Tunnel::start(&ssh_config(ssh), "127.0.0.1", 5432)
            .await
            .expect("tunnel starts");
        let addr = tunnel.local_addr();
        drop(tunnel);

        for _ in 0..50 {
            if TcpStream::connect(addr).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("listener on {addr} still accepting after drop");
    }

    #[tokio::test]
    async fn key_auth_is_not_yet_implemented() {
        let config = SshConfig {
            key: Some(PathBuf::from("/home/deploy/.ssh/id_ed25519")),
            ..ssh_config(SocketAddr::from((Ipv4Addr::LOCALHOST, 22)))
        };
        let err = Tunnel::start(&config, "db", 5432)
            .await
            .expect_err("key auth unsupported");
        assert_eq!(
            err.to_string(),
            "SSH private-key authentication (DB_SSH_KEY) is not yet implemented"
        );
    }

    #[tokio::test]
    async fn host_key_verification_is_not_yet_implemented() {
        for (mode, value) in [
            (StrictHostKeyChecking::Yes, "yes"),
            (StrictHostKeyChecking::AcceptNew, "accept-new"),
        ] {
            let config = SshConfig {
                strict_host_key_checking: mode,
                ..ssh_config(SocketAddr::from((Ipv4Addr::LOCALHOST, 22)))
            };
            let err = Tunnel::start(&config, "db", 5432).await.expect_err("mode unsupported");
            assert_eq!(
                err.to_string(),
                format!("SSH host-key verification (DB_SSH_STRICT_HOST_KEY_CHECKING={value}) is not yet implemented")
            );
        }
    }

    #[test]
    fn inner_debug_redacts_password() {
        let inner = Inner {
            client_config: Arc::new(client::Config::default()),
            ssh_target: "bastion:22".into(),
            ssh_host: "bastion".into(),
            ssh_port: 22,
            user: USER.into(),
            password: "hunter2".into(),
            db_target: "db:5432".into(),
            db_host: "db".into(),
            db_port: 5432,
            session: Mutex::new(None),
        };
        let debug = format!("{inner:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("[REDACTED]"), "{debug}");
    }
}
