use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::Duration;
use sysinfo::{System, Networks, Disks};
use log::debug;

// ─── Ring buffer ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RingBuffer {
    data:   [f32; 60],
    head:   usize,
    filled: bool,
}

impl RingBuffer {
    pub fn new() -> Self {
        Self { data: [0.0; 60], head: 0, filled: false }
    }

    pub fn push(&mut self, v: f32) {
        self.data[self.head] = v;
        self.head = (self.head + 1) % 60;
        if self.head == 0 { self.filled = true; }
    }

    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        let len   = if self.filled { 60 } else { self.head };
        let start = if self.filled { self.head } else { 0 };
        (0..len).map(move |i| self.data[(start + i) % 60])
    }

    #[allow(dead_code)]
    pub fn latest(&self) -> f32 {
        if self.head == 0 && !self.filled { return 0.0; }
        let i = if self.head == 0 { 59 } else { self.head - 1 };
        self.data[i]
    }

    #[allow(dead_code)]
    pub fn average(&self) -> f32 {
        let vals: Vec<f32> = self.iter().collect();
        if vals.is_empty() { return 0.0; }
        vals.iter().sum::<f32>() / vals.len() as f32
    }
}

// ─── Snapshot ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StatsSnapshot {
    pub cpu_pct:       f32,
    pub ram_used_gb:   f32,
    pub ram_total_gb:  f32,
    pub net_rx_bps:    u64,
    pub net_tx_bps:    u64,
    pub disk_avail_gb:  f32,
    pub disk_total_gb:  f32,
    pub cpu_history:   RingBuffer,
    #[allow(dead_code)]
    pub ram_history:   RingBuffer,
}

// ─── Spawn polling thread ─────────────────────────────────────────────────────

pub fn spawn() -> Receiver<StatsSnapshot> {
    let (tx, rx): (SyncSender<StatsSnapshot>, Receiver<StatsSnapshot>) =
        mpsc::sync_channel(2);

    thread::Builder::new()
        .name("linuxpet-stats".into())
        .spawn(move || polling_loop(tx))
        .expect("Failed to spawn stats thread");

    rx
}

fn polling_loop(tx: SyncSender<StatsSnapshot>) {
    let mut sys      = System::new_all();
    let mut networks = Networks::new_with_refreshed_list();
    let mut disks    = Disks::new_with_refreshed_list();

    let mut cpu_history = RingBuffer::new();
    let mut ram_history = RingBuffer::new();

    // Warm-up: sysinfo needs two polls for accurate CPU %
    sys.refresh_cpu_usage();
    thread::sleep(Duration::from_millis(200));

    loop {
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        networks.refresh();   // sysinfo 0.30: no argument
        disks.refresh();      // sysinfo 0.30: no argument

        // CPU
        let cpu_pct: f32 = {
            let cpus = sys.cpus();
            if cpus.is_empty() { 0.0 }
            else { cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpus.len() as f32 }
        };

        // RAM
        let ram_used_gb  = sys.used_memory()  as f32 / 1_073_741_824.0;
        let ram_total_gb = sys.total_memory() as f32 / 1_073_741_824.0;
        let ram_pct      = if ram_total_gb > 0.0 { ram_used_gb / ram_total_gb * 100.0 } else { 0.0 };

        // Network
        let (net_rx, net_tx) = networks
            .iter()
            .fold((0u64, 0u64), |(rx, tx), (_, net)| {
                (rx + net.received(), tx + net.transmitted())
            });

        // Disk — sysinfo 0.30 Disk struct exposes space, not I/O rates
        // Sum available space across all mounted disks
        let (disk_avail, disk_total) = disks
            .iter()
            .fold((0u64, 0u64), |(avail, total), disk| {
                (avail + disk.available_space(), total + disk.total_space())
            });

        cpu_history.push(cpu_pct);
        ram_history.push(ram_pct);

        let snapshot = StatsSnapshot {
            cpu_pct,
            ram_used_gb,
            ram_total_gb,
            net_rx_bps:     net_rx,
            net_tx_bps:     net_tx,
            disk_avail_gb:  disk_avail as f32 / 1_073_741_824.0,
            disk_total_gb:  disk_total as f32 / 1_073_741_824.0,
            cpu_history:    cpu_history.clone(),
            ram_history:    ram_history.clone(),
        };

        debug!("CPU:{:.1}%  RAM:{:.1}/{:.1}GB  NET:↓{}↑{}",
            snapshot.cpu_pct, snapshot.ram_used_gb, snapshot.ram_total_gb,
            snapshot.net_rx_bps, snapshot.net_tx_bps,
        );

        let _ = tx.try_send(snapshot);
        thread::sleep(Duration::from_secs(1));
    }
}

// ─── Format helper ───────────────────────────────────────────────────────────

pub fn format_bytes(bps: u64) -> String {
    if      bps >= 1_073_741_824 { format!("{:.1} GB/s", bps as f64 / 1_073_741_824.0) }
    else if bps >= 1_048_576     { format!("{:.1} MB/s", bps as f64 / 1_048_576.0) }
    else if bps >= 1_024         { format!("{:.0} KB/s", bps as f64 / 1_024.0) }
    else                         { format!("{} B/s", bps) }
}
