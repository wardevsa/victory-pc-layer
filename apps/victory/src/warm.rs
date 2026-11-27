//! `victory warm`: host pipelines made ahead of a run (`known_pipelines`), in
//! processes of their own: a compiler that crashes or hangs costs one pipeline.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use victory_hle::known_pipelines as known;

/// Pipelines a process makes before another takes its place: the compiler's
/// and the translations' memory goes with it.
const AT_ONCE: usize = 40;
/// A pipeline that takes longer is taken for one the compiler hangs on.
const LONGEST: Duration = Duration::from_secs(180);

/// As the runs count one that takes long.
const SLOW_MS: f64 = 1000.0;

fn folder() -> Result<PathBuf> {
    let cfg = victory_core::Config::load()?;
    Ok(cfg.cache_subdir("shaders")?.with_file_name("pipelines"))
}

/// What to say when there is nothing to make: no pipeline written down, or
/// (`--used`) none an earlier run drew with.
fn empty_hint(known: usize, early: usize, used: bool) -> Option<&'static str> {
    if known == 0 {
        Some("no pipelines recorded: play once with VICTORY_PIPELINE_BOOK=1, e.g. play.ps1 -Record")
    } else if used && early == 0 {
        Some("no pipeline an earlier run drew with (early-pipelines.txt): play once, or leave out --used")
    } else {
        None
    }
}

pub fn run(
    jobs: Option<usize>,
    limit: Option<usize>,
    again: bool,
    used: bool,
    list_no_host: bool,
    these: Option<String>,
) -> Result<()> {
    let dir = folder()?;
    if let Some(these) = these {
        return make(&dir, &these);
    }
    let began = Instant::now();
    victory_hle::known_pipelines::use_translations(dir.with_file_name("shaders"));
    let fingerprint = known::fingerprint();
    let done_file = dir.join("warm-done.txt");
    let bad_file = dir.join("warm-bad.txt");
    let keys = known::keys(&dir);
    let listed = std::fs::read_to_string(&done_file).unwrap_or_default();
    let done = recorded_keys(&listed, &fingerprint);
    let bad = std::fs::read_to_string(&bad_file).unwrap_or_default();
    let bad = recorded_keys(&bad, &fingerprint);
    let early = std::fs::read_to_string(dir.join("early-pipelines.txt")).unwrap_or_default();
    let early: HashSet<&str> = early.lines().collect();
    // Those earlier runs drew with first, the others in no order (by key).
    let mut all: Vec<usize> = (0..keys.len())
        .filter(|&n| {
            let key = keys[n].as_str();
            (again || !done.contains(key)) && !bad.contains(key) && (!used || early.contains(key))
        })
        .collect();
    all.sort_by_key(|&n| (!early.contains(keys[n].as_str()), &keys[n]));
    all.truncate(limit.unwrap_or(usize::MAX));
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let jobs = jobs.unwrap_or(cores * 2 / 3).max(1);
    println!(
        "{} pipelines known, {} made before ({fingerprint}), {} bad; {} to make, {jobs} processes",
        keys.len(),
        done.len(),
        bad.len(),
        all.len()
    );
    if let Some(hint) = empty_hint(keys.len(), early.len(), used) {
        println!("{hint}");
    }
    let chunks: Vec<&[usize]> = all.chunks(AT_ONCE).collect();
    let next = AtomicUsize::new(0);
    let failure = Mutex::new(None);
    let state = Mutex::new((Vec::<f64>::new(), 0usize, 0usize));
    let done_out = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&done_file)?);
    // (Read back by the runs: these are made beside the others, not before.)
    let slow_out = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(dir.join("slow-pipelines.txt"))?);
    let bad_out = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&bad_file)?);
    std::thread::scope(|s| {
        for _ in 0..jobs.min(chunks.len()) {
            s.spawn(|| {
                while let Some(&chunk) = chunks.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if lock(&failure).is_some() {
                        return;
                    }
                    let mut rest = chunk;
                    while !rest.is_empty() {
                        let result = child(rest, |n, ms, there| {
                            if let Some(line) = no_host_line(list_no_host, there, &fingerprint, &keys[n]) {
                                println!("{line}");
                            }
                            if ms >= SLOW_MS {
                                let _ = writeln!(lock(&slow_out), "\n{} {ms:.0}", keys[n]);
                            }
                            let mut state = lock(&state);
                            state.0.push(ms);
                            state.1 += usize::from(!there);
                            let count = state.0.len();
                            if count.is_multiple_of(1000) {
                                println!("{count} of {}, {:.0} s", all.len(), began.elapsed().as_secs_f64());
                            }
                            drop(state);
                            let _ = writeln!(lock(&done_out), "{fingerprint} {}", keys[n]);
                        });
                        let (made, ended) = match result {
                            Ok(done) => done,
                            Err(e) => {
                                lock(&failure).get_or_insert(e);
                                return;
                            }
                        };
                        rest = &rest[made..];
                        match (ended, rest.split_first()) {
                            (Some(why), Some((&n, after))) => {
                                // The one it was at.
                                println!("pipeline {n} {}: {why}", keys[n]);
                                let _ = writeln!(lock(&bad_out), "{fingerprint} {} {why}", keys[n]);
                                lock(&state).2 += 1;
                                rest = after;
                            }
                            _ => break,
                        }
                    }
                }
            });
        }
    });
    let (mut times, refused, crashed) = state.into_inner().unwrap_or_else(|e| e.into_inner());
    times.sort_by(f64::total_cmp);
    let at = |q: f64| times.get((times.len() as f64 * q) as usize).copied().unwrap_or(0.0);
    println!(
        "{} made in {:.0} s ({refused} have no host pipeline, {crashed} ended their process); ms each: median {:.1}, p90 {:.1}, p99 {:.1}, most {:.0}, sum {:.0} s",
        times.len(),
        began.elapsed().as_secs_f64(),
        at(0.5),
        at(0.9),
        at(0.99),
        times.last().copied().unwrap_or(0.0),
        times.iter().sum::<f64>() / 1e3
    );
    match failure.into_inner().unwrap_or_else(|e| e.into_inner()) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Collects keys recorded with the current translator settings and driver.
/// Workers retry failures from an older fingerprint or an unversioned log.
fn recorded_keys<'a>(records: &'a str, fingerprint: &str) -> HashSet<&'a str> {
    records
        .lines()
        .filter_map(|line| {
            let (of, rest) = line.split_once(' ')?;
            if of != fingerprint {
                return None;
            }
            rest.split_ascii_whitespace().next()
        })
        .collect()
}

// The same confirmed refusal on --again prints the same pair, even if its archive index changes.
fn no_host_line(list: bool, made_host: bool, fingerprint: &str, key: &str) -> Option<String> {
    (list && !made_host).then(|| format!("no-host {fingerprint} {key}"))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
struct Progress {
    made: usize,
    active: bool,
}

impl Progress {
    fn record(&mut self, these: &[usize], line: &str) -> Result<Option<(usize, f64, bool)>> {
        let mut words = line.split_ascii_whitespace();
        let kind = match words.next() {
            Some(kind @ ("at" | "made")) => kind,
            _ => return Ok(None),
        };
        let n: usize = words.next().context("missing warm worker index")?.parse()?;
        anyhow::ensure!(
            these.get(self.made) == Some(&n),
            "unexpected warm worker index {n}"
        );
        if kind == "at" {
            anyhow::ensure!(
                !self.active && words.next().is_none(),
                "invalid warm worker start for {n}"
            );
            self.active = true;
            return Ok(None);
        }
        let ms: f64 = words
            .next()
            .context("missing warm worker duration")?
            .parse()?;
        let there = match words.next() {
            Some("0") => false,
            Some("1") => true,
            _ => anyhow::bail!("invalid warm worker result for {n}"),
        };
        anyhow::ensure!(
            self.active && ms.is_finite() && ms >= 0.0 && words.next().is_none(),
            "invalid warm worker completion for {n}"
        );
        self.active = false;
        self.made += 1;
        Ok(Some((n, ms, there)))
    }

    fn finish(self, count: usize, failure: Option<String>) -> Result<(usize, Option<String>)> {
        if let Some(reason) = failure {
            if self.active {
                // A failed worker identified exactly which pipeline it was making.
                return Ok((self.made, Some(reason)));
            }
            anyhow::bail!(
                "warm worker ended between pipelines after {} of {count}: {reason}",
                self.made
            );
        }
        anyhow::ensure!(
            self.made == count,
            "warm worker exited successfully after reporting {} of {count} pipelines",
            self.made
        );
        Ok((self.made, None))
    }
}

/// Has a process make `these`; how many it made, and how it ended if not well.
fn child(
    these: &[usize],
    mut each: impl FnMut(usize, f64, bool),
) -> Result<(usize, Option<String>)> {
    let list: Vec<String> = these.iter().map(usize::to_string).collect();
    let spawned = std::env::current_exe().and_then(|exe| {
        use std::os::windows::process::CommandExt;
        const IDLE_PRIORITY_CLASS: u32 = 0x40;
        Command::new(exe)
            .args(["warm", "--these", &list.join(",")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(IDLE_PRIORITY_CLASS)
            .spawn()
    });
    let mut process = spawned.context("starting the pipeline warm worker")?;
    let Some(out) = process.stdout.take() else {
        let _ = process.kill();
        let _ = process.wait();
        anyhow::bail!("warm worker has no output pipe");
    };
    let process = Mutex::new(process);
    let (heard, over) = (Mutex::new(Instant::now()), AtomicBool::new(false));
    let hung = AtomicBool::new(false);
    let mut progress = Progress::default();
    let read = std::thread::scope(|s| {
        s.spawn(|| {
            while !over.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
                if lock(&heard).elapsed() > LONGEST {
                    hung.store(true, Ordering::Relaxed);
                    let _ = lock(&process).kill();
                    return;
                }
            }
        });
        let read = (|| -> Result<()> {
            for line in BufReader::new(out).lines() {
                let line = line.context("reading warm worker output")?;
                *lock(&heard) = Instant::now();
                if let Some((n, ms, there)) = progress.record(these, &line)? {
                    each(n, ms, there);
                }
            }
            Ok(())
        })();
        if read.is_err() {
            let _ = lock(&process).kill();
        }
        over.store(true, Ordering::Relaxed);
        read
    });
    let status = lock(&process)
        .wait()
        .context("waiting for the pipeline warm worker")?;
    read?;
    if status.success() {
        return progress.finish(these.len(), None);
    }
    let why = if hung.load(Ordering::Relaxed) {
        format!("hung for {} s", LONGEST.as_secs())
    } else {
        status.to_string()
    };
    progress.finish(these.len(), Some(why))
}

/// What such a process does.
fn make(dir: &Path, these: &str) -> Result<()> {
    let these: Vec<usize> = these.split(',').map(|n| n.parse().context("--these")).collect::<Result<_>>()?;
    let cfg = victory_core::Config::load()?;
    victory_hle::known_pipelines::use_translations(cfg.cache_subdir("shaders")?);
    let mut out = std::io::stdout();
    known::make(dir, &these, |n, came| {
        let told = match came {
            None => writeln!(out, "at {n}"),
            Some((ms, there)) => writeln!(out, "made {n} {ms:.2} {}", u8::from(there)),
        };
        if told.and_then(|()| out.flush()).is_err() {
            // Whoever asked is gone.
            std::process::exit(1);
        }
    });
    // Without what a run's end does (nothing to save, the host's threads).
    let _ = out.flush();
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_no_host_results_is_opt_in() {
        use clap::Parser;
        for (extra, expected) in [(None, false), (Some("--list-no-host"), true)] {
            let args = ["victory", "warm"].into_iter().chain(extra);
            let cli = crate::Cli::try_parse_from(args).unwrap();
            assert!(matches!(cli.command, crate::Command::Warm { list_no_host, .. } if list_no_host == expected));
        }
    }

    #[test]
    fn only_confirmed_no_host_results_are_listed_on_request() {
        let outcomes = [("one", true), ("two", false), ("three", true)];
        let lines = |list, fingerprint| {
            outcomes
                .iter()
                .filter_map(|(key, host)| no_host_line(list, *host, fingerprint, key))
                .collect::<Vec<_>>()
        };
        assert!(lines(false, "build-a").is_empty());
        assert_eq!(lines(true, "build-a"), ["no-host build-a two"]);
        assert_eq!(lines(true, "build-b"), ["no-host build-b two"]);
        assert_eq!(no_host_line(true, false, "build-a", "two").as_deref(), Some("no-host build-a two"));
    }

    #[test]
    fn empty_hint_only_when_there_is_nothing_to_make() {
        assert!(empty_hint(0, 0, false).is_some());
        assert!(empty_hint(0, 5, true).is_some());
        assert!(empty_hint(3, 0, true).is_some());
        assert!(empty_hint(3, 0, false).is_none());
        assert!(empty_hint(3, 2, true).is_none());
    }

    #[test]
    fn warm_results_follow_the_translator_and_driver() {
        let records = "current crashed exit code: 101\n\
                       current finished\n\
                       previous slow timed out\n\
                       legacy-key exit code: 101\n\
                       current\n";
        assert_eq!(recorded_keys(records, "current"), HashSet::from(["crashed", "finished"]));
        assert_eq!(recorded_keys(records, "previous"), HashSet::from(["slow"]));
        assert!(recorded_keys(records, "new").is_empty());
    }

    #[test]
    fn only_a_failed_worker_with_a_current_pipeline_marks_it_bad() {
        let chunk = [9, 2, 7];
        let mut p = Progress::default();
        p.record(&chunk, "at 9").unwrap();
        assert_eq!(
            p.record(&chunk, "made 9 3.00 0").unwrap(),
            Some((9, 3.0, false))
        );
        assert!(p.finish(3, Some("exit 101".into())).is_err());

        let mut p = Progress::default();
        p.record(&chunk, "at 9").unwrap();
        p.record(&chunk, "made 9 1.00 1").unwrap();
        p.record(&chunk, "at 2").unwrap();
        assert_eq!(
            p.finish(3, Some("exit 101".into())).unwrap(),
            (1, Some("exit 101".into()))
        );
    }

    #[test]
    fn incomplete_or_out_of_order_results_leave_the_rest_retryable() {
        let chunk = [7, 3];
        assert!(Progress::default().finish(2, None).is_err());
        assert!(
            Progress::default()
                .finish(2, Some("exit 1".into()))
                .is_err()
        );
        let mut p = Progress::default();
        assert!(p.record(&chunk, "made 7 0.1 1").is_err());
        assert!(p.record(&chunk, "at 3").is_err());
        assert!(p.record(&chunk, "diagnostic output").unwrap().is_none());
        p.record(&chunk, "at 7").unwrap();
        assert!(p.record(&chunk, "made 3 0.0 0").is_err());
        assert!(p.record(&chunk, "made 7 0.0 ?").is_err());
        assert!(p.finish(2, None).is_err());
        let mut p = Progress::default();
        for n in chunk {
            p.record(&chunk, &format!("at {n}")).unwrap();
            p.record(&chunk, &format!("made {n} 0.00 0")).unwrap();
        }
        assert_eq!(p.finish(2, None).unwrap(), (2, None));
    }
}
