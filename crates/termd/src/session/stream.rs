//! The clients attached to a session: attaching with a snapshot or a replay from the ring,
//! fanning output out, catching a lagging client up, and the one size the terminal has.
//! `docs/terminals.md` 5.4.

use std::sync::Arc;

use bytes::Bytes;
use colosso_term_control::{
    ActorKind, AttachMode, ErrorCode, EventKind, Lagging, Message, Output, ProtocolError,
    ResponseBody, SNAPSHOT_FORMAT, Size, SnapshotChunk,
};

use super::cmd::{Attached, Reply, exited, not_attached};
use super::{Session, control, engine_size, pty};

/// Snapshots go out in pieces of this size.
const SNAPSHOT_CHUNK: usize = 4 << 20;

impl Session {
    pub(super) fn attach(
        &mut self,
        reply: &Reply,
        size: Option<Size>,
        mode: AttachMode,
        since_offset: Option<u64>,
    ) {
        let client = Arc::clone(&reply.client);
        let (index, lane) = client.attach(&self.record.handle);
        log::info!(
            "{}: client {} attached as {index}, {mode:?} at {size:?}",
            self.record.handle,
            client.id
        );
        // Whatever an earlier attach left queued is for a screen the client no longer shows.
        lane.bump();
        self.clients.insert(
            client.id,
            Attached {
                client: Arc::clone(&client),
                index,
                lane: Arc::clone(&lane),
                mode,
                size,
                lagging: false,
            },
        );
        self.refresh_observers();
        self.negotiate_size();
        self.publish(false);
        if let Some(since) = since_offset
            && let Some(bytes) = self.ring.read(since, usize::MAX)
        {
            reply.ok(ResponseBody::Attached {
                terminal: Box::new(self.record.clone()),
                index,
                offset: since,
            });
            if !bytes.is_empty() {
                self.send_output(client.id, since, Bytes::from(bytes));
            }
            return;
        }
        let offset = self.ring.end();
        reply.ok(ResponseBody::Attached {
            terminal: Box::new(self.record.clone()),
            index,
            offset,
        });
        self.send_snapshot(client.id, offset);
    }

    pub(super) fn detach(&mut self, client_id: u32) {
        if let Some(attached) = self.clients.remove(&client_id) {
            log::info!("{}: client {client_id} detached", self.record.handle);
            attached.client.detach(&self.record.handle);
            attached.lane.bump();
            self.refresh_observers();
            self.negotiate_size();
            self.publish(false);
        }
    }

    pub(super) fn input(&mut self, client_id: u32, bytes: &[u8]) {
        let Some(attached) = self.clients.get(&client_id) else {
            log::debug!(
                "{}: input from client {client_id}, which isn't attached",
                self.record.handle
            );
            return;
        };
        let refused = if attached.mode != AttachMode::Control {
            Some(ProtocolError::new(
                ErrorCode::ControlRequired,
                "observers don't type; attach in control mode",
            ))
        } else if self.pty.is_none() {
            Some(exited())
        } else {
            None
        };
        let client = Arc::clone(&attached.client);
        // A person's view reporting focus or a wheel turn writes without taking anything.
        let passive = client.actor.kind == ActorKind::Human && control::is_passive(bytes);
        let refused = refused.or_else(|| {
            if passive {
                None
            } else {
                self.may_act(&client.actor).err()
            }
        });
        if let Some(error) = refused {
            self.log_refused_input(&client.actor, bytes, &error);
            client.event(&self.record.handle, EventKind::Refused { error });
            return;
        }
        if !passive {
            self.status.input();
            self.commands.last_writer = Some(client.actor.clone());
            self.log_input(&client.actor, bytes);
        }
        self.write_pty(bytes);
    }

    pub(super) fn paste(&mut self, reply: &Reply, text: &str) {
        match self.clients.get(&reply.client.id) {
            None => return reply.err(not_attached()),
            Some(attached) if attached.mode != AttachMode::Control => {
                return reply.err(ProtocolError::new(
                    ErrorCode::ControlRequired,
                    "observers don't paste; attach in control mode",
                ));
            }
            Some(_) => {}
        }
        if self.pty.is_none() {
            return reply.err(exited());
        }
        let actor = reply.client.actor.clone();
        if let Err(error) = self.may_act(&actor) {
            self.log_paste(&actor, text, Err(&error));
            return reply.err(error);
        }
        self.commands.last_writer = Some(actor.clone());
        self.log_paste(&actor, text, Ok(()));
        let bytes = self.paste_bytes(text);
        self.status.input();
        self.write_pty(&bytes);
        reply.ok(ResponseBody::Done);
    }

    /// What pasting `text` writes: line ends as carriage returns, as a keyboard sends them,
    /// bracketed when the program asked; the end marker can't appear inside the brackets.
    pub(super) fn paste_bytes(&self, text: &str) -> Vec<u8> {
        let text = text
            .replace("\r\n", "\r")
            .replace('\n', "\r")
            .replace("\x1b[201~", "");
        if self.engine.is_bracketed_paste() {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.into_bytes()
        }
    }

    /// One chunk of output, starting at `offset`, to every attached client.
    pub(super) fn fan_out(&mut self, offset: u64, body: &Bytes) {
        let ids: Vec<u32> = self.clients.keys().copied().collect();
        for id in ids {
            self.send_output(id, offset, body.clone());
        }
    }

    /// Queues `body` as an `Output` frame for one client; a client whose queue is full is
    /// marked lagging and told so, and gets a snapshot once it drains.
    fn send_output(&mut self, client_id: u32, offset: u64, body: Bytes) {
        let Some(attached) = self.clients.get_mut(&client_id) else {
            return;
        };
        if attached.lagging {
            return;
        }
        let mut head = Vec::with_capacity(16);
        if let Err(error) = Output::encode_header(attached.index, offset, body.len(), &mut head) {
            log::error!("{}: {error}", self.record.handle);
            return;
        }
        if !attached
            .client
            .outbound
            .send_stream(Bytes::from(head), body, &attached.lane)
        {
            attached.lagging = true;
            attached.lane.bump();
            attached.client.send(&Message::Lagging(Lagging {
                index: attached.index,
            }));
            log::info!("{}: client {} is lagging", self.record.handle, client_id);
        }
    }

    /// The engine's state as of `offset`, in chunks; the ring replayed when the engine
    /// can't encode one.
    fn send_snapshot(&mut self, client_id: u32, offset: u64) {
        let snapshot = match self.engine.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                log::warn!(
                    "{}: no snapshot ({error}); replaying the ring instead",
                    self.record.handle
                );
                let start = self.ring.start();
                if let Some(bytes) = self.ring.read(start, usize::MAX) {
                    self.send_output(client_id, start, Bytes::from(bytes));
                }
                return;
            }
        };
        let Some(attached) = self.clients.get_mut(&client_id) else {
            return;
        };
        let pieces: Vec<&[u8]> = if snapshot.is_empty() {
            vec![&[][..]]
        } else {
            snapshot.chunks(SNAPSHOT_CHUNK).collect()
        };
        let of = pieces.len() as u16;
        for (i, piece) in pieces.into_iter().enumerate() {
            let chunk = SnapshotChunk {
                index: attached.index,
                offset,
                format: SNAPSHOT_FORMAT,
                chunk: i as u16,
                of,
                bytes: Bytes::copy_from_slice(piece),
            };
            let mut head = Vec::with_capacity(24);
            if let Err(error) = chunk.encode_header(&mut head) {
                log::error!("{}: {error}", self.record.handle);
                return;
            }
            if !attached
                .client
                .outbound
                .send_stream(Bytes::from(head), chunk.bytes, &attached.lane)
            {
                attached.lagging = true;
                attached.lane.bump();
                attached.client.send(&Message::Lagging(Lagging {
                    index: attached.index,
                }));
                return;
            }
        }
    }

    /// Lagging clients whose queues drained get a fresh snapshot; clients whose
    /// connections closed are let go.
    pub(super) fn resume_lagging(&mut self) {
        let gone: Vec<u32> = self
            .clients
            .values()
            .filter(|a| a.client.outbound.is_closed())
            .map(|a| a.client.id)
            .collect();
        for id in gone {
            self.detach(id);
        }
        let ready: Vec<u32> = self
            .clients
            .values()
            .filter(|a| a.lagging && a.client.outbound.drained())
            .map(|a| a.client.id)
            .collect();
        for id in ready {
            if let Some(attached) = self.clients.get_mut(&id) {
                attached.lagging = false;
            }
            let offset = self.ring.end();
            log::info!(
                "{}: client {id} caught up; sending a snapshot",
                self.record.handle
            );
            self.send_snapshot(id, offset);
        }
    }

    /// The terminal takes the smallest size among the clients attached in control mode;
    /// observers never size it.
    pub(super) fn negotiate_size(&mut self) {
        let mut best: Option<Size> = None;
        for attached in self.clients.values() {
            if attached.mode != AttachMode::Control {
                continue;
            }
            let Some(size) = attached.size else { continue };
            best = Some(match best {
                None => size,
                Some(other) => smaller(other, size),
            });
        }
        let Some(size) = best else { return };
        if size == self.record.size || size.cols == 0 || size.rows == 0 {
            return;
        }
        if let Some(pty) = &self.pty
            && let Err(error) = pty.master.resize(pty::pty_size(size))
        {
            log::warn!("{}: couldn't resize the pty: {error}", self.record.handle);
        }
        if let Err(error) = self.engine.resize(engine_size(size)) {
            log::warn!(
                "{}: couldn't resize the engine: {error}",
                self.record.handle
            );
        }
        self.record.size = size;
        self.publish(true);
        self.daemon
            .broadcast(&self.record.handle, &EventKind::Size { size });
    }
}

/// The smaller grid of two, with the cell size of the narrower one.
fn smaller(a: Size, b: Size) -> Size {
    let narrow = if b.cols < a.cols { b } else { a };
    Size {
        cols: a.cols.min(b.cols),
        rows: a.rows.min(b.rows),
        cell_width_px: narrow.cell_width_px,
        cell_height_px: narrow.cell_height_px,
    }
}
