//! Bounded-priority work queue (plan §9).
//!
//! P0 currently visible · P1 just outside the viewport · P2 current search results ·
//! P3 rest of the library (analysis) · P4 background thumbnail prefill.
//! Analysis of the whole library (P3) completes before speculative thumbnail rendering
//! (P4), so full-content search becomes complete as early as possible.
//! One entry per asset; pushing an existing asset merges its job flags and can only raise
//! its priority. Reprioritization is O(1) per asset using lazy deletion: lanes hold
//! `(asset, seq)` pairs and a popped pair is skipped if its seq is no longer current.

use parking_lot::{Condvar, Mutex};
use std::collections::{HashMap, HashSet, VecDeque};
use svg_core::model::AssetId;

pub const P0: u8 = 0;
pub const P1: u8 = 1;
pub const P2: u8 = 2;
pub const P3: u8 = 3;
pub const P4: u8 = 4;
const LANES: usize = 5;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobFlags {
    /// Extract metadata/text/hash (only meaningful while the asset is `Discovered`).
    pub analyze: bool,
    /// Ensure a cached thumbnail exists.
    pub thumb: bool,
}

impl JobFlags {
    pub const ANALYZE: Self = Self { analyze: true, thumb: false };
    pub const THUMB: Self = Self { analyze: false, thumb: true };
    pub const BOTH: Self = Self { analyze: true, thumb: true };
    fn merge(self, o: Self) -> Self {
        Self { analyze: self.analyze || o.analyze, thumb: self.thumb || o.thumb }
    }
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    prio: u8,
    flags: JobFlags,
    seq: u64,
}

#[derive(Default)]
struct State {
    entries: HashMap<AssetId, Entry>,
    lanes: [VecDeque<(AssetId, u64)>; LANES],
    seq: u64,
    shutdown: bool,
    /// Number of queued entries with the `analyze` flag (for progress reporting).
    analyze_count: usize,
    /// IDs the UI reported as visible/nearby in the last `set_viewport`.
    viewport: HashSet<AssetId>,
}

impl State {
    fn enqueue(&mut self, id: AssetId, prio: u8, flags: JobFlags) -> bool {
        let prio = prio.min(P4);
        self.seq += 1;
        let seq = self.seq;
        match self.entries.get_mut(&id) {
            Some(e) => {
                if flags.analyze && !e.flags.analyze {
                    self.analyze_count += 1;
                }
                e.flags = e.flags.merge(flags);
                if prio < e.prio {
                    e.prio = prio;
                    e.seq = seq;
                    self.lanes[prio as usize].push_back((id, seq));
                    true
                } else {
                    false
                }
            }
            None => {
                if flags.analyze {
                    self.analyze_count += 1;
                }
                self.entries.insert(id, Entry { prio, flags, seq });
                self.lanes[prio as usize].push_back((id, seq));
                true
            }
        }
    }

    fn set_prio(&mut self, id: AssetId, prio: u8) {
        self.seq += 1;
        let seq = self.seq;
        if let Some(e) = self.entries.get_mut(&id) {
            if e.prio != prio {
                e.prio = prio;
                e.seq = seq;
                self.lanes[prio as usize].push_back((id, seq));
            }
        }
    }

    fn pop(&mut self) -> Option<(AssetId, JobFlags, u8)> {
        for lane in 0..LANES {
            loop {
                // Visible work is LIFO: during fast scrolling the newest request is what's on screen.
                let item = if lane == 0 { self.lanes[lane].pop_back() } else { self.lanes[lane].pop_front() };
                let Some((id, seq)) = item else { break };
                if let Some(e) = self.entries.get(&id) {
                    if e.seq == seq {
                        let e = self.entries.remove(&id).unwrap();
                        if e.flags.analyze {
                            self.analyze_count -= 1;
                        }
                        return Some((id, e.flags, e.prio));
                    }
                }
            }
        }
        None
    }

    fn compact_if_needed(&mut self) {
        // Lazy deletion leaves stale pairs behind; rebuild when lanes are mostly garbage.
        let lane_total: usize = self.lanes.iter().map(|l| l.len()).sum();
        if lane_total > 4 * self.entries.len() + 1024 {
            let mut lanes: [VecDeque<(AssetId, u64)>; LANES] = Default::default();
            for lane in self.lanes.iter() {
                for &(id, seq) in lane {
                    if let Some(e) = self.entries.get(&id) {
                        if e.seq == seq {
                            lanes[e.prio as usize].push_back((id, seq));
                        }
                    }
                }
            }
            self.lanes = lanes;
        }
    }
}

#[derive(Default)]
pub struct WorkQueue {
    state: Mutex<State>,
    cv: Condvar,
}

impl WorkQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, id: AssetId, prio: u8, flags: JobFlags) {
        let mut st = self.state.lock();
        if st.enqueue(id, prio, flags) {
            drop(st);
            self.cv.notify_one();
        }
    }

    pub fn push_many(&self, ids: impl IntoIterator<Item = AssetId>, prio: u8, flags: JobFlags) {
        let mut st = self.state.lock();
        let mut any = false;
        for id in ids {
            any |= st.enqueue(id, prio, flags);
        }
        st.compact_if_needed();
        drop(st);
        if any {
            self.cv.notify_all();
        }
    }

    /// Viewport changed: `visible` → P0 (thumb), `nearby` → P1 (thumb). Work that was P0
    /// but is no longer in view is demoted: to P1 if a thumbnail request is still waiting
    /// on it (`waiting`), so held-open requests drain promptly instead of sitting behind
    /// the background backlog; otherwise to P3.
    pub fn set_viewport(&self, visible: &[AssetId], nearby: &[AssetId], waiting: &HashSet<AssetId>) {
        let mut st = self.state.lock();
        let keep: HashSet<AssetId> = visible.iter().chain(nearby.iter()).copied().collect();
        let demote: Vec<(AssetId, u8)> = st
            .entries
            .iter()
            .filter(|(id, e)| e.prio <= P1 && !keep.contains(id))
            .map(|(id, e)| (*id, e.prio))
            .collect();
        for (id, prio) in demote {
            let target = if waiting.contains(&id) { P1 } else { P3 };
            if target != prio {
                st.set_prio(id, target);
            }
        }
        // Only reprioritize work that is already queued (the thumbnail protocol enqueues
        // misses itself); this keeps cache hits from generating jobs.
        for &id in nearby {
            if st.entries.get(&id).is_some_and(|e| e.prio > P1) {
                st.set_prio(id, P1);
            }
        }
        for &id in visible {
            if st.entries.get(&id).is_some_and(|e| e.prio > P0) {
                st.set_prio(id, P0);
            }
        }
        st.viewport = keep;
        st.compact_if_needed();
        drop(st);
        self.cv.notify_all();
    }

    pub fn in_viewport(&self, id: AssetId) -> bool {
        self.state.lock().viewport.contains(&id)
    }

    /// Promote queued P3 work for assets in the current search results to P2.
    pub fn promote_results(&self, results: &[AssetId]) {
        let mut st = self.state.lock();
        if st.entries.is_empty() {
            return;
        }
        if results.len() > st.entries.len() {
            let set: HashSet<AssetId> = results.iter().copied().collect();
            let ids: Vec<AssetId> = st
                .entries
                .iter()
                .filter(|(id, e)| e.prio == P3 && set.contains(id))
                .map(|(id, _)| *id)
                .collect();
            for id in ids {
                st.set_prio(id, P2);
            }
        } else {
            for &id in results {
                if st.entries.get(&id).is_some_and(|e| e.prio == P3) {
                    st.set_prio(id, P2);
                }
            }
        }
        st.compact_if_needed();
    }

    /// Blocking pop of the highest-priority job. `None` after shutdown.
    pub fn pop(&self) -> Option<(AssetId, JobFlags, u8)> {
        let mut st = self.state.lock();
        loop {
            if st.shutdown {
                return None;
            }
            if let Some(job) = st.pop() {
                return Some(job);
            }
            self.cv.wait(&mut st);
        }
    }

    pub fn clear(&self) {
        let mut st = self.state.lock();
        st.entries.clear();
        for l in st.lanes.iter_mut() {
            l.clear();
        }
        st.viewport.clear();
        st.analyze_count = 0;
    }

    /// Queued entries that still need analysis.
    pub fn analyze_pending(&self) -> usize {
        self.state.lock().analyze_count
    }

    pub fn len(&self) -> usize {
        self.state.lock().entries.len()
    }

    pub fn shutdown(&self) {
        self.state.lock().shutdown = true;
        self.cv.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(q: &WorkQueue) -> Vec<(AssetId, u8)> {
        let mut out = vec![];
        loop {
            let mut st = q.state.lock();
            match st.pop() {
                Some((id, _, p)) => out.push((id, p)),
                None => break,
            }
        }
        out
    }

    #[test]
    fn priorities_and_merge() {
        let q = WorkQueue::new();
        q.push_many(1..=5, P3, JobFlags::ANALYZE);
        q.push(4, P0, JobFlags::THUMB);
        q.push(2, P2, JobFlags::THUMB);
        let mut st = q.state.lock();
        let first = st.pop().unwrap();
        assert_eq!((first.0, first.2), (4, P0));
        assert_eq!(first.1, JobFlags::BOTH);
        let second = st.pop().unwrap();
        assert_eq!((second.0, second.2), (2, P2));
        drop(st);
        assert_eq!(drain(&q).iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 3, 5]);
    }

    #[test]
    fn viewport_demotes_and_promotes() {
        let q = WorkQueue::new();
        q.push(10, P0, JobFlags::THUMB);
        q.push(11, P0, JobFlags::THUMB);
        q.push(12, P3, JobFlags::THUMB);
        let waiting: HashSet<AssetId> = [11].into_iter().collect();
        q.set_viewport(&[12], &[], &waiting);
        let order = drain(&q);
        assert_eq!(order[0], (12, P0));
        assert_eq!(order[1], (11, P1)); // still awaited by a request
        assert_eq!(order[2], (10, P3));
    }

    #[test]
    fn analyze_counter_and_p4() {
        let q = WorkQueue::new();
        q.push_many(1..=3, P3, JobFlags::ANALYZE);
        q.push(4, P4, JobFlags::THUMB);
        q.push(1, P0, JobFlags::THUMB); // merge keeps count
        assert_eq!(q.analyze_pending(), 3);
        let order = drain(&q);
        assert_eq!(order.last().unwrap(), &(4, P4));
        assert_eq!(q.analyze_pending(), 0);
    }

    #[test]
    fn p0_is_lifo() {
        let q = WorkQueue::new();
        q.push(1, P0, JobFlags::THUMB);
        q.push(2, P0, JobFlags::THUMB);
        assert_eq!(drain(&q)[0].0, 2);
    }

    #[test]
    fn results_promoted_to_p2() {
        let q = WorkQueue::new();
        q.push_many(1..=100, P3, JobFlags::ANALYZE);
        q.promote_results(&[50, 60]);
        let order = drain(&q);
        assert_eq!(&order[..2], &[(50, P2), (60, P2)]);
        assert_eq!(order.len(), 100);
    }
}
