use super::readback::Handle;
use crate::flat::arena::Arena;
use crate::flat::runtime::{Node, Runtime, UserData};
use crate::linker::Linked;
use futures::future::RemoteHandle;
use futures::stream::{FuturesUnordered, StreamExt};
use futures::task::{FutureObj, Spawn, SpawnExt};
use std::future::poll_fn;
use std::sync::atomic::{Ordering::Acquire, fence};
use std::sync::{Arc, Weak};
use std::task::Poll;
use std::time::Instant;
use tokio::sync::mpsc;

pub enum ReducerMessage {
    Redex(Node<Linked>, Node<Linked>),
    Spawn(FutureObj<'static, ()>),
    /// All handles have been dropped.
    Released,
}

/// What handles need to reach their net: the reducer's inbox and the arena.
///
/// It's shared by all handles of a net, so that cloning a handle is cheap.
pub struct NetShared {
    sender: mpsc::UnboundedSender<ReducerMessage>,
    pub(crate) arena: Arc<Arena<Linked>>,
}

impl Drop for NetShared {
    fn drop(&mut self) {
        // Wake up the reducer, in case it's waiting, so it can tell it's done.
        let _ = self.sender.send(ReducerMessage::Released);
    }
}

#[derive(Clone)]
pub struct NetHandle(Arc<NetShared>);

impl NetHandle {
    pub(crate) fn send(&self, msg: ReducerMessage) {
        // If the reducer is gone (for example, its run was cancelled), there's nothing left
        // to deliver the message to.
        let _ = self.0.sender.send(msg);
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
    sender: mpsc::UnboundedSender<ReducerMessage>,
    // Once all handles are dropped, the inbox is drained, and no external
    // calls are pending, the net is done.
    net: Weak<NetShared>,
    // A handle kept alive while the reducer is busy, so that consecutive
    // external calls don't need to set up a new `NetShared`. It's dropped
    // before waiting, so it doesn't count as a live handle.
    active_net: Option<NetHandle>,
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
            sender: tx.clone(),
            arena: runtime.arena.clone(),
        });
        (
            Self {
                runtime,
                measure_net_duration,
                spawner,
                inbox: rx,
                sender: tx,
                net: Arc::downgrade(&net),
                active_net: None,
                external_tasks: FuturesUnordered::new(),
            },
            NetHandle(net),
        )
    }
    fn net_handle(&mut self) -> NetHandle {
        if let Some(net) = &self.active_net {
            return net.clone();
        }
        let net = match self.net.upgrade() {
            Some(net) => net,
            None => {
                let net = Arc::new(NetShared {
                    sender: self.sender.clone(),
                    arena: self.runtime.arena.clone(),
                });
                self.net = Arc::downgrade(&net);
                net
            }
        };
        self.active_net = Some(NetHandle(net.clone()));
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
            ReducerMessage::Released => {}
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
            // Nothing to do right now.
            self.active_net = None;
            if self.external_tasks.is_empty() {
                if self.net.strong_count() == 0 {
                    // No more messages can come. Make sure to see all the ones
                    // sent before the last handle was dropped.
                    fence(Acquire);
                    match self.inbox.try_recv() {
                        Ok(msg) => self.handle_message(msg),
                        Err(_) => break,
                    }
                } else {
                    let msg = self.inbox.recv().await;
                    self.handle_message(msg.expect("the reducer holds a sender"));
                }
            } else {
                tokio::select! {
                    msg = self.inbox.recv() => {
                        self.handle_message(msg.expect("the reducer holds a sender"));
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
