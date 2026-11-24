//! Typed view of `/api/state`. Only the fields the UI reads are kept; serde skips the rest
//! without allocating, so a poll leaves almost nothing behind once the snapshot is swapped in.

use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Snapshot {
    pub ready: bool,
    pub now: f64,
    pub network: String,
    pub fork: Option<Fork>,
    pub services: Services,
    pub node: Option<Node>,
    pub farm: Option<Farm>,
    pub wallet: Option<Wallet>,
    pub peers: Vec<Peer>,
    pub chain: Chain,
    pub history: Vec<HistPt>,
    pub sps: Vec<Sp>,
    pub sp_totals: Totals,
    pub events: Vec<Event>,
    pub strength: Option<Strength>,
    pub plotting: Option<Plotting>,
}

#[derive(Deserialize, Clone, Copy)]
pub struct Fork {
    pub height: u32,
    pub v1_cutoff: u32,
    pub eta: f64,
    pub cutoff_eta: f64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Services {
    pub node: bool,
    pub farmer: bool,
    pub solver: bool,
    pub wallet: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Node {
    pub height: u32,
    pub synced: bool,
    pub sync_mode: bool,
    pub sync_tip: u32,
    pub space: Option<f64>,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(default)]
pub struct VerStat {
    pub plots: u32,
    pub size: f64,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(default)]
pub struct ByVersion {
    pub v1: VerStat,
    pub v2: VerStat,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Farm {
    pub plots: u32,
    pub failed: u32,
    pub no_key: u32,
    pub duplicates: u32,
    pub by_version: ByVersion,
    pub expected_pass: Option<f64>,
    pub share: Option<f64>,
    pub etw: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Wallet {
    pub confirmed: f64,
    pub unconfirmed: f64,
    pub synced: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Peer {
    pub host: String,
    pub port: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub height: Option<u32>,
    pub read: u64,
    pub written: u64,
    pub since: f64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Mix {
    pub v1: u32,
    pub v2: u32,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Chain {
    pub count: u32,
    pub mix: Mix,
    pub ours: Vec<u32>,
    pub ours_by_kind: Mix,
    /// One byte per block: b'1', b'2', or b'0' for a block we have not seen.
    pub seq: Box<str>,
    pub last_win_t: Option<f64>,
}

#[derive(Deserialize, Clone, Copy)]
pub struct HistPt {
    pub t: f64,
    pub space: f64,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(default)]
pub struct Sp {
    pub t: f64,
    pub eligible: u32,
    pub v1: Option<u32>,
    pub v2: u32,
    pub proofs: u32,
    pub s: f64,
    pub plots: u32,
}

impl Sp {
    pub fn v1_count(&self) -> u32 {
        self.v1.unwrap_or(self.proofs)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Totals {
    pub count: u32,
    pub passed: u32,
    pub proofs: u32,
    pub v1: Option<u32>,
    pub v2: Option<u32>,
    pub max_s: f64,
    pub avg_s: f64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Event {
    pub t: f64,
    pub kind: String,
    pub text: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct StrengthRow {
    pub cls: String,
    pub state: String,
    pub plots: u32,
    pub gb: f64,
    pub gb_sps: f64,
    pub until: u32,
    pub unlock: u32,
    pub bits: Option<u32>,
    pub checks: u64,
    pub locked: u64,
    pub passes: u64,
    pub expected: f64,
    pub proofs: u64,
    pub wins: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Mismatch {
    pub challenge: String,
    pub ours: i64,
    pub harvester: i64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Check {
    pub compared: u32,
    pub mismatches: Vec<Mismatch>,
    pub mismatch_count: u32,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Strength {
    pub rows: Vec<StrengthRow>,
    pub since: f64,
    pub sps_per_day: f64,
    pub fork: u32,
    pub max_strength: u32,
    pub check: Check,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Job {
    pub kind: String,
    pub k: Option<String>,
    pub total: u32,
    pub done: u32,
    pub started: f64,
    pub phase: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct OnDisk {
    pub v1: u32,
    pub v2: u32,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Plotting {
    pub jobs: Vec<Job>,
    pub on_disk: OnDisk,
    pub not_loaded: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A captured /api/state (peers and ids anonymised). Guards the field names the UI depends on.
    #[test]
    fn parses_server_state() {
        let s: Snapshot =
            serde_json::from_str(include_str!("../tests/fixtures/state.json")).unwrap();
        assert!(s.ready);
        assert_eq!(s.network, "test-example-chia");
        let fork = s.fork.unwrap();
        assert!(fork.v1_cutoff > fork.height);
        let farm = s.farm.unwrap();
        assert_eq!(
            farm.plots,
            farm.by_version.v1.plots + farm.by_version.v2.plots
        );
        assert!(s.peers.iter().any(|p| p.kind == "full node"));
        assert!(!s.chain.seq.is_empty());
        assert!(s.strength.unwrap().rows.iter().any(|r| r.cls == "v1"));
        assert!(!s.sps.is_empty());
    }

    #[test]
    fn tolerates_a_node_that_is_not_ready() {
        let s: Snapshot = serde_json::from_str(r#"{"ready": false}"#).unwrap();
        assert!(!s.ready && s.node.is_none() && s.peers.is_empty());
    }

    #[test]
    fn old_servers_without_v2_fields() {
        let sp: Sp = serde_json::from_str(
            r#"{"t": 1.0, "eligible": 2, "proofs": 1, "s": 0.01, "plots": 10}"#,
        )
        .unwrap();
        assert_eq!(sp.v1_count(), 1);
        assert_eq!(sp.v2, 0);
    }
}
