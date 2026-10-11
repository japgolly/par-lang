use super::readback::Handle;
use crate::flat::arena::Arena;
use crate::flat::runtime::{Node, Runtime, UserData};
use crate::linker::Linked;
use futures::future::RemoteHandle;
use futures::stream::{FuturesUnordered, StreamExt};
use futures::task::{FutureObj, Spawn, SpawnExt};
use std::future::poll_fn;
use std::sync::{Arc, Weak};
use std::task::Poll;
use std::time::Instant;
use tokio::sync::mpsc;

pub enum ReducerMessage {
    Redex(Node<Linked>, Node<Linked>),
    Spawn(FutureObj<'static, ()>),
}

/// What handles need to reach their net: the reducer's inbox and the arena.
///
/// It's shared by all handles of a net, so that cloning a handle is cheap.
pub struct NetShared {
    sender: mpsc::UnboundedSender<ReducerMessage>,
    pub(crate) arena: Arc<Arena<Linked>>,
}

#[derive(Clone)]
pub struct NetHandle(Arc<NetShared>);

impl NetHandle {
    pub(crate) fn send(&self, msg: ReducerMessage) {
        self.0.sender.send(msg).unwrap()
    }

    pub(crate) fn arena(&self) -> &Arc<Arena<Linked>> {
        &self.0.arena
    }
}

pub(crate) struct Reducer {
    pub runtime: Runtime,
    measure_net_duration: bool,
    spawner: Arc<dyn Spawn + Send + Sync>,
    inbox: mpsc::UnboundedReceiver<ReducerMessage>,
    // The reducer doesn't keep its inbox open by itself: once all handles are
    // dropped and the inbox is drained, the net is done.
    net: Weak<NetShared>,
    // External calls remain independent futures, but are cooperatively polled
    // by the reducer instead of allocating a Tokio task for every call.
    external_tasks: FuturesUnordered<super::runtime::ExternalFnRet>,
}

impl Reducer {
    pub(crate) fn from(
        runtime: Runtime,
        spawner: Arc<dyn Spawn + Send + Sync + 'static>,
        measure_net_duration: bool,
    ) -> (Self, NetHandle) {
        let (tx, rx) = mpsc::unbounded_channel();
        let net = Arc::new(NetShared {
            sender: tx,
            arena: runtime.arena.clone(),
        });
        (
            Self {
                runtime,
                measure_net_duration,
                spawner,
                inbox: rx,
                net: Arc::downgrade(&net),
                external_tasks: FuturesUnordered::new(),
            },
            NetHandle(net),
        )
    }
    // this function should only be called inside run, to avoid race conditions
    fn net_handle(&mut self) -> NetHandle {
        if let Some(net) = self.net.upgrade() {
            return NetHandle(net);
        }
        // all handles have been dropped, so we can just create a new channel
        let (tx, rx) = mpsc::unbounded_channel();
        // forward the old messages to the new channel
        loop {
            match self.inbox.try_recv() {
                Ok(msg) => {
                    tx.send(msg).unwrap();
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    // The last sender is still in the middle of being dropped.
                    std::thread::yield_now();
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // it's guaranteed there will never be another message
                    break;
                }
            }
        }
        self.inbox = rx;
        let net = Arc::new(NetShared {
            sender: tx,
            arena: self.runtime.arena.clone(),
        });
        self.net = Arc::downgrade(&net);
        NetHandle(net)
    }
    fn handle_message(&mut self, msg: ReducerMessage) {
        match msg {
            ReducerMessage::Redex(a, b) => {
                self.runtime.redexes.push((a, b));
            }
            ReducerMessage::Spawn(s) => {
                self.spawner.spawn_obj(s).unwrap();
            }
        }
    }
    async fn schedule_external(&mut self, mut task: super::runtime::ExternalFnRet) {
        let pending =
            poll_fn(|context| Poll::Ready(task.as_mut().poll(context).is_pending())).await;
        if pending {
            self.external_tasks.push(task);
        }
    }
    pub(crate) async fn run(&mut self) {
        let mut inbox_closed = false;
        loop {
            loop {
                if !self.runtime.redexes.is_empty() {
                    #[cfg(not(target_family = "wasm"))]
                    let reduction = if self.measure_net_duration {
                        let start = Instant::now();
                        let reduction = self.runtime.reduce();
                        self.runtime.rewrites.net_duration += start.elapsed();
                        reduction
                    } else {
                        self.runtime.reduce()
                    };
                    #[cfg(target_family = "wasm")]
                    let reduction = self.runtime.reduce();

                    if let Some((a, b)) = reduction {
                        match (a, b) {
                            (UserData::ExternalFn(f), other) => {
                                let handle = Handle::from_node(self.net_handle(), other);
                                self.schedule_external(f(handle.into())).await;
                            }
                            (UserData::ExternalArc(f), other) => {
                                let handle = Handle::from_node(self.net_handle(), other);
                                self.schedule_external((f.0).as_ref()(handle.into())).await;
                            }
                        }
                    }
                } else {
                    match self.inbox.try_recv() {
                        Ok(msg) => {
                            self.handle_message(msg);
                        }
                        _ => {
                            break;
                        }
                    }
                }
            }
            if self.external_tasks.is_empty() {
                if inbox_closed {
                    break;
                }
                match self.inbox.recv().await {
                    Some(msg) => {
                        self.handle_message(msg);
                    }
                    None => {
                        inbox_closed = true;
                    }
                }
            } else if inbox_closed {
                self.external_tasks.next().await;
            } else {
                tokio::select! {
                    msg = self.inbox.recv() => {
                        if let Some(msg) = msg {
                            self.handle_message(msg);
                        } else {
                            inbox_closed = true;
                        }
                    }
                    _ = self.external_tasks.next() => {}
                }
            }
        }
    }
    pub(crate) fn spawn_reducer(mut self) -> RemoteHandle<Self> {
        self.spawner
            .clone()
            .spawn_with_handle(async move {
                self.run().await;
                self
            })
            .unwrap()
    }
}
