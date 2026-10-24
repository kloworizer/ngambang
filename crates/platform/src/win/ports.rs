//! Listening TCP ports on this machine (SPEC §6.4).

use ngambang_core::model::PortEntry;
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

use super::util::{process_path, OwnedHandle};

fn table(family: u16) -> Vec<u8> {
    let mut size = 0u32;
    unsafe {
        let r = GetExtendedTcpTable(None, &mut size, false, family as u32, TCP_TABLE_OWNER_PID_LISTENER, 0);
        if r != ERROR_INSUFFICIENT_BUFFER.0 && r != NO_ERROR.0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size as usize + 1024];
        let mut size = buf.len() as u32;
        if GetExtendedTcpTable(Some(buf.as_mut_ptr().cast()), &mut size, false, family as u32, TCP_TABLE_OWNER_PID_LISTENER, 0)
            != NO_ERROR.0
        {
            return Vec::new();
        }
        buf
    }
}

fn port(raw: u32) -> u16 {
    u16::from_be(raw as u16)
}

fn can_stop(pid: u32) -> bool {
    unsafe { OpenProcess(PROCESS_TERMINATE, false, pid).map(|h| drop(OwnedHandle(h))).is_ok() }
}

/// Listeners on loopback or any address. Filtering by port and process is the model's job.
pub fn scan() -> Vec<PortEntry> {
    let mut found: Vec<(u16, u32)> = Vec::new();
    unsafe {
        let b4 = table(AF_INET.0);
        if b4.len() >= 4 {
            let t = &*(b4.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            let rows = std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize);
            for r in rows {
                let addr = u32::from_be(r.dwLocalAddr);
                if addr == 0 || addr >> 24 == 127 {
                    found.push((port(r.dwLocalPort), r.dwOwningPid));
                }
            }
        }
        let b6 = table(AF_INET6.0);
        if b6.len() >= 4 {
            let t = &*(b6.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
            let rows = std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize);
            for r in rows {
                let a = r.ucLocalAddr;
                let any = a.iter().all(|b| *b == 0);
                let loopback = a[..15].iter().all(|b| *b == 0) && a[15] == 1;
                if any || loopback {
                    found.push((port(r.dwLocalPort), r.dwOwningPid));
                }
            }
        }
    }
    found.sort();
    found.dedup();
    let mut names = std::collections::HashMap::new();
    found
        .into_iter()
        .map(|(port, pid)| {
            let (process, stop) = names
                .entry(pid)
                .or_insert_with(|| {
                    let name =
                        process_path(pid).map(|p| p.rsplit('\\').next().unwrap_or(&p).to_string()).unwrap_or_else(|| "system".into());
                    (name, can_stop(pid))
                })
                .clone();
            PortEntry { port, pid, process, can_stop: stop }
        })
        .collect()
}

pub fn kill(pid: u32) {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let h = OwnedHandle(h);
            let _ = TerminateProcess(h.0, 1);
        }
    }
}
