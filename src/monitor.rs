//! Read-only Linux sampling over an existing SSH transport; optional process identities gate actions.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A fixed command, with no interpolated user values. Missing tools remain visible as errors.
pub const SAMPLE_COMMAND: &str = r#"export LC_ALL=C
printf '__MANTASH_OS__\n'; uname -s; uname -r; hostname; cat /proc/uptime 2>/dev/null
printf '__MANTASH_CPU__\n'; cat /proc/stat 2>/dev/null || printf 'MANTASH_ERROR: /proc/stat unavailable\n'
printf '__MANTASH_MEM__\n'; cat /proc/meminfo 2>/dev/null || printf 'MANTASH_ERROR: /proc/meminfo unavailable\n'
printf '__MANTASH_NET__\n'; cat /proc/net/dev 2>/dev/null || printf 'MANTASH_ERROR: /proc/net/dev unavailable\n'
printf '__MANTASH_DISK__\n'; df -Pk 2>&1
printf '__MANTASH_BOOT__\n'; cat /proc/sys/kernel/random/boot_id 2>/dev/null; getconf CLK_TCK 2>/dev/null; awk '$1 == "btime" {print $2}' /proc/stat 2>/dev/null
printf '__MANTASH_START_BEFORE__\n'; awk '{pid=$1; sub(/^.*\) /,""); if ($20 != "") print pid, $20}' /proc/[0-9]*/stat 2>/dev/null
printf '__MANTASH_PROCESS_V2__\n'; ps -eo pid=,ppid=,pcpu=,pmem=,rss=,stat=,user:64=,comm= --sort=-pcpu 2>&1
printf '__MANTASH_START_AFTER__\n'; awk '{pid=$1; sub(/^.*\) /,""); if ($20 != "") print pid, $20}' /proc/[0-9]*/stat 2>/dev/null
printf '__MANTASH_PORT__\n'
if command -v ss >/dev/null 2>&1; then
  ss -lntuap 2>&1 || {
    status=$?
    printf 'MANTASH_ERROR: ss exited with status %s\n' "$status"
  }
else
  printf 'MANTASH_ERROR: ss is not installed (install iproute2)\n'
fi
"#;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cpu {
    pub name: String,
    pub total: u64,
    pub idle: u64,
    pub percent: Option<f64>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Network {
    pub name: String,
    pub received: u64,
    pub sent: u64,
    pub receive_rate: Option<f64>,
    pub send_rate: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disk {
    pub filesystem: String,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub mount: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    pub user: String,
    pub started_at: Option<i64>,
    pub identity: Option<crate::processes::Identity>,
    pub pid: u32,
    pub parent: u32,
    pub cpu: f64,
    pub memory: f64,
    pub rss: u64,
    pub state: String,
    pub command: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub protocol: String,
    pub state: String,
    pub local: String,
    pub peer: String,
    pub process: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sample {
    pub boot_id: Option<String>,
    pub timestamp: i64,
    pub system: String,
    pub kernel: String,
    pub hostname: String,
    pub uptime: f64,
    pub cpu: Vec<Cpu>,
    pub memory: Option<Memory>,
    pub disks: Vec<Disk>,
    pub network: Vec<Network>,
    pub processes: Vec<Process>,
    pub ports: Vec<Port>,
    pub errors: BTreeMap<String, String>,
}

fn parse_ports(lines: &[&str]) -> Result<Vec<Port>, String> {
    let lines: Vec<_> = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    if lines.iter().any(|line| line.starts_with("MANTASH_ERROR:")) {
        return Err(lines.join("\n"));
    }
    let Some(header) = lines.first() else {
        return Ok(Vec::new());
    };
    let columns: Vec<_> = header.split_whitespace().collect();
    if columns.len() < 6
        || columns[0] != "Netid"
        || columns[1] != "State"
        || columns[2] != "Recv-Q"
        || columns[3] != "Send-Q"
        || columns[4] != "Local"
        || !columns.iter().skip(5).any(|column| *column == "Peer")
    {
        return Err(lines.join("\n"));
    }
    let mut ports = Vec::new();
    for line in lines.into_iter().skip(1) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 6
            || fields[2].parse::<u64>().is_err()
            || fields[3].parse::<u64>().is_err()
        {
            return Err(format!("Malformed ss row: {line}"));
        }
        ports.push(Port {
            protocol: fields[0].into(),
            state: fields[1].into(),
            local: fields[4].into(),
            peer: fields[5].into(),
            process: fields[6..].join(" "),
        });
    }
    Ok(ports)
}

/// Parse one command response; derive CPU/network rates only from valid monotonic pairs.
pub fn parse(output: &str, previous: Option<&Sample>, timestamp: i64) -> Sample {
    let mut sections: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut section = String::new();
    for line in output.lines() {
        if line.starts_with("__MANTASH_") && line.ends_with("__") {
            section = line
                .trim_start_matches("__MANTASH_")
                .trim_end_matches("__")
                .into();
            sections.entry(section.clone()).or_default();
        } else {
            sections.entry(section.clone()).or_default().push(line);
        }
    }
    let mut sample = Sample {
        timestamp,
        ..Default::default()
    };
    let lines = |name: &str| sections.get(name).cloned().unwrap_or_default();
    let os = lines("OS");
    sample.system = os.first().unwrap_or(&"").to_string();
    sample.kernel = os.get(1).unwrap_or(&"").to_string();
    sample.hostname = os.get(2).unwrap_or(&"").to_string();
    sample.uptime = os
        .get(3)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.);
    if sample.system != "Linux" {
        sample.errors.insert(
            "system".into(),
            "System monitoring requires a Linux SSH server".into(),
        );
        return sample;
    }
    let current_boot = lines("BOOT").first().map(|s| s.to_string());
    let previous = previous.filter(|old| old.boot_id.is_none() || old.boot_id == current_boot);
    for line in lines("CPU") {
        let mut fields = line.split_whitespace();
        let name = fields.next().unwrap_or("");
        if !name.starts_with("cpu") {
            continue;
        }
        let values: Vec<u64> = fields.take(8).filter_map(|s| s.parse().ok()).collect();
        if values.len() < 4 {
            continue;
        }
        let total: u64 = values.iter().sum();
        let idle = values[3] + values.get(4).copied().unwrap_or(0);
        let percent = previous
            .filter(|p| p.uptime <= sample.uptime)
            .and_then(|p| p.cpu.iter().find(|c| c.name == name))
            .and_then(|p| {
                let dt = total.checked_sub(p.total)?;
                let di = idle.checked_sub(p.idle)?;
                (dt > 0).then(|| 100. * (dt.saturating_sub(di)) as f64 / dt as f64)
            });
        sample.cpu.push(Cpu {
            name: name.into(),
            total,
            idle,
            percent,
        });
    }
    if sample.cpu.is_empty() {
        sample.errors.insert("cpu".into(), lines("CPU").join("\n"));
    }
    let mem: BTreeMap<&str, u64> = lines("MEM")
        .iter()
        .filter_map(|s| {
            let mut f = s.split_whitespace();
            let key = f.next()?.trim_end_matches(':');
            let value = f.next()?.parse::<u64>().ok()? * 1024;
            Some((key, value))
        })
        .collect();
    if let Some(total) = mem.get("MemTotal") {
        sample.memory = Some(Memory {
            total: *total,
            available: mem.get("MemAvailable").copied().unwrap_or_else(|| {
                mem.get("MemFree").copied().unwrap_or(0)
                    + mem.get("Buffers").copied().unwrap_or(0)
                    + mem.get("Cached").copied().unwrap_or(0)
            }),
            swap_total: mem.get("SwapTotal").copied().unwrap_or(0),
            swap_free: mem.get("SwapFree").copied().unwrap_or(0),
        });
    } else {
        sample
            .errors
            .insert("memory".into(), lines("MEM").join("\n"));
    }
    for line in lines("NET") {
        let Some((name, values)) = line.split_once(':') else {
            continue;
        };
        let numbers: Vec<u64> = values
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if numbers.len() < 9 {
            continue;
        }
        let received = numbers[0];
        let sent = numbers[8];
        let name = name.trim();
        let rates = previous.filter(|p| p.uptime < sample.uptime).and_then(|p| {
            let old = p.network.iter().find(|n| n.name == name)?;
            let seconds = sample.uptime - p.uptime;
            Some((
                received.checked_sub(old.received)? as f64 / seconds,
                sent.checked_sub(old.sent)? as f64 / seconds,
            ))
        });
        sample.network.push(Network {
            name: name.into(),
            received,
            sent,
            receive_rate: rates.map(|r| r.0),
            send_rate: rates.map(|r| r.1),
        });
    }
    if sample.network.is_empty() {
        sample
            .errors
            .insert("network".into(), lines("NET").join("\n"));
    }
    for line in lines("DISK").into_iter().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 6 {
            continue;
        }
        if let (Ok(total), Ok(used), Ok(available)) = (
            f[1].parse::<u64>(),
            f[2].parse::<u64>(),
            f[3].parse::<u64>(),
        ) {
            sample.disks.push(Disk {
                filesystem: f[0].into(),
                total: total * 1024,
                used: used * 1024,
                available: available * 1024,
                mount: f[5..].join(" "),
            });
        }
    }
    if sample.disks.is_empty() {
        sample
            .errors
            .insert("disk".into(), lines("DISK").join("\n"));
    }
    let boot = lines("BOOT");
    sample.boot_id = boot
        .first()
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .map(|id| id.to_string());
    let clock = boot
        .get(1)
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|c| *c > 0);
    let boot_time = boot.get(2).and_then(|s| s.parse::<i64>().ok());
    let starts = |section| -> BTreeMap<u32, u64> {
        lines(section)
            .iter()
            .filter_map(|line| {
                let mut f = line.split_whitespace();
                Some((f.next()?.parse().ok()?, f.next()?.parse().ok()?))
            })
            .collect()
    };
    let before = starts("START_BEFORE");
    let after = starts("START_AFTER");
    let v2 = sections.contains_key("PROCESS_V2");
    let section = if v2 { "PROCESS_V2" } else { "PROCESS" };
    for line in lines(section).into_iter().skip(if v2 { 0 } else { 1 }) {
        let f: Vec<_> = line.split_whitespace().collect();
        if f.len() < if v2 { 8 } else { 7 } {
            continue;
        }
        if let (Ok(pid), Ok(parent), Ok(cpu), Ok(memory), Ok(rss)) = (
            f[0].parse::<u32>(),
            f[1].parse(),
            f[2].parse(),
            f[3].parse(),
            f[4].parse::<u64>(),
        ) {
            let start = before
                .get(&pid)
                .filter(|start| after.get(&pid) == Some(start))
                .copied();
            let identity = start
                .zip(sample.boot_id.as_ref())
                .map(|(start_ticks, boot_id)| crate::processes::Identity {
                    pid,
                    start_ticks,
                    boot_id: boot_id.clone(),
                });
            sample.processes.push(Process {
                pid,
                parent,
                cpu,
                memory,
                rss: rss.saturating_mul(1024),
                state: f[5].into(),
                user: if v2 { f[6].into() } else { String::new() },
                started_at: start
                    .zip(clock)
                    .zip(boot_time)
                    .map(|((ticks, hz), boot)| boot.saturating_add((ticks / hz) as i64)),
                identity,
                command: f[if v2 { 7 } else { 6 }..].join(" "),
            });
        }
    }
    if sample.processes.is_empty() {
        sample
            .errors
            .insert("processes".into(), lines(section).join("\n"));
    }
    let port_lines = lines("PORT");
    if !sections.contains_key("PORT") {
        sample
            .errors
            .insert("ports".into(), "Port sampling returned no section".into());
    } else {
        match parse_ports(&port_lines) {
            Ok(ports) => sample.ports = ports,
            Err(error) => {
                sample.errors.insert("ports".into(), error);
            }
        }
    }
    sample
}

/// Display binary byte units consistently across files and system tools.
pub fn bytes(value: u64) -> String {
    let mut n = value as f64;
    let mut unit = 0;
    // Decimal units everywhere (disk details, file sizes, transfers, rates):
    // one scheme — B/KB/MB/GB/TB at 1000 per step.
    let units = ["B", "KB", "MB", "GB", "TB"];
    while n >= 1000. && unit < units.len() - 1 {
        n /= 1000.;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{n:.1} {}", units[unit])
    }
}
