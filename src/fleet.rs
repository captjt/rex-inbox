//! The machines the hub follows: this one, every host in `rex hosts`, and
//! any extras in `REX_INBOX_HOSTS`. The list is read again every so often
//! (and at once when a client asks for the hosts), so a host added to or
//! removed from Rex is followed or dropped without restarting the hub.
//!
//! `REX_INBOX_HOSTS` is a comma-separated list of `label=server`, or a bare
//! `label` (reached as `rex -S label`). It adds hosts Rex doesn't list, such
//! as a test server on a socket of its own.

use std::collections::BTreeMap;
use std::time::Duration;
use tokio::task::AbortHandle;

use crate::hub::{HubEvent, Shared, watch_host};
use crate::rex::Rex;

/// How often to read `rex hosts` again on its own.
const EVERY: Duration = Duration::from_secs(30);

/// The extra hosts `REX_INBOX_HOSTS` names.
fn extra_hosts(spec: &str) -> Vec<Rex> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| match s.split_once('=') {
            Some((label, server)) => Rex::remote(label.trim(), server.trim()),
            None => Rex::remote(s, s),
        })
        .collect()
}

/// The hosts to follow: this machine first, then each other host once.
/// `listed` is `None` when `rex hosts` couldn't be read; the hosts already
/// followed stay then, rather than dropping on a hiccup.
fn wanted(listed: Option<Vec<String>>, current: &[Rex], extra: Vec<Rex>) -> Vec<Rex> {
    let mut out = vec![Rex::local()];
    let others: Vec<Rex> = match listed {
        Some(labels) => labels.iter().map(|l| Rex::remote(l, l)).collect(),
        None => current.iter().filter(|r| r.host.is_some()).cloned().collect(),
    };
    for r in others.into_iter().chain(extra) {
        // "local" is this machine's name in records.
        if r.name() == "local" || out.iter().any(|o| o.name() == r.name()) {
            continue;
        }
        out.push(r);
    }
    out
}

pub async fn run(shared: Shared) {
    let me = Rex::local();
    let wake = shared.lock().fleet.clone();
    let mut running: BTreeMap<String, (Rex, AbortHandle)> = BTreeMap::new();
    loop {
        let listed = match me.host_labels().await {
            Ok(l) => Some(l),
            Err(e) => {
                if !running.is_empty() {
                    say!("rex hosts: {e:#}");
                }
                None
            }
        };
        let current: Vec<Rex> = running.values().map(|(r, _)| r.clone()).collect();
        let extra = extra_hosts(&std::env::var("REX_INBOX_HOSTS").unwrap_or_default());
        let want = wanted(listed, &current, extra);

        let stale: Vec<String> =
            running.iter().filter(|(name, (r, _))| !want.iter().any(|w| w.name() == *name && w == r)).map(|(n, _)| n.clone()).collect();
        for name in stale {
            if let Some((_, task)) = running.remove(&name) {
                task.abort();
            }
            drop_host(&shared, &name);
            say!("no longer following {name}");
        }
        for r in want {
            let name = r.name().to_string();
            if running.contains_key(&name) {
                continue;
            }
            shared.lock().rexes.insert(name.clone(), r.clone());
            let task = tokio::spawn(watch_host(shared.clone(), r.clone()));
            running.insert(name, (r, task.abort_handle()));
        }

        tokio::select! {
            _ = tokio::time::sleep(EVERY) => {}
            _ = wake.notified() => {}
        }
    }
}

/// Forget a host Rex no longer lists: its panes, and the alerts they raised.
fn drop_host(shared: &Shared, host: &str) {
    let mut core = shared.lock();
    core.rexes.remove(host);
    core.hosts.remove(host);
    let blocks: Vec<String> = match core.store.live_alerts_for_host(host) {
        Ok(v) => v.into_iter().map(|a| a.block_id).collect(),
        Err(_) => Vec::new(),
    };
    core.reconcile(host, blocks);
    core.emit(HubEvent::HostDisconnected { host: host.to_string(), error: Some("removed from rex hosts".into()) });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[Rex]) -> Vec<&str> {
        v.iter().map(Rex::name).collect()
    }

    #[test]
    fn extras_parse() {
        let v = extra_hosts(" sandbox=unix:///tmp/s.sock, studio ,,");
        assert_eq!(names(&v), ["sandbox", "studio"]);
        assert_eq!(v[0], Rex::remote("sandbox", "unix:///tmp/s.sock"));
        assert_eq!(v[1], Rex::remote("studio", "studio"));
    }

    #[test]
    fn this_machine_first_and_each_host_once() {
        let listed = Some(vec!["studio".into(), "local".into(), "pi".into()]);
        let w = wanted(listed, &[], extra_hosts("studio=unix:///x,lab"));
        assert_eq!(names(&w), ["local", "studio", "pi", "lab"]);
        // Rex's own entry wins over an extra of the same name.
        assert_eq!(w[1], Rex::remote("studio", "studio"));
    }

    #[test]
    fn hosts_stay_when_rex_hosts_fails() {
        let current = vec![Rex::local(), Rex::remote("studio", "studio")];
        assert_eq!(names(&wanted(None, &current, vec![])), ["local", "studio"]);
        assert_eq!(names(&wanted(Some(vec![]), &current, vec![])), ["local"]);
    }
}
