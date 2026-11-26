use sdk::json::Json;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

mod aim;
mod arrange;
mod catalogue;
mod harmony;
mod http;
mod library;
mod notes;
mod path;
mod presets;
mod rpc;
mod tools;
mod write;

pub(crate) use aim::sampled;
pub(crate) use arrange::counted;
pub use library::{Shelf, whole};
pub use tools::{Host, Listening, browsing, housed, listening, serve};
pub use write::{Revisions, Shift};

pub const DEFAULT_PORT: u16 = 9393;
pub const LEAST_PORT: u16 = 1024;
pub const ROUTE: &str = "/mcp";
pub const LISTEN: &str = "listen";
pub const LIBRARY: &str = "library";
pub const EXPORT: &str = "export";
const PATIENCE: Duration = Duration::from_secs(10);
const RENDER_PATIENCE: Duration = Duration::from_secs(600);
const CLAIM_PATIENCE: Duration = Duration::from_millis(50);
const POKE: Duration = Duration::from_secs(1);
const MOST_REPLY_BYTES: usize = 40_000;
const CLOSED: &str = "Baking Powder has closed its server";
const WAITING: u8 = 0;
const CLAIMED: u8 = 1;
const ABANDONED: u8 = 2;

pub fn edits(tool: &str) -> bool {
    catalogue::edits(tool)
}

pub fn address(port: u16) -> String {
    format!("http://127.0.0.1:{port}{ROUTE}")
}

pub fn setup(port: u16, asking: bool) -> String {
    let address = address(port);
    let ask = if asking { format!(" {}", catalogue::ASK) } else { String::new() };
    format!("Baking Powder MCP: streamable HTTP at {address}; local only, JSON-RPC POST, no token. The app serves it while the user has it open: if it does not connect, say so and stop. Never open, launch, quit or automate Baking Powder. Reconnect after setup changes. Call read first; manual provides help.{ask}\n\nJSON config: {{\"mcpServers\": {{\"baking-powder\": {{\"type\": \"http\", \"url\": \"{address}\"}}}}}}\n\nTOML config:\n[mcp_servers.baking-powder]\nurl = \"{address}\"\n")
}

#[derive(Default)]
pub struct Status {
    client: Option<String>,
    calls: u64,
    last: Option<(String, Instant)>,
}

impl Status {
    pub fn connected(&mut self, client: String) {
        *self = Status { client: Some(client), ..Status::default() };
    }

    pub fn called(&mut self, tool: &str) {
        self.calls += 1;
        self.last = Some((String::from(tool), Instant::now()));
    }

    pub fn client(&self) -> Option<&str> {
        self.client.as_deref()
    }

    pub fn calls(&self) -> u64 {
        self.calls
    }

    pub fn last(&self) -> Option<(&str, Duration)> {
        self.last.as_ref().map(|(tool, at)| (tool.as_str(), at.elapsed()))
    }
}

pub struct Call {
    pub tool: String,
    pub args: Json,
    reply: Sender<Reply>,
    state: Arc<AtomicU8>,
}

impl Call {
    pub fn new(tool: &str, args: Json) -> (Call, Waiting) {
        let (reply, replied) = mpsc::channel();
        let state = Arc::new(AtomicU8::new(WAITING));
        (Call { tool: String::from(tool), args, reply, state: state.clone() }, Waiting { replied, state })
    }

    pub fn awaited(&self) -> bool {
        self.state.load(Ordering::SeqCst) != ABANDONED
    }

    pub fn claim(&self) -> bool {
        self.state.compare_exchange(WAITING, CLAIMED, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }

    pub fn release(&self) {
        let _ = self.state.compare_exchange(CLAIMED, WAITING, Ordering::SeqCst, Ordering::SeqCst);
    }

    pub fn answer(self, reply: Reply) {
        let _ = self.reply.send(reply);
    }
}

pub struct Waiting {
    replied: Receiver<Reply>,
    state: Arc<AtomicU8>,
}

impl Waiting {
    pub fn reply(self, patience: Duration) -> Reply {
        let mut wait = patience;
        loop {
            match self.replied.recv_timeout(wait) {
                Ok(reply) => return reply,
                Err(RecvTimeoutError::Disconnected) => return Reply::refused(CLOSED),
                Err(RecvTimeoutError::Timeout) if self.state.compare_exchange(WAITING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst).is_ok() => {
                    return Reply::refused("Baking Powder did not answer in time: an edit waits while the user drags or an export renders in the app, and the calls behind it wait too, so try again");
                }
                Err(RecvTimeoutError::Timeout) => wait = CLAIM_PATIENCE,
            }
        }
    }
}

pub struct Reply {
    pub text: String,
    pub aside: Option<String>,
    pub failed: bool,
}

impl Reply {
    pub fn ok(text: impl Into<String>) -> Reply {
        Reply { text: text.into(), aside: None, failed: false }
    }

    pub fn refused(text: impl Into<String>) -> Reply {
        Reply { text: text.into(), aside: None, failed: true }
    }
}

pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    asking: Arc<AtomicBool>,
    active: Arc<Mutex<Option<TcpStream>>>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn ask(&self, asking: bool) {
        self.asking.store(asking, Ordering::Relaxed);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        self.stop.store(true, Ordering::Relaxed);
        if let Some(stream) = active.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        drop(active);
        let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], self.port)), POKE);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn listen(port: u16, asking: bool) -> Option<(Server, Receiver<Call>)> {
    let listener = TcpListener::bind(("127.0.0.1", port)).ok()?;
    let (calls, received) = mpsc::channel();
    let (stop, active, asking) = (Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)), Arc::new(AtomicBool::new(asking)));
    let (stopping, held, asked) = (stop.clone(), active.clone(), asking.clone());
    let thread = std::thread::Builder::new()
        .name(String::from("bp-mcp"))
        .spawn(move || {
            for stream in listener.incoming() {
                let mut watched = held.lock().unwrap_or_else(PoisonError::into_inner);
                if stopping.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else {
                    continue;
                };
                *watched = stream.try_clone().ok();
                drop(watched);
                http::answer(stream, asked.load(Ordering::Relaxed), &|tool, args| forward(&calls, tool, args));
                *held.lock().unwrap_or_else(PoisonError::into_inner) = None;
            }
        })
        .ok()?;
    Some((Server { port, stop, asking, active, thread: Some(thread) }, received))
}

fn forward(calls: &Sender<Call>, tool: &str, args: &Json) -> Reply {
    let (call, waiting) = Call::new(tool, args.clone());
    if calls.send(call).is_err() {
        return Reply::refused(CLOSED);
    }
    waiting.reply(if tool == LISTEN || tool == EXPORT { RENDER_PATIENCE } else { PATIENCE })
}

pub fn object(fields: Vec<(&str, Json)>) -> Json {
    Json::Object(fields.into_iter().map(|(key, value)| (String::from(key), value)).collect())
}

pub fn text(text: &str) -> Json {
    Json::String(String::from(text))
}
