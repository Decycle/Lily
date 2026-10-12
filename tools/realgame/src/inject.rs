//! Playback of a tap timeline into the shared buffer (doc 07 §1.3, §6): plan the publishes, write them on time,
//! record each write and the DLL's acknowledgement, and always release every cell at the end.
//!
//! Every state change is one `edge` publish; a held state is re-published (`renew`) every [`RENEW_MS`] with a
//! [`LEASE_MS`] lease, so a crashed injector releases all cells within 0.1 s. Playback ends with a `clear`.

use crate::cells::{CELLS, to_cells};
use crate::shm::ShmView;
use crate::stats::{Stats, summarize};
use crate::timeline::Tap;
use crate::wcon::input_payload;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wacca_game::input::InputFrame;

pub const LEASE_MS: u64 = 100;
pub const RENEW_MS: f64 = 50.0;
/// How long after a write to keep polling (spinning) for the DLL's ack; it normally comes within ~1 ms.
pub const ACK_WAIT_MS: f64 = 20.0;
/// Treat the DLL as attached only if its heartbeat is younger than this (doc 07 §6 safety rules).
pub const HEARTBEAT_MAX_AGE_MS: u64 = 3000;
/// `source_id` in our input records ("WSLM").
const SOURCE_ID: u32 = 0x4D4C_5357;
/// Sleep until this close to a deadline, then spin.
const SPIN_MS: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Edge,
    Renew,
    Clear,
}

/// One planned write, at script time `t_ms`.
#[derive(Clone, Debug, PartialEq)]
pub struct Publish {
    pub t_ms: f64,
    pub kind: Kind,
    /// Operator buttons (`wcon::OPBTN_*`), used by test-menu navigation ([`crate::nav`]).
    pub opbtn: u8,
    pub cells: [u8; CELLS],
}

/// One performed write (one `inject.jsonl` line). Times are ms since playback start; `t_ack_ms` is when we saw
/// `io.input_sequence == seq`, `None` if the DLL never took this record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub kind: Kind,
    pub seq: u32,
    pub t_script_ms: f64,
    pub t_sched_ms: f64,
    pub t_write_ms: f64,
    pub t_ack_ms: Option<f64>,
    #[serde(default)]
    pub opbtn: u8,
    /// 240 cells as 60 hex digits; digit `j` holds cells `4j..4j+4`, cell `4j` in bit 0.
    pub cells: String,
}

impl Record {
    pub fn write_error_ms(&self) -> f64 {
        self.t_write_ms - self.t_sched_ms
    }

    pub fn ack_latency_ms(&self) -> Option<f64> {
        self.t_ack_ms.map(|a| a - self.t_write_ms)
    }
}

pub fn cells_hex(cells: &[u8; CELLS]) -> String {
    cells
        .chunks(4)
        .map(|c| {
            let nibble = c
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &v)| n | u32::from(v != 0) << i);
            char::from_digit(nibble, 16).unwrap_or('?')
        })
        .collect()
}

/// Inverse of [`cells_hex`]; `None` unless exactly 60 hex digits.
pub fn cells_from_hex(hex: &str) -> Option<[u8; CELLS]> {
    if hex.len() != CELLS / 4 {
        return None;
    }
    let mut cells = [0; CELLS];
    for (j, c) in hex.chars().enumerate() {
        let nibble = c.to_digit(16)?;
        for i in 0..4 {
            cells[4 * j + i] = u8::from(nibble >> i & 1 == 1);
        }
    }
    Some(cells)
}

/// Edge list -> publishes: one edge per frame plus lease renewals while any cell is held.
pub fn schedule(frames: &[InputFrame]) -> Vec<Publish> {
    let mut out = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        // Whole microseconds, so seconds -> ms does not leave float noise in the log.
        let t = (f.time * 1e6).round() / 1e3;
        let cells = to_cells(&f.sensors);
        out.push(Publish {
            t_ms: t,
            kind: Kind::Edge,
            opbtn: 0,
            cells,
        });
        if f.sensors.is_empty() {
            continue;
        }
        let Some(next) = frames.get(i + 1).map(|n| n.time * 1000.0) else {
            continue;
        };
        let mut r = t + RENEW_MS;
        while r < next {
            out.push(Publish {
                t_ms: r,
                kind: Kind::Renew,
                opbtn: 0,
                cells,
            });
            r += RENEW_MS;
        }
    }
    out
}

/// Time source and waiting strategy, so playback can be tested without real time.
pub trait Clock {
    /// Milliseconds since playback start.
    fn now_ms(&self) -> f64;
    /// Waits for up to `remaining_ms` (may return early; called in a polling loop).
    fn pause(&self, remaining_ms: f64);
}

/// Spin-sleep hybrid on `Instant` (QPC on Windows): sleep in <= 1 ms steps until [`SPIN_MS`] before the
/// deadline, then spin. std's `sleep` uses a high-resolution waitable timer on Windows 10 1803+, so no
/// `timeBeginPeriod` is needed.
#[derive(Debug)]
pub struct RealClock {
    start: Instant,
}

impl RealClock {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Clock for RealClock {
    fn now_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    fn pause(&self, remaining_ms: f64) {
        if remaining_ms > SPIN_MS {
            std::thread::sleep(Duration::from_secs_f64(
                (remaining_ms - SPIN_MS).min(1.0) / 1000.0,
            ));
        } else {
            std::hint::spin_loop();
        }
    }
}

pub fn heartbeat_fresh(now_tick_ms: u64, heartbeat_ms: u64) -> bool {
    heartbeat_ms != 0 && now_tick_ms.saturating_sub(heartbeat_ms) < HEARTBEAT_MAX_AGE_MS
}

const CLEAR: [u8; CELLS] = [0; CELLS];

/// Releases all cells if playback unwinds before its explicit clear.
struct ClearOnDrop<'a> {
    view: &'a ShmView,
    armed: bool,
}

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.view
                .publish_input(&input_payload(0, 0, &CLEAR, SOURCE_ID, 0, LEASE_MS));
        }
    }
}

fn write(
    view: &ShmView,
    clock: &impl Clock,
    p: &Publish,
    t_script_ms: f64,
    t_sched_ms: f64,
) -> Record {
    let t_write_ms = clock.now_ms();
    let (kind, cells) = (p.kind, &p.cells);
    let rec = input_payload(
        p.opbtn,
        0,
        cells,
        SOURCE_ID,
        (t_write_ms * 1000.0) as u64,
        LEASE_MS,
    );
    let seq = view.publish_input(&rec);
    Record {
        kind,
        seq,
        t_script_ms,
        t_sched_ms,
        t_write_ms,
        t_ack_ms: None,
        opbtn: p.opbtn,
        cells: cells_hex(cells),
    }
}

/// Marks `rec` acknowledged if the DLL has taken it; returns whether it is still worth waiting (unacked and
/// written less than [`ACK_WAIT_MS`] ago).
fn poll_ack(view: &ShmView, clock: &impl Clock, rec: Option<&mut Record>) -> bool {
    let Some(r) = rec.filter(|r| r.t_ack_ms.is_none()) else {
        return false;
    };
    let now = clock.now_ms();
    if view.io_consumed_sequence() == u64::from(r.seq) {
        r.t_ack_ms = Some(now);
        return false;
    }
    now - r.t_write_ms < ACK_WAIT_MS
}

fn await_ack(view: &ShmView, clock: &impl Clock, rec: &mut Record) {
    while poll_ack(view, clock, Some(rec)) {
        clock.pause(0.0);
    }
}

fn flush(
    done: &mut Vec<Record>,
    sink: &mut dyn FnMut(&Record) -> std::io::Result<()>,
) -> std::io::Result<()> {
    done.drain(..).try_for_each(|r| sink(&r))
}

/// Plays `plan` (script times shifted by `delay_ms`) into `view`, handing each finished record to `sink`.
/// Stops early when `stop` is set or `sink` fails. Whatever happens, finishes with a `clear` publish of all
/// cells (also on unwind) and returns the first sink error.
///
/// Timing: the sink only runs while idle (latest record acked or given up on), never between a deadline and
/// its write; while an ack is outstanding the loop spins so the ack time is precise.
pub fn play(
    view: &ShmView,
    plan: &[Publish],
    delay_ms: f64,
    clock: &impl Clock,
    stop: &AtomicBool,
    sink: &mut dyn FnMut(&Record) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut guard = ClearOnDrop { view, armed: true };
    let mut pending: Option<Record> = None;
    let mut done = Vec::new();
    let mut result = Ok(());
    'plan: for p in plan {
        let t_sched = delay_ms + p.t_ms;
        loop {
            let waiting_ack = poll_ack(view, clock, pending.as_mut());
            if !waiting_ack {
                result = flush(&mut done, sink);
            }
            if result.is_err() || stop.load(Ordering::Relaxed) {
                break 'plan;
            }
            let remaining = t_sched - clock.now_ms();
            if remaining <= 0.0 {
                break;
            }
            clock.pause(if waiting_ack { 0.0 } else { remaining });
        }
        let rec = write(view, clock, p, p.t_ms, t_sched);
        done.extend(pending.replace(rec));
    }

    if let Some(mut r) = pending.take() {
        if !stop.load(Ordering::Relaxed) {
            await_ack(view, clock, &mut r);
        }
        done.push(r);
    }
    result = result.and(flush(&mut done, sink));
    let now = clock.now_ms();
    let release = Publish {
        t_ms: now - delay_ms,
        kind: Kind::Clear,
        opbtn: 0,
        cells: CLEAR,
    };
    let mut clear = write(view, clock, &release, release.t_ms, now);
    guard.armed = false;
    await_ack(view, clock, &mut clear);
    result.and(sink(&clear))
}

/// End-of-run numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub taps_total: usize,
    /// Taps whose press edge was written.
    pub taps_sent: usize,
    pub publishes: usize,
    pub acked: usize,
    /// `t_write - t_sched` over edges and renewals.
    pub timing: Option<Stats>,
    /// `t_ack - t_write` over acknowledged publishes.
    pub ack: Option<Stats>,
}

pub fn summary(taps: &[Tap], records: &[Record]) -> Summary {
    let last_edge = records
        .iter()
        .filter(|r| r.kind == Kind::Edge)
        .map(|r| r.t_script_ms)
        .fold(f64::NEG_INFINITY, f64::max);
    let timing: Vec<f64> = records
        .iter()
        .filter(|r| r.kind != Kind::Clear)
        .map(Record::write_error_ms)
        .collect();
    let ack: Vec<f64> = records.iter().filter_map(Record::ack_latency_ms).collect();
    Summary {
        taps_total: taps.len(),
        taps_sent: taps
            .iter()
            .filter(|t| t.dur_ms > 0.0 && t.at_ms <= last_edge)
            .count(),
        publishes: records.len(),
        acked: ack.len(),
        timing: summarize(&timing),
        ack: summarize(&ack),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shm::MemBuffer;
    use crate::timeline::{expand, parse};
    use std::cell::Cell;

    /// Jumps straight to each deadline (at least 0.25 ms per pause); optionally plays the DLL acking every
    /// record on the first pause after it was written.
    struct FakeClock<'a> {
        now: Cell<f64>,
        dll: Option<&'a ShmView>,
    }

    impl Clock for FakeClock<'_> {
        fn now_ms(&self) -> f64 {
            self.now.get()
        }
        fn pause(&self, remaining_ms: f64) {
            self.now.set(self.now.get() + remaining_ms.max(0.25));
            if let Some(v) = self.dll {
                v.simulate_dll_ack();
            }
        }
    }

    fn fake_clock(dll: Option<&ShmView>) -> FakeClock<'_> {
        FakeClock {
            now: Cell::new(0.0),
            dll,
        }
    }

    fn tap(at_ms: f64, dur_ms: f64, lane: u8) -> Tap {
        Tap {
            at_ms,
            dur_ms,
            lane,
            width: 1,
            depths: 1,
        }
    }

    fn run(
        view: &ShmView,
        plan: &[Publish],
        clock: &impl Clock,
        stop: &AtomicBool,
    ) -> (Vec<Record>, std::io::Result<()>) {
        let mut out = Vec::new();
        let res = play(view, plan, 10.0, clock, stop, &mut |r| {
            out.push(r.clone());
            Ok(())
        });
        (out, res)
    }

    #[test]
    fn hex_layout() {
        let mut cells = [0; CELLS];
        cells[0] = 1;
        cells[5] = 1;
        cells[239] = 1;
        let h = cells_hex(&cells);
        assert_eq!(h.len(), 60);
        assert_eq!(&h[..2], "12");
        assert_eq!(&h[59..], "8");
        assert_eq!(cells_from_hex(&h), Some(cells));
        assert_eq!(cells_from_hex("12"), None);
        assert_eq!(cells_from_hex(&"g".repeat(60)), None);
    }

    #[test]
    fn schedule_edges_and_renewals() {
        let plan = schedule(&expand(&[tap(1000.0, 120.0, 0), tap(2000.0, 30.0, 1)]));
        let got: Vec<(f64, Kind)> = plan.iter().map(|p| (p.t_ms, p.kind)).collect();
        assert_eq!(
            got,
            [
                (1000.0, Kind::Edge),
                (1050.0, Kind::Renew),
                (1100.0, Kind::Renew),
                (1120.0, Kind::Edge),
                (2000.0, Kind::Edge),
                (2030.0, Kind::Edge),
            ]
        );
        assert_eq!(plan[0].cells, plan[1].cells);
        assert_eq!(plan[0].cells[crate::cells::cell_index(0, 0)], 1);
        assert_eq!(plan[0].cells.iter().filter(|&&c| c == 1).count(), 1);
        assert!(plan[3].cells.iter().all(|&c| c == 0));
    }

    #[test]
    fn e1_schedule() {
        let s = parse(include_str!("../scripts/e1-lanes.taps.json")).unwrap();
        let plan = schedule(&expand(&s.taps));
        let edges = plan.iter().filter(|p| p.kind == Kind::Edge).count();
        assert_eq!(edges, 42);
        // 10 x 200 ms (3 renewals) + 7 x 300 ms (5) + 34/50 ms taps (0; a renewal lands only strictly inside).
        assert_eq!(plan.len() - edges, 10 * 3 + 7 * 5);
        assert!(plan.windows(2).all(|w| w[0].t_ms <= w[1].t_ms));
    }

    #[test]
    fn plays_on_schedule_and_measures_acks() {
        let buf = MemBuffer::default();
        let view = buf.view();
        let plan = schedule(&expand(&[tap(100.0, 60.0, 3)]));
        let clock = fake_clock(Some(&view));
        let (recs, res) = run(&view, &plan, &clock, &AtomicBool::new(false));
        assert!(res.is_ok());
        let kinds: Vec<Kind> = recs.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [Kind::Edge, Kind::Renew, Kind::Edge, Kind::Clear]);
        assert_eq!(recs[0].t_sched_ms, 110.0);
        // The fake clock lands exactly on deadlines and the sink never runs between deadline and write.
        assert!(recs[..3].iter().all(|r| r.write_error_ms() == 0.0));
        assert!(recs.iter().all(|r| r.ack_latency_ms() == Some(0.25)));
        let seqs: Vec<u32> = recs.iter().map(|r| r.seq).collect();
        assert!(seqs.windows(2).all(|w| w[1] == w[0] + 2));
        let s = summary(&[tap(100.0, 60.0, 3)], &recs);
        assert_eq!(
            (s.taps_total, s.taps_sent, s.publishes, s.acked),
            (1, 1, 4, 4)
        );
        assert_eq!(s.timing.map(|t| t.n), Some(3));
    }

    #[test]
    fn without_dll_acks_are_none_and_clear_still_written() {
        let buf = MemBuffer::default();
        let view = buf.view();
        let plan = schedule(&expand(&[tap(0.0, 10.0, 3)]));
        let (recs, _) = run(&view, &plan, &fake_clock(None), &AtomicBool::new(false));
        assert!(recs.iter().all(|r| r.t_ack_ms.is_none()));
        assert_eq!(recs.last().map(|r| r.kind), Some(Kind::Clear));
        assert_eq!(summary(&[], &recs).ack, None);
    }

    fn all_released(view: &ShmView) -> bool {
        view.input_record()[2..2 + CELLS].iter().all(|&c| c == 0)
    }

    #[test]
    fn stop_releases_everything() {
        let buf = MemBuffer::default();
        let view = buf.view();
        // A long hold; stop requested as soon as the press is written.
        let plan = schedule(&expand(&[tap(0.0, 10_000.0, 7)]));
        let stop = AtomicBool::new(false);
        let mut out = Vec::new();
        let res = play(&view, &plan, 0.0, &fake_clock(None), &stop, &mut |r| {
            out.push(r.kind);
            stop.store(true, Ordering::Relaxed);
            Ok(())
        });
        assert!(res.is_ok());
        assert_eq!(out, [Kind::Edge, Kind::Renew, Kind::Clear]);
        assert!(all_released(&view));
    }

    #[test]
    fn sink_error_still_releases_and_is_returned() {
        let buf = MemBuffer::default();
        let view = buf.view();
        let plan = schedule(&expand(&[tap(0.0, 500.0, 7), tap(600.0, 10.0, 8)]));
        let mut kinds = Vec::new();
        let res = play(
            &view,
            &plan,
            0.0,
            &fake_clock(None),
            &AtomicBool::new(false),
            &mut |r| {
                kinds.push(r.kind);
                Err(std::io::Error::other("disk full"))
            },
        );
        assert_eq!(res.map_err(|e| e.to_string()), Err("disk full".into()));
        assert_eq!(kinds.last(), Some(&Kind::Clear));
        assert!(kinds.len() < plan.len());
        assert!(all_released(&view));
    }

    #[test]
    fn unwind_releases_everything() {
        let buf = MemBuffer::default();
        let view = buf.view();
        let plan = schedule(&expand(&[tap(0.0, 500.0, 7)]));
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            play(
                &view,
                &plan,
                0.0,
                &fake_clock(None),
                &AtomicBool::new(false),
                &mut |_| panic!("sink panicked"),
            )
        }));
        assert!(res.is_err());
        assert!(all_released(&view));
    }

    #[test]
    fn heartbeat_freshness() {
        assert!(!heartbeat_fresh(10_000, 0));
        assert!(heartbeat_fresh(10_000, 9_000));
        assert!(!heartbeat_fresh(10_000, 7_000));
        assert!(heartbeat_fresh(10_000, 10_500)); // clock read before the DLL's last beat
    }

    #[test]
    fn summary_counts_only_sent_taps() {
        let taps = [tap(0.0, 10.0, 1), tap(100.0, 10.0, 2)];
        let rec = |kind, t: f64| Record {
            kind,
            seq: 2,
            t_script_ms: t,
            t_sched_ms: t,
            t_write_ms: t + 1.0,
            t_ack_ms: None,
            opbtn: 0,
            cells: String::new(),
        };
        let s = summary(&taps, &[rec(Kind::Edge, 0.0), rec(Kind::Clear, 5.0)]);
        assert_eq!((s.taps_total, s.taps_sent, s.publishes), (2, 1, 2));
        assert_eq!(s.timing.map(|t| (t.n, t.mean)), Some((1, 1.0)));
    }
}
