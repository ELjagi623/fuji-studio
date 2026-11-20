//! PTP-over-USB framing (ISO 15740 §D): command / data / response containers on a
//! pair of bulk endpoints. This is the transport used on macOS (and Linux), where
//! there is no WPD-style pass-through and we talk to the camera through libusb.
//!
//! The framing logic is separated from libusb behind the tiny [`Bulk`] trait so it
//! can be unit-tested with a fake camera on any OS (see the tests at the bottom).
//!
//! Unlike WPD, nobody else manages the session here: we send OpenSession ourselves
//! (`driver_manages_session() == false`) and keep our own transaction counter.

use crate::codes::{op, resp};
use crate::transport::{DataPhase, Progress, Response, Transport};
use anyhow::{bail, Context, Result};
use std::time::Duration;

const HEADER: usize = 12;
const T_COMMAND: u16 = 1;
const T_DATA: u16 = 2;
const T_RESPONSE: u16 = 3;
const T_EVENT: u16 = 4;

/// Largest single bulk transfer we issue (a multiple of every USB max packet size).
const CHUNK: usize = 1024 * 1024;

/// One bulk IN/OUT endpoint pair.
pub trait Bulk {
    fn max_packet_in(&self) -> usize;
    fn max_packet_out(&self) -> usize;
    /// Bulk OUT. An empty slice must send a zero-length packet.
    fn write(&mut self, buf: &[u8]) -> Result<usize>;
    /// Bulk IN. `buf.len()` is always a multiple of `max_packet_in()`.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    fn set_timeout(&mut self, t: Duration);
}

pub struct PtpUsb<B: Bulk> {
    io: B,
    label: String,
    session_open: bool,
    /// next transaction id (1.. while a session is open)
    tid: u32,
    /// default per-transfer timeout, restored after the short one used in Drop
    timeout: Duration,
}

struct Container {
    kind: u16,
    code: u16,
    payload: Vec<u8>,
}

fn round_up(n: usize, to: usize) -> usize {
    n.div_ceil(to) * to
}

fn header(len: usize, kind: u16, code: u16, tid: u32) -> [u8; HEADER] {
    let mut h = [0u8; HEADER];
    h[0..4].copy_from_slice(&(len as u32).to_le_bytes());
    h[4..6].copy_from_slice(&kind.to_le_bytes());
    h[6..8].copy_from_slice(&code.to_le_bytes());
    h[8..12].copy_from_slice(&tid.to_le_bytes());
    h
}

impl<B: Bulk> PtpUsb<B> {
    pub fn new(io: B, label: String, timeout: Duration) -> Self {
        let mut s = Self { io, label, session_open: false, tid: 1, timeout };
        s.io.set_timeout(timeout);
        s
    }

    fn next_tid(&mut self) -> u32 {
        if !self.session_open {
            return 0; // session-less operations (GetDeviceInfo, OpenSession) use 0
        }
        let t = self.tid;
        self.tid = self.tid.wrapping_add(1).max(1);
        t
    }

    /// Write `head` followed by `body` as one logical OUT stream, in big chunks.
    fn write_stream(&mut self, head: &[u8], body: &[u8], mut progress: Progress<'_>) -> Result<()> {
        let total = head.len() + body.len();
        let first = body.len().min(CHUNK.saturating_sub(head.len()));
        let mut buf = Vec::with_capacity(head.len() + first);
        buf.extend_from_slice(head);
        buf.extend_from_slice(&body[..first]);
        self.write_all(&buf)?;
        let mut sent = first;
        if let Some(cb) = progress.as_mut() {
            cb(sent as u64, body.len() as u64);
        }
        while sent < body.len() {
            let n = (body.len() - sent).min(CHUNK);
            self.write_all(&body[sent..sent + n])?;
            sent += n;
            if let Some(cb) = progress.as_mut() {
                cb(sent as u64, body.len() as u64);
            }
        }
        // A transfer that ends exactly on a packet boundary needs a zero-length packet
        // so the camera sees the end of the container.
        if total % self.io.max_packet_out() == 0 {
            self.io.write(&[]).context("USB write (end of data)")?;
        }
        Ok(())
    }

    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            let n = self.io.write(buf).context("USB write")?;
            if n == 0 {
                bail!("USB write wrote 0 bytes");
            }
            buf = &buf[n..];
        }
        Ok(())
    }

    /// One bulk IN read that returns at least one byte (stray zero-length packets are skipped).
    fn read_some(&mut self, buf: &mut [u8]) -> Result<usize> {
        for _ in 0..16 {
            let n = self.io.read(buf).context("USB read")?;
            if n > 0 {
                return Ok(n);
            }
        }
        bail!("USB read kept returning empty packets")
    }

    /// Read one container. For DATA containers `progress` reports payload bytes so far.
    fn read_container(&mut self, mut progress: Progress<'_>) -> Result<Container> {
        let mps = self.io.max_packet_in();
        // First read is exactly one packet: it can never swallow the container that follows.
        let mut first = vec![0u8; mps.max(HEADER)];
        let first_len = round_up(first.len(), mps);
        first.resize(first_len, 0);
        let n = self.read_some(&mut first)?;
        if n < HEADER {
            bail!("short PTP container ({n} bytes)");
        }
        let len = u32::from_le_bytes(first[0..4].try_into().unwrap());
        let kind = u16::from_le_bytes(first[4..6].try_into().unwrap());
        let code = u16::from_le_bytes(first[6..8].try_into().unwrap());
        let mut payload: Vec<u8> = Vec::new();

        if len == u32::MAX {
            // length unknown (>4 GiB or streaming): read until a short packet
            payload.extend_from_slice(&first[HEADER..n]);
            let mut last = n;
            let mut buf = vec![0u8; CHUNK];
            while last % mps == 0 {
                last = self.read_some(&mut buf)?;
                payload.extend_from_slice(&buf[..last]);
                if let Some(cb) = progress.as_mut() {
                    cb(payload.len() as u64, 0);
                }
            }
            return Ok(Container { kind, code, payload });
        }

        let len = len as usize;
        if len < HEADER {
            bail!("bad PTP container length {len}");
        }
        let expected = len - HEADER;
        payload.reserve(expected);
        payload.extend_from_slice(&first[HEADER..n.min(len)]);
        if let Some(cb) = progress.as_mut() {
            cb(payload.len() as u64, expected as u64);
        }
        let mut buf = vec![0u8; CHUNK.min(round_up(expected.max(mps), mps))];
        while payload.len() < expected {
            let remaining = expected - payload.len();
            let want = round_up(remaining, mps).min(CHUNK);
            let n = self.read_some(&mut buf[..want])?;
            payload.extend_from_slice(&buf[..n.min(remaining)]);
            if let Some(cb) = progress.as_mut() {
                cb(payload.len() as u64, expected as u64);
            }
            if n < want && payload.len() < expected {
                bail!("PTP data truncated: got {} of {expected} bytes", payload.len());
            }
        }
        Ok(Container { kind, code, payload })
    }

    fn read_response(&mut self) -> Result<Response> {
        loop {
            let c = self.read_container(None)?;
            match c.kind {
                T_RESPONSE => {
                    let params = c.payload.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
                    return Ok(Response { code: c.code, params, data: Vec::new() });
                }
                T_EVENT => continue,
                other => bail!("expected PTP response container, got type {other} (code 0x{:04X})", c.code),
            }
        }
    }
}

impl<B: Bulk> Transport for PtpUsb<B> {
    fn describe(&self) -> String {
        self.label.clone()
    }

    fn driver_manages_session(&self) -> bool {
        false
    }

    fn execute(&mut self, opc: u16, params: &[u32], data: DataPhase<'_>, progress: Progress<'_>) -> Result<Response> {
        let tid = self.next_tid();

        let mut cmd = Vec::with_capacity(HEADER + params.len() * 4);
        cmd.extend_from_slice(&header(HEADER + params.len() * 4, T_COMMAND, opc, tid));
        for p in params {
            cmd.extend_from_slice(&p.to_le_bytes());
        }
        self.write_all(&cmd)?;

        let mut resp = match data {
            DataPhase::None => self.read_response()?,
            DataPhase::Out(body) => {
                if body.len() + HEADER > u32::MAX as usize {
                    bail!("data phase too large ({} bytes)", body.len());
                }
                self.write_stream(&header(HEADER + body.len(), T_DATA, opc, tid), body, progress)?;
                self.read_response()?
            }
            DataPhase::In => {
                let mut c = self.read_container(progress)?;
                while c.kind == T_EVENT {
                    c = self.read_container(None)?;
                }
                match c.kind {
                    T_DATA => {
                        let mut r = self.read_response()?;
                        r.data = c.payload;
                        r
                    }
                    // device refused before the data phase: the response comes first
                    T_RESPONSE => Response {
                        code: c.code,
                        params: c.payload.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect(),
                        data: Vec::new(),
                    },
                    other => bail!("unexpected PTP container type {other} (code 0x{:04X})", c.code),
                }
            }
        };

        match opc {
            op::OPEN_SESSION if resp.code == crate::codes::resp::OK || resp.code == resp::SESSION_ALREADY_OPEN => {
                self.session_open = true;
                self.tid = self.tid.max(1);
                if resp.code == resp::OK {
                    self.tid = 1;
                }
            }
            op::CLOSE_SESSION => {
                self.session_open = false;
                self.tid = 1;
            }
            _ => {}
        }
        resp.params.shrink_to_fit();
        Ok(resp)
    }
}

impl<B: Bulk> Drop for PtpUsb<B> {
    fn drop(&mut self) {
        // Leave the camera without a dangling session; never hang long doing it.
        if self.session_open {
            self.io.set_timeout(Duration::from_secs(2));
            let _ = self.execute(op::CLOSE_SESSION, &[], DataPhase::None, None);
            self.io.set_timeout(self.timeout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Fake camera: records what the host writes, replays canned IN packets
    /// (split exactly like a real device would at `mps`).
    struct Fake {
        mps: usize,
        written: Vec<Vec<u8>>,
        incoming: VecDeque<Vec<u8>>,
    }

    impl Fake {
        fn new(mps: usize) -> Self {
            Self { mps, written: vec![], incoming: VecDeque::new() }
        }
        /// Queue one container as the device sends it: bytes split into packets,
        /// each `read` returning up to the requested size.
        fn push(&mut self, kind: u16, code: u16, tid: u32, payload: &[u8], zlp: bool) {
            let mut c = header(HEADER + payload.len(), kind, code, tid).to_vec();
            c.extend_from_slice(payload);
            let exact = c.len() % self.mps == 0;
            self.incoming.push_back(c);
            if exact && zlp {
                self.incoming.push_back(vec![]);
            }
        }
    }

    impl Bulk for Fake {
        fn max_packet_in(&self) -> usize {
            self.mps
        }
        fn max_packet_out(&self) -> usize {
            self.mps
        }
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            self.written.push(buf.to_vec());
            Ok(buf.len())
        }
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            // hand out data from the front queue entry, at most buf.len() bytes
            let Some(front) = self.incoming.front_mut() else { bail!("fake: nothing to read") };
            if front.is_empty() {
                self.incoming.pop_front();
                return Ok(0);
            }
            let n = front.len().min(buf.len());
            buf[..n].copy_from_slice(&front[..n]);
            front.drain(..n);
            if front.is_empty() {
                self.incoming.pop_front();
            }
            Ok(n)
        }
        fn set_timeout(&mut self, _: Duration) {}
    }

    fn cam(fake: Fake) -> PtpUsb<Fake> {
        PtpUsb::new(fake, "fake".into(), Duration::from_secs(1))
    }

    #[test]
    fn open_session_uses_tid_zero_then_counts() {
        let mut f = Fake::new(512);
        f.push(T_RESPONSE, resp::OK, 0, &[], true);
        f.push(T_RESPONSE, resp::OK, 1, &[], true);
        f.push(T_RESPONSE, resp::OK, 2, &[], true);
        let mut c = cam(f);
        assert!(c.execute(op::OPEN_SESSION, &[1], DataPhase::None, None).unwrap().is_ok());
        c.execute(op::GET_STORAGE_IDS, &[], DataPhase::None, None).unwrap();
        c.execute(op::GET_STORAGE_IDS, &[], DataPhase::None, None).unwrap();
        let tids: Vec<u32> = c.io.written.iter().map(|w| u32::from_le_bytes(w[8..12].try_into().unwrap())).collect();
        assert_eq!(tids, [0, 1, 2]);
        // OpenSession carries session id 1 as its parameter
        assert_eq!(&c.io.written[0][12..16], &1u32.to_le_bytes());
        // forget the session so Drop doesn't try to talk to the (exhausted) fake
        c.session_open = false;
    }

    #[test]
    fn in_phase_big_payload_and_zlp_alignment() {
        // payload sized so the whole data container is an exact multiple of 512: device sends a ZLP
        for payload_len in [0usize, 1, 500, 512 - HEADER, 512, 100_000, 1024 * 1024 + 7, 3 * 1024 * 1024 - HEADER] {
            let payload: Vec<u8> = (0..payload_len).map(|i| (i % 251) as u8).collect();
            let mut f = Fake::new(512);
            f.push(T_DATA, op::GET_OBJECT, 1, &payload, true);
            f.push(T_RESPONSE, resp::OK, 1, &[], true);
            let mut c = cam(f);
            c.session_open = true;
            let mut last = (0, 0);
            let mut cb = |d, t| last = (d, t);
            let r = c.execute(op::GET_OBJECT, &[1], DataPhase::In, Some(&mut cb)).unwrap();
            assert!(r.is_ok(), "len {payload_len}");
            assert_eq!(r.data, payload, "len {payload_len}");
            assert_eq!(last, (payload_len as u64, payload_len as u64));
            c.session_open = false;
        }
    }

    #[test]
    fn in_phase_without_zlp_does_not_swallow_response() {
        // exact multiple of the packet size and the device does NOT send a ZLP
        let payload = vec![7u8; 512 * 3 - HEADER];
        let mut f = Fake::new(512);
        f.push(T_DATA, op::GET_OBJECT, 1, &payload, false);
        f.push(T_RESPONSE, resp::OK, 1, &[], false);
        let mut c = cam(f);
        c.session_open = true;
        let r = c.execute(op::GET_OBJECT, &[1], DataPhase::In, None).unwrap();
        assert_eq!(r.data.len(), payload.len());
        assert!(r.is_ok());
        c.session_open = false;
    }

    #[test]
    fn in_phase_error_response_without_data() {
        let mut f = Fake::new(512);
        f.push(T_RESPONSE, resp::DEVICE_BUSY, 1, &[], true);
        let mut c = cam(f);
        c.session_open = true;
        let r = c.execute(op::GET_DEVICE_PROP_VALUE, &[0xD185], DataPhase::In, None).unwrap();
        assert_eq!(r.code, resp::DEVICE_BUSY);
        assert!(r.data.is_empty());
        c.session_open = false;
    }

    #[test]
    fn response_params_and_events_are_parsed() {
        let mut f = Fake::new(64);
        f.push(T_EVENT, 0x4002, 0, &[1, 0, 0, 0], true);
        let mut p = vec![];
        p.extend_from_slice(&0xAABBCCDDu32.to_le_bytes());
        p.extend_from_slice(&5u32.to_le_bytes());
        f.push(T_RESPONSE, resp::OK, 1, &p, true);
        let mut c = cam(f);
        c.session_open = true;
        let r = c.execute(op::SET_DEVICE_PROP_VALUE, &[1], DataPhase::None, None).unwrap();
        assert_eq!(r.params, [0xAABBCCDD, 5]);
        c.session_open = false;
    }

    #[test]
    fn out_phase_frames_container_and_reports_progress() {
        for body_len in [2usize, 500, 512 - HEADER, 100_000, 2 * 1024 * 1024 + 5] {
            let body: Vec<u8> = (0..body_len).map(|i| (i % 13) as u8).collect();
            let mut f = Fake::new(512);
            f.push(T_RESPONSE, resp::OK, 1, &[], true);
            let mut c = cam(f);
            c.session_open = true;
            let mut last = (0, 0);
            let mut cb = |d, t| last = (d, t);
            let r = c.execute(op::SET_DEVICE_PROP_VALUE, &[0xD185], DataPhase::Out(&body), Some(&mut cb)).unwrap();
            assert!(r.is_ok());
            assert_eq!(last, (body_len as u64, body_len as u64));

            let w = &c.io.written;
            // first write is the command container
            assert_eq!(w[0].len(), 16);
            assert_eq!(u16::from_le_bytes(w[0][4..6].try_into().unwrap()), T_COMMAND);
            // everything after it is the data container: header + body (+ optional ZLP)
            let stream: Vec<u8> = w[1..].concat();
            assert_eq!(stream.len(), HEADER + body_len);
            assert_eq!(u32::from_le_bytes(stream[0..4].try_into().unwrap()) as usize, HEADER + body_len);
            assert_eq!(u16::from_le_bytes(stream[4..6].try_into().unwrap()), T_DATA);
            assert_eq!(u16::from_le_bytes(stream[6..8].try_into().unwrap()), op::SET_DEVICE_PROP_VALUE);
            assert_eq!(&stream[HEADER..], &body[..]);
            // same transaction id on command and data
            assert_eq!(w[0][8..12], stream[8..12]);
            // ZLP exactly when the container ends on a packet boundary
            let zlp = w.last().unwrap().is_empty();
            assert_eq!(zlp, (HEADER + body_len) % 512 == 0, "len {body_len}");
            c.session_open = false;
        }
    }

    #[test]
    fn already_open_session_is_treated_as_open() {
        let mut f = Fake::new(512);
        f.push(T_RESPONSE, resp::SESSION_ALREADY_OPEN, 0, &[], true);
        f.push(T_RESPONSE, resp::OK, 1, &[], true); // CloseSession with tid 1
        f.push(T_RESPONSE, resp::OK, 0, &[], true); // OpenSession again with tid 0
        let mut c = cam(f);
        let r = c.execute(op::OPEN_SESSION, &[1], DataPhase::None, None).unwrap();
        assert_eq!(r.code, resp::SESSION_ALREADY_OPEN);
        c.execute(op::CLOSE_SESSION, &[], DataPhase::None, None).unwrap();
        c.execute(op::OPEN_SESSION, &[1], DataPhase::None, None).unwrap();
        let tids: Vec<u32> = c.io.written.iter().map(|w| u32::from_le_bytes(w[8..12].try_into().unwrap())).collect();
        assert_eq!(tids, [0, 1, 0]);
        c.session_open = false;
    }
}
