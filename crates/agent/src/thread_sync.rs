use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::{Result, anyhow};
use async_channel::{Receiver, Sender};
use collections::HashMap;
use gpui::{App, AppContext, AsyncApp, BackgroundExecutor, Context, Entity, EventEmitter, Global, Task};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use smol::io::{AsyncReadExt, AsyncWriteExt};
use smol::net::{TcpListener, TcpStream};

/// Maximum size of a single framed message. Stream deltas are small; this only
/// exists to fail loudly instead of ballooning memory on a corrupt peer.
const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

/// A streamed assistant-token delta carried across the local sync bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThreadSyncStreamEvent {
    Text(String),
    Thinking { text: String, signature: Option<String> },
    Stop(String),
}

/// A message on the local cross-instance thread sync bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThreadSyncMessage {
    Stream {
        session_id: String,
        event: ThreadSyncStreamEvent,
    },
}

/// Emitted on the foreground when a peer publishes a message.
#[derive(Debug, Clone)]
pub enum ThreadSyncBusEvent {
    Message(ThreadSyncMessage),
}

impl EventEmitter<ThreadSyncBusEvent> for ThreadSyncBus {}

struct GlobalThreadSyncBus(Entity<ThreadSyncBus>);

impl Global for GlobalThreadSyncBus {}

/// The local inter-process bus that broadcasts streamed thread content between
/// Zed instances. One process acts as the hub and the rest connect as clients;
/// the hub forwards every message to the other peers without echoing it back to
/// its sender.
pub struct ThreadSyncBus {
    outbound_tx: Sender<ThreadSyncMessage>,
    _pump_task: Task<()>,
    _network_task: Task<()>,
}

/// First frame a client sends to the hub to prove it knows the shared secret.
#[derive(Serialize, Deserialize)]
struct Hello {
    secret: String,
}

#[derive(Serialize, Deserialize)]
struct ControlFile {
    port: u16,
    secret: String,
}

enum Role {
    Hub { listener: TcpListener, secret: String },
    Client { stream: TcpStream, secret: String },
}

impl ThreadSyncBus {
    pub fn init_global(cx: &mut App) {
        if cx.try_global::<GlobalThreadSyncBus>().is_some() {
            return;
        }
        let bus = cx.new(|cx| Self::new(cx));
        cx.set_global(GlobalThreadSyncBus(bus));
    }

    pub fn try_global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalThreadSyncBus>()
            .map(|store| store.0.clone())
    }

    fn new(cx: &mut Context<Self>) -> Self {
        let (outbound_tx, outbound_rx) = async_channel::unbounded();
        let (inbound_tx, inbound_rx) = async_channel::unbounded();

        let pump_task = cx.spawn(async move |this, cx| {
            while let Ok(message) = inbound_rx.recv().await {
                if this
                    .update(cx, |_bus, cx| cx.emit(ThreadSyncBusEvent::Message(message)))
                    .is_err()
                {
                    return;
                }
            }
        });

        let executor = cx.background_executor().clone();
        let network_task = executor.clone().spawn(async move {
            Self::run_network(executor, outbound_rx, inbound_tx).await;
        });

        Self {
            outbound_tx,
            _pump_task: pump_task,
            _network_task: network_task,
        }
    }

    /// Publishes a stream event to the other instances. Non-blocking; a peer
    /// that is temporarily unreachable simply drops the message (streaming is
    /// best-effort and the completed turn is recovered from disk).
    pub fn broadcast_stream(&self, session_id: String, event: ThreadSyncStreamEvent) {
        self.outbound_tx
            .try_send(ThreadSyncMessage::Stream { session_id, event })
            .ok();
    }

    /// Publishes a stream event from an async context, if the bus has been
    /// initialized. No-op when the bus is absent (tests, or before init).
    pub fn broadcast_stream_global(cx: &AsyncApp, session_id: String, event: ThreadSyncStreamEvent) {
        if !cx.has_global::<GlobalThreadSyncBus>() {
            return;
        }
        cx.read_global::<GlobalThreadSyncBus, ()>(|bus, app| {
            bus.0.read(app).broadcast_stream(session_id, event)
        });
    }

    async fn run_network(
        executor: BackgroundExecutor,
        outbound_rx: Receiver<ThreadSyncMessage>,
        inbound_tx: Sender<ThreadSyncMessage>,
    ) {
        loop {
            match acquire_role().await {
                Ok(Role::Hub { listener, secret }) => {
                    log::info!("[THREAD_SYNC] acting as sync hub");
                    Self::run_hub(&executor, listener, secret, &outbound_rx, &inbound_tx).await;
                }
                Ok(Role::Client { stream, secret }) => {
                    log::info!("[THREAD_SYNC] connected to sync hub");
                    Self::run_client(&executor, stream, secret, &outbound_rx, &inbound_tx).await;
                }
                Err(error) => {
                    log::warn!("[THREAD_SYNC] sync setup failed: {error:#}");
                }
            }

            executor.timer(Duration::from_secs(1)).await;
        }
    }

    async fn run_hub(
        executor: &BackgroundExecutor,
        listener: TcpListener,
        secret: String,
        outbound_rx: &Receiver<ThreadSyncMessage>,
        inbound_tx: &Sender<ThreadSyncMessage>,
    ) {
        let clients: Arc<Mutex<HashMap<u64, Sender<ThreadSyncMessage>>>> =
            Arc::new(Mutex::new(HashMap::default()));

        // Forward this process's own messages to every client. They were
        // already applied locally by the normal stream path, so no local emit.
        let outbound_rx = outbound_rx.clone();
        let pump_clients = clients.clone();
        executor
            .spawn(async move {
                while let Ok(message) = outbound_rx.recv().await {
                    for client in pump_clients.lock().values() {
                        client.try_send(message.clone()).ok();
                    }
                }
            })
            .detach();

        let mut next_client_id: u64 = 0;
        loop {
            let Ok((stream, _addr)) = listener.accept().await else {
                return;
            };

            // A client must authenticate with the shared secret before it can
            // read or write anything else.
            let mut auth_stream = stream.clone();
            let Ok(hello) = read_frame::<Hello>(&mut auth_stream).await else {
                continue;
            };
            if hello.secret != secret {
                continue;
            }

            let client_id = next_client_id;
            next_client_id += 1;

            let (client_tx, client_rx) = async_channel::unbounded();
            clients.lock().insert(client_id, client_tx);

            // Writer: drain this client's outbound queue into its socket.
            let mut write_stream = stream.clone();
            executor
                .spawn(async move {
                    while let Ok(message) = client_rx.recv().await {
                        if write_frame(&mut write_stream, &message).await.is_err() {
                            return;
                        }
                    }
                })
                .detach();

            // Reader: forward this client's messages to the local pump and to
            // the other clients (but not back to the sender).
            let mut read_stream = stream;
            let inbound_tx = inbound_tx.clone();
            let reader_clients = clients.clone();
            executor
                .spawn(async move {
                    loop {
                        let Ok(message) = read_frame::<ThreadSyncMessage>(&mut read_stream).await
                        else {
                            return;
                        };
                        inbound_tx.send(message.clone()).await.ok();
                        for (id, client) in reader_clients.lock().iter() {
                            if *id != client_id {
                                client.try_send(message.clone()).ok();
                            }
                        }
                    }
                })
                .detach();
        }
    }

    async fn run_client(
        executor: &BackgroundExecutor,
        mut stream: TcpStream,
        secret: String,
        outbound_rx: &Receiver<ThreadSyncMessage>,
        inbound_tx: &Sender<ThreadSyncMessage>,
    ) {
        if write_frame(&mut stream, &Hello { secret }).await.is_err() {
            return;
        }

        // Writer: publish this process's messages to the hub. They were already
        // applied locally by the normal stream path, so no local emit.
        let mut write_stream = stream.clone();
        let outbound_rx = outbound_rx.clone();
        let writer = executor.spawn(async move {
            while let Ok(message) = outbound_rx.recv().await {
                if write_frame(&mut write_stream, &message).await.is_err() {
                    return;
                }
            }
        });

        // Reader: run inline so `run_network` knows when the hub dropped us.
        loop {
            match read_frame::<ThreadSyncMessage>(&mut stream).await {
                Ok(message) => {
                    inbound_tx.send(message).await.ok();
                }
                Err(_) => {
                    drop(writer);
                    return;
                }
            }
        }
    }
}

fn control_path() -> PathBuf {
    paths::data_dir().join("threads").join("sync.json")
}

async fn acquire_role() -> Result<Role> {
    let path = control_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    loop {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await?;
        let port = listener.local_addr()?.port();
        let secret = uuid::Uuid::new_v4().to_string();

        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                let control = ControlFile {
                    port,
                    secret: secret.clone(),
                };
                if let Err(error) = serde_json::to_writer(&mut file, &control) {
                    let _ = std::fs::remove_file(&path);
                    return Err(error.into());
                }
                return Ok(Role::Hub { listener, secret });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // A control file exists. Verify the claimed hub is alive before
                // joining it; otherwise clear the stale file and retry.
                let Ok(data) = std::fs::read_to_string(&path) else {
                    let _ = std::fs::remove_file(&path);
                    continue;
                };
                let Ok(control) = serde_json::from_str::<ControlFile>(&data) else {
                    let _ = std::fs::remove_file(&path);
                    continue;
                };
                let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), control.port);
                match TcpStream::connect(addr).await {
                    Ok(stream) => {
                        drop(listener);
                        return Ok(Role::Client {
                            stream,
                            secret: control.secret,
                        });
                    }
                    Err(_) => {
                        drop(listener);
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
}

async fn write_frame<S: Serialize>(stream: &mut TcpStream, value: &S) -> Result<()> {
    let json = serde_json::to_vec(value)?;
    let len = json.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&json).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_frame<D: DeserializeOwned>(stream: &mut TcpStream) -> Result<D> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return Err(anyhow!("sync frame too large: {len} bytes"));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(serde_json::from_slice(&buf)?)
}
