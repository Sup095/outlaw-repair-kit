//! Stress and burn-in: making the machine work hard on purpose.
//!
//! Everything else in this tool observes. This is the one part that acts on
//! the hardware -- it loads every processor core and fills most of the free
//! memory, deliberately, for as long as it is asked to. It exists because a
//! whole class of fault is invisible to observation: memory that corrupts one
//! bit an hour, a processor that computes wrongly only when hot, a cooling
//! system that was fine when the machine was new and is now full of dust. None
//! of those show up in a log. They show up as a computer that is "just
//! unreliable", and they are the faults people give up on and replace working
//! machines over.
//!
//! ## Why this is not part of a scan
//!
//! It is asked for, every time, on its own. It is never something a scan does
//! because you picked the thorough option. Choosing "check my computer
//! carefully" is not consent to have that computer pinned at full load and
//! heated for ten minutes, and a tool that treated it as consent would be
//! doing something to somebody's machine that they did not ask for -- which is
//! the one thing this project does not do.
//!
//! ## The rails
//!
//! * **The temperature is watched the whole time**, and the run stops if any
//!   part of the machine reaches the temperature that machine says is critical
//!   for it. If nothing can be read, the report says nothing was watching --
//!   it does not quietly imply the machine stayed cool.
//! * **A gigabyte of memory is always left alone**, whatever share is asked
//!   for, so the machine keeps running normally and does not start paging.
//!   Memory that cannot be allocated is memory not tested, not a crash.
//! * **Stopping is instant and always available.** Blocks are milliseconds
//!   long precisely so that nobody has to wait to stop something that is
//!   heating their computer.
//! * **Nothing is written to disk and nothing is changed.** The machine is
//!   exactly as it was afterwards, hotter.
//!
//! ## On the duration
//!
//! Nothing in this tool is ever cut off for taking too long, and that has not
//! changed here. The duration is not a deadline on work that would otherwise
//! continue -- it *is* the work: "load this machine for ten minutes" is the
//! request. A block already running is always allowed to finish, the run ends
//! when the requested work is done, and the report says how long it actually
//! ran rather than how long it was asked for.
//!
//! ## What a clean result means
//!
//! Less than people will want it to mean, so the report says it plainly: the
//! memory it could reach held what was written to it, and every core agreed
//! with itself, for as long as it ran. Intermittent faults are intermittent.
//! A clean ten minutes narrows the problem; it does not clear the hardware.

pub mod cpu;
pub mod memory;
pub mod thermal;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::Platform;
use crate::util::format_bytes;

pub use memory::{Mismatch, Pattern};
pub use thermal::Heat;

/// How long to run when nobody says otherwise.
///
/// Long enough to get the machine properly hot, which is when marginal
/// hardware misbehaves, and short enough that somebody will actually sit
/// through it.
pub const DEFAULT_DURATION: Duration = Duration::from_secs(10 * 60);

/// A run shorter than this is reported as too short to conclude much from.
///
/// Not a floor -- a short run is allowed, and is genuinely useful for
/// confirming the test itself works. It is the point below which a clean
/// result should not be read as reassurance.
pub const BRIEF: Duration = Duration::from_secs(120);

/// How often to read the temperature and report progress.
const SAMPLE: Duration = Duration::from_secs(2);

/// What to work.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub cpu: bool,
    pub memory: bool,
    pub duration: Duration,
    /// The share of currently free memory to test, before the reserve is taken
    /// out of it.
    pub memory_share: f64,
    /// How many cores to work. `None` means all of them.
    pub threads: Option<usize>,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            cpu: true,
            memory: true,
            duration: DEFAULT_DURATION,
            memory_share: memory::DEFAULT_SHARE,
            threads: None,
        }
    }
}

impl Plan {
    /// Whether there is anything to do.
    pub fn is_empty(&self) -> bool {
        !self.cpu && !self.memory
    }

    pub fn describe(&self) -> String {
        match (self.cpu, self.memory) {
            (true, true) => "processor and memory".to_string(),
            (true, false) => "processor".to_string(),
            (false, true) => "memory".to_string(),
            (false, false) => "nothing".to_string(),
        }
    }
}

/// Why the run ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ending", rename_all = "kebab-case")]
pub enum Ending {
    /// The requested work was done.
    Completed,
    /// Somebody stopped it. Not a failure, and not reported as one.
    Cancelled,
    /// A part of the machine reached the temperature it should not be pushed
    /// past. The most important result this test can produce.
    TooHot {
        sensor: String,
        reached_c: f32,
        ceiling_c: f32,
    },
}

/// Something the machine got wrong.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fault {
    /// `cpu` or `memory`.
    pub kind: String,
    /// Which core, or where in the tested region.
    pub part: String,
    pub detail: String,
}

/// What the processors did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuSummary {
    pub threads: usize,
    pub blocks: u64,
    /// Blocks that came back wrong. Any number above zero is a hardware fault.
    pub wrong: u64,
}

/// What the memory did, or why it was not tested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "memory", rename_all = "kebab-case")]
pub enum MemorySummary {
    Ran {
        /// How much was actually allocated and tested, which can be less than
        /// asked for if the machine would not give it all up.
        bytes: u64,
        /// How many patterns were written across the whole region and read
        /// back in full.
        ///
        /// Counted per pattern rather than per complete cycle of all five,
        /// because a cycle over a large region takes long enough that a short
        /// run finishes none -- and reporting "0" for a run that did check
        /// two patterns is worse than useless. It reads as though the memory
        /// was left untested, next to a result saying nothing went wrong.
        patterns: u64,
        mismatches: Vec<Mismatch>,
    },
    /// Deliberately not run, with the reason. Never a silent absence.
    NotRun { reason: String },
}

/// What happened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressReport {
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    pub asked_for_secs: u64,
    pub ran_for_secs: u64,
    pub ending: Ending,
    pub cpu: Option<CpuSummary>,
    pub memory: Option<MemorySummary>,
    /// The hottest each sensor got, hottest first.
    pub heat: Vec<Heat>,
    /// Whether the temperature could be read at all on this machine.
    pub watched_heat: bool,
    pub faults: Vec<Fault>,
}

impl StressReport {
    /// Whether the machine got everything right.
    ///
    /// Note that this is about correctness only. A run that overheated and was
    /// stopped can still be clean by this measure, and the report says both.
    pub fn clean(&self) -> bool {
        self.faults.is_empty()
    }

    /// How long it ran, as a person would say it.
    pub fn ran_for(&self) -> String {
        let seconds = self.ran_for_secs;
        if seconds < 90 {
            format!("{seconds} seconds")
        } else if seconds < 60 * 90 {
            format!("{} minutes", (seconds + 30) / 60)
        } else {
            format!("{:.1} hours", seconds as f64 / 3600.0)
        }
    }

    /// The hottest thing in the machine, if anything could be read.
    pub fn hottest(&self) -> Option<&Heat> {
        self.heat.first()
    }

    /// Everything worth telling somebody, in the same shape as a scan's
    /// findings -- so a stress result goes into the queue, the runbooks, the
    /// AI analysis, and the history through exactly the machinery that already
    /// exists, rather than through a second one built beside it.
    pub fn findings(&self) -> Vec<Finding> {
        let mut findings = Vec::new();

        for fault in &self.faults {
            let (category, id, title) = if fault.kind == "cpu" {
                (
                    Category::Cpu,
                    "stress.cpu-wrong-answer",
                    format!("{} returned the wrong answer under load", fault.part),
                )
            } else {
                (
                    Category::Memory,
                    "stress.memory-corruption",
                    "Memory did not return what was written to it".to_string(),
                )
            };
            findings.push(
                Finding::builder("stress", id)
                    .subject(fault.part.clone())
                    .severity(Severity::Critical)
                    .category(category)
                    .title(title)
                    .detail(format!(
                        "{} This is a hardware fault, not a software one: the machine was \
                         asked to repeat work it had already done and gave a different \
                         answer. A machine doing this corrupts whatever it happens to be \
                         working on -- files, archives, builds, saved games -- silently and \
                         at random, which is why it is usually mistaken for bad software.",
                        fault.detail
                    ))
                    .evidence("ran_for_seconds", self.ran_for_secs.to_string())
                    .evidence("kind", fault.kind.clone())
                    .evidence("part", fault.part.clone())
                    .remediation_hint(
                        "Stop using this machine for anything you care about losing until \
                         the cause is found. Nothing this tool can change in software will \
                         fix it.",
                    )
                    .triage(Triage::Queue)
                    .build(),
            );
        }

        if let Ending::TooHot {
            sensor,
            reached_c,
            ceiling_c,
        } = &self.ending
        {
            findings.push(
                Finding::builder("stress", "stress.overheated")
                    .subject(sensor.clone())
                    .severity(Severity::High)
                    .category(Category::Cpu)
                    .title(format!(
                        "{sensor} reached {reached_c:.0}C and the test was stopped"
                    ))
                    .detail(format!(
                        "The test stopped itself after {} because {sensor} reached \
                         {reached_c:.0}C, past the {ceiling_c:.0}C this machine should not be \
                         pushed beyond. A machine that cannot be worked hard without \
                         overheating will throttle itself under any real load, which is felt \
                         as the computer being fast for a minute and then slow, and over time \
                         it shortens the life of the parts getting hot. The usual causes are \
                         dust in the cooling path, a fan that has stopped, or thermal paste \
                         that has dried out -- all physical, none of them things this tool \
                         can change.",
                        self.ran_for()
                    ))
                    .evidence("sensor", sensor.clone())
                    .evidence("reached_c", format!("{reached_c:.1}"))
                    .evidence("ceiling_c", format!("{ceiling_c:.1}"))
                    .evidence("ran_for_seconds", self.ran_for_secs.to_string())
                    .remediation_hint(
                        "Check that the fans are turning and that the vents and heatsink are \
                         clear before anything else.",
                    )
                    .triage(Triage::Queue)
                    .build(),
            );
        }

        if !self.watched_heat {
            // Said out loud. An empty temperature list must never be read as a
            // machine that stayed cool.
            findings.push(
                Finding::builder("stress", "stress.no-temperature-readings")
                    .severity(Severity::Info)
                    .category(Category::Cpu)
                    .title("Nothing was watching the temperature during this test")
                    .detail(format!(
                        "This machine did not report any temperature that could be believed, \
                         so the run had no way to stop itself if the machine got too hot, and \
                         there is no peak temperature to show. {} It is not a fault; it does \
                         mean the heat half of this test did not happen.",
                        // Said precisely, because "no sensors" sends somebody looking
                        // for a driver when the real answer on Windows is usually that
                        // the reading needs administrator rights, and often that the
                        // board never publishes it at all.
                        if cfg!(windows) {
                            "On Windows this reading comes from the firmware through WMI, \
                             which needs administrator rights -- so running elevated may \
                             produce one. Many desktop motherboards do not publish it at \
                             all, whoever asks."
                        } else {
                            "That is usual inside virtual machines, and on machines whose \
                             sensor drivers are not loaded."
                        }
                    ))
                    .triage(Triage::None)
                    .build(),
            );
        }

        if let Some(MemorySummary::NotRun { reason }) = &self.memory {
            findings.push(
                Finding::builder("stress", "stress.memory-not-tested")
                    .severity(Severity::Info)
                    .category(Category::Memory)
                    .title("The memory was not tested")
                    .detail(reason.clone())
                    .triage(Triage::None)
                    .build(),
            );
        }

        if self.clean() && matches!(self.ending, Ending::Completed) {
            findings.push(self.clean_result());
        }

        findings
    }

    /// The "nothing went wrong" finding, written so that it cannot be
    /// mistaken for a clean bill of health.
    fn clean_result(&self) -> Finding {
        let mut what = Vec::new();
        if let Some(cpu) = &self.cpu {
            what.push(format!(
                "{} core{} completed {} blocks of arithmetic and every one of them came back \
                 correct",
                cpu.threads,
                if cpu.threads == 1 { "" } else { "s" },
                cpu.blocks
            ));
        }
        if let Some(MemorySummary::Ran {
            bytes, patterns, ..
        }) = &self.memory
        {
            // Nothing fully checked is said as nothing fully checked. The
            // alternative -- "read back 0 times without a single bit
            // changing" -- is a sentence that claims a clean result for work
            // that did not finish.
            what.push(if *patterns == 0 {
                format!(
                    "{} of memory was filled, but the run ended before any one pattern had \
                     been read back across the whole of it, so the memory was not fully \
                     checked even once",
                    format_bytes(*bytes)
                )
            } else {
                format!(
                    "{} of memory was written and read back under {} different pattern{}, \
                     without a single bit changing",
                    format_bytes(*bytes),
                    patterns,
                    if *patterns == 1 { "" } else { "s" }
                )
            });
        }

        let brevity = if Duration::from_secs(self.ran_for_secs) < BRIEF {
            " That was a short run, and a short run proves correspondingly little -- the \
             faults this looks for are intermittent, and the machine barely had time to get \
             warm."
        } else {
            ""
        };

        Finding::builder("stress", "stress.clean")
            .severity(Severity::Info)
            .category(Category::Performance)
            .title(format!("Nothing went wrong in {}", self.ran_for()))
            .detail(format!(
                "Over {}: {}.{brevity} What this does not mean: the hardware is not cleared. \
                 Faults of this kind are intermittent by nature, and only the memory the \
                 operating system was willing to hand this program could be tested -- \
                 anything already in use, including the operating system's own memory, was \
                 out of reach. A clean result narrows where a problem can be hiding. It does \
                 not prove there isn't one.",
                self.ran_for(),
                what.join("; ")
            ))
            .evidence("ran_for_seconds", self.ran_for_secs.to_string())
            .triage(Triage::None)
            .build()
    }
}

/// Progress from a running stress test.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum StressEvent {
    Started {
        seconds: u64,
        cpu_threads: usize,
        memory_bytes: u64,
        /// Whether the temperature can be read on this machine. Shown at the
        /// start rather than discovered in the report, because somebody about
        /// to heat their laptop should know before it starts whether anything
        /// is watching.
        watching_heat: bool,
    },
    Progress {
        elapsed_secs: u64,
        total_secs: u64,
        blocks: u64,
        memory_patterns: u64,
        hottest: Option<Heat>,
    },
    /// Sent the moment something goes wrong, not held back until the end.
    Fault {
        fault: Fault,
    },
    Finished {
        report: Box<StressReport>,
    },
}

/// A stress test, ready to run.
pub struct StressTest {
    plan: Plan,
    cancel: CancellationToken,
    events: Option<mpsc::UnboundedSender<StressEvent>>,
    thermometer: Option<thermal::Thermometer>,
}

impl StressTest {
    pub fn new(plan: Plan) -> Self {
        Self {
            plan,
            cancel: CancellationToken::new(),
            events: None,
            thermometer: None,
        }
    }

    /// Read the temperature from somewhere other than this machine's sensors.
    ///
    /// Exists so that the stop-on-heat rail can be exercised without
    /// overheating a computer.
    #[cfg(test)]
    fn with_thermometer(mut self, thermometer: thermal::Thermometer) -> Self {
        self.thermometer = Some(thermometer);
        self
    }

    pub fn with_events(mut self, sender: mpsc::UnboundedSender<StressEvent>) -> Self {
        self.events = Some(sender);
        self
    }

    /// The token that stops the run. Stopping is the user's decision, and this
    /// is the one place in the tool where it is also a safety control.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    fn emit(&self, event: StressEvent) {
        if let Some(sender) = &self.events {
            let _ = sender.send(event);
        }
    }

    /// Work the machine, and report what it did.
    pub async fn run(&mut self, platform: Arc<dyn Platform>) -> Result<StressReport> {
        let started_at = OffsetDateTime::now_utc();
        let started = Instant::now();
        let deadline = started + self.plan.duration;

        let threads = match self.plan.threads {
            Some(count) if self.plan.cpu => count.max(1),
            Some(_) => 0,
            None if self.plan.cpu => std::thread::available_parallelism()
                .map(|count| count.get())
                .unwrap_or(1),
            None => 0,
        };

        // Decided before anything starts, so the machine is measured as it is
        // rather than as this test has already made it.
        let memory_budget = if self.plan.memory {
            let info = {
                let platform = Arc::clone(&platform);
                tokio::task::spawn_blocking(move || platform.memory()).await??
            };
            Some(memory::budget(info.available_bytes, self.plan.memory_share))
        } else {
            None
        };

        let mut thermometer = self
            .thermometer
            .take()
            .unwrap_or_else(thermal::Thermometer::sensors);
        let watching_heat = !thermometer.read().is_empty();

        let memory_bytes = match &memory_budget {
            Some(memory::Budget::Test { bytes }) => *bytes,
            _ => 0,
        };
        self.emit(StressEvent::Started {
            seconds: self.plan.duration.as_secs(),
            cpu_threads: threads,
            memory_bytes,
            watching_heat,
        });

        let halt = Arc::new(AtomicBool::new(false));
        // Held for the rest of this function. If the run is dropped rather
        // than awaited -- a window closed mid-test, a caller that gave up, a
        // timeout somewhere above this -- every local goes with it, including
        // this, and the workers stop.
        //
        // Without it, dropping the run leaves every core pinned at full load
        // for the rest of the duration that was asked for, with the thermal
        // watch gone. Detached work is bad anywhere; detached work that is
        // heating somebody's computer with nothing left watching the
        // temperature is the worst version of it in this project.
        let _deadman = Deadman(Arc::clone(&halt));
        let blocks_done = Arc::new(AtomicU64::new(0));
        let patterns_done = Arc::new(AtomicU64::new(0));
        let (fault_tx, mut fault_rx) = mpsc::unbounded_channel::<Fault>();

        let mut workers = Vec::new();
        for core in 0..threads {
            let halt = Arc::clone(&halt);
            let counter = Arc::clone(&blocks_done);
            let faults = fault_tx.clone();
            workers.push(tokio::task::spawn_blocking(move || {
                burn_core(core, deadline, halt, counter, faults)
            }));
        }

        let memory_worker = match &memory_budget {
            Some(memory::Budget::Test { bytes }) => {
                let bytes = *bytes;
                let halt = Arc::clone(&halt);
                let counter = Arc::clone(&patterns_done);
                let faults = fault_tx.clone();
                Some(tokio::task::spawn_blocking(move || {
                    fill_memory(bytes, deadline, halt, counter, faults)
                }))
            }
            _ => None,
        };
        drop(fault_tx);

        // Everything below this point is the supervisor: it watches the
        // temperature, collects faults as they happen, and is the only thing
        // that can stop the workers early.
        let mut ending = Ending::Completed;
        let mut faults = Vec::new();
        let mut ticker = tokio::time::interval(SAMPLE);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            if halt.load(Ordering::Relaxed) {
                break;
            }
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    ending = Ending::Cancelled;
                    halt.store(true, Ordering::Relaxed);
                    break;
                }
                fault = fault_rx.recv() => {
                    match fault {
                        Some(fault) => {
                            self.emit(StressEvent::Fault { fault: fault.clone() });
                            faults.push(fault);
                        }
                        // Every worker has finished and dropped its sender.
                        None => break,
                    }
                }
                _ = ticker.tick() => {
                    let readings = thermometer.read();
                    if let Some((sensor, reached_c, ceiling_c)) =
                        thermal::Thermometer::too_hot(&readings)
                    {
                        ending = Ending::TooHot { sensor, reached_c, ceiling_c };
                        halt.store(true, Ordering::Relaxed);
                        break;
                    }
                    self.emit(StressEvent::Progress {
                        elapsed_secs: started.elapsed().as_secs(),
                        total_secs: self.plan.duration.as_secs(),
                        blocks: blocks_done.load(Ordering::Relaxed),
                        memory_patterns: patterns_done.load(Ordering::Relaxed),
                        hottest: thermometer.peaks().into_iter().next(),
                    });
                }
            }
        }
        halt.store(true, Ordering::Relaxed);

        let mut cpu = CpuSummary {
            threads,
            blocks: 0,
            wrong: 0,
        };
        for worker in workers {
            let work = worker.await?;
            cpu.blocks += work.blocks;
            cpu.wrong += work.wrong;
        }
        let memory = match memory_worker {
            Some(worker) => Some(worker.await?),
            None => match &memory_budget {
                Some(memory::Budget::NotEnoughSpare {
                    available,
                    reserved,
                }) => Some(MemorySummary::NotRun {
                    reason: format!(
                        "Only {} of memory was free, and this test always leaves {} for the \
                         machine to keep running in -- taking more would push it into swap, \
                         which would test the disk rather than the memory and would make the \
                         machine unusable while it ran. Close some programs and try again, or \
                         run the memory test on its own.",
                        format_bytes(*available),
                        format_bytes(*reserved),
                    ),
                }),
                _ => None,
            },
        };

        // Faults that arrived after the supervisor stopped listening. Losing
        // one of these would mean a run that found a hardware fault and did
        // not mention it.
        while let Ok(fault) = fault_rx.try_recv() {
            self.emit(StressEvent::Fault {
                fault: fault.clone(),
            });
            faults.push(fault);
        }

        let report = StressReport {
            started_at,
            asked_for_secs: self.plan.duration.as_secs(),
            ran_for_secs: started.elapsed().as_secs(),
            ending,
            cpu: (threads > 0).then_some(cpu),
            memory,
            heat: thermometer.peaks(),
            watched_heat: thermometer.saw_anything(),
            faults,
        };
        self.emit(StressEvent::Finished {
            report: Box::new(report.clone()),
        });
        Ok(report)
    }
}

/// Stops the workers when it goes out of scope, however it goes out of scope.
struct Deadman(Arc<AtomicBool>);

impl Drop for Deadman {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// One core, working until told to stop.
fn burn_core(
    core: usize,
    deadline: Instant,
    halt: Arc<AtomicBool>,
    counter: Arc<AtomicU64>,
    faults: mpsc::UnboundedSender<Fault>,
) -> cpu::CoreWork {
    let seed = cpu::seed_for(core);
    // Computed here, on this core, before the load starts. See the note at the
    // top of `cpu` for why this is the right reference and a constant is not.
    let expected = cpu::block(seed);
    let mut work = cpu::CoreWork {
        core,
        blocks: 1,
        wrong: 0,
    };

    while !halt.load(Ordering::Relaxed) && Instant::now() < deadline {
        let answer = cpu::block(seed);
        work.blocks += 1;
        counter.fetch_add(1, Ordering::Relaxed);
        if answer != expected {
            work.wrong += 1;
            let _ = faults.send(Fault {
                kind: "cpu".to_string(),
                part: format!("Core {core}"),
                detail: format!(
                    "Core {core} was asked to repeat a calculation it had already done and \
                     returned a different answer: expected {expected:#018x}, got \
                     {answer:#018x}.",
                ),
            });
            // One report per core is enough to condemn it. A core that has
            // started failing can fail thousands of times a second, and
            // filling the report with them would bury everything else.
            break;
        }
    }

    // Keep the core busy for the rest of the run even after it has been
    // reported, so that stopping the test is still the user's decision and the
    // other cores are not suddenly running on a cooler machine.
    while !halt.load(Ordering::Relaxed) && Instant::now() < deadline {
        std::hint::black_box(cpu::block(seed));
        work.blocks += 1;
        counter.fetch_add(1, Ordering::Relaxed);
    }

    work
}

/// Fill as much memory as was budgeted, and check it keeps what it was given.
fn fill_memory(
    bytes: u64,
    deadline: Instant,
    halt: Arc<AtomicBool>,
    counter: Arc<AtomicU64>,
    faults: mpsc::UnboundedSender<Fault>,
) -> MemorySummary {
    let cells_per_chunk = (memory::CHUNK_BYTES / 8) as usize;
    let wanted_chunks = (bytes / memory::CHUNK_BYTES) as usize;
    let mut chunks: Vec<Vec<u64>> = Vec::new();

    for _ in 0..wanted_chunks {
        let mut chunk: Vec<u64> = Vec::new();
        // Asking rather than demanding. A plain allocation that fails aborts
        // the whole process, which on a machine short of memory -- exactly the
        // machine somebody would be testing -- would turn a diagnostic into a
        // crash.
        if chunk.try_reserve_exact(cells_per_chunk).is_err() {
            break;
        }
        chunk.resize(cells_per_chunk, 0);
        chunks.push(chunk);
        if halt.load(Ordering::Relaxed) {
            break;
        }
    }

    let tested = (chunks.len() as u64) * memory::CHUNK_BYTES;
    if chunks.is_empty() {
        return MemorySummary::NotRun {
            reason: "The machine would not give up any memory to test, which usually means \
                     something else took it between the check and the test."
                .to_string(),
        };
    }

    let mut mismatches: Vec<Mismatch> = Vec::new();
    let mut patterns = 0u64;

    'patterns: loop {
        for pattern in Pattern::ALL {
            if halt.load(Ordering::Relaxed) || Instant::now() >= deadline {
                break 'patterns;
            }
            // Written across the whole region first, then read back, so that
            // what is verified has been sitting in memory rather than in the
            // processor's cache. A test that never leaves cache tests the
            // cache.
            for (number, chunk) in chunks.iter_mut().enumerate() {
                memory::write(chunk, pattern, (number * cells_per_chunk) as u64);
                if halt.load(Ordering::Relaxed) {
                    break 'patterns;
                }
            }
            for (number, chunk) in chunks.iter().enumerate() {
                if let Some(mismatch) =
                    memory::verify(chunk, pattern, (number * cells_per_chunk) as u64)
                {
                    let _ = faults.send(Fault {
                        kind: "memory".to_string(),
                        part: format!("offset {}", mismatch.offset_bytes),
                        detail: format!(
                            "Memory did not keep what was written to it: {}.",
                            mismatch.describe()
                        ),
                    });
                    mismatches.push(mismatch);
                    // As with a failing core: one is enough, and a failing
                    // module can produce millions.
                    break 'patterns;
                }
                if halt.load(Ordering::Relaxed) {
                    break 'patterns;
                }
            }
            // Counted here: written across the whole region and read back
            // across the whole of it, which is what one pattern's worth of
            // coverage means.
            patterns += 1;
            counter.store(patterns, Ordering::Relaxed);
        }
    }

    MemorySummary::Ran {
        bytes: tested,
        patterns,
        mismatches,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(faults: Vec<Fault>, ending: Ending, watched_heat: bool) -> StressReport {
        StressReport {
            started_at: OffsetDateTime::now_utc(),
            asked_for_secs: 600,
            ran_for_secs: 600,
            ending,
            cpu: Some(CpuSummary {
                threads: 8,
                blocks: 100_000,
                wrong: 0,
            }),
            memory: Some(MemorySummary::Ran {
                bytes: 8 * 1024 * 1024 * 1024,
                patterns: 20,
                mismatches: Vec::new(),
            }),
            heat: vec![Heat {
                label: "Package".into(),
                peak_c: 72.0,
                critical_c: Some(100.0),
            }],
            watched_heat,
            faults,
        }
    }

    fn cpu_fault() -> Fault {
        Fault {
            kind: "cpu".into(),
            part: "Core 3".into(),
            detail: "Core 3 disagreed with itself.".into(),
        }
    }

    #[test]
    fn a_clean_run_says_what_it_does_not_prove() {
        // The most likely way this feature does harm: somebody runs it for ten
        // minutes, sees "nothing went wrong", and concludes the hardware is
        // fine when the fault they are chasing happens twice a week.
        let findings = report(Vec::new(), Ending::Completed, true).findings();
        let clean = findings
            .iter()
            .find(|finding| finding.id == "stress.clean")
            .expect("a clean run should say so");
        let detail = clean.detail.to_lowercase();
        assert!(
            detail.contains("does not prove") || detail.contains("not cleared"),
            "a clean result reads as a clean bill of health: {}",
            clean.detail
        );
        assert!(
            detail.contains("intermittent"),
            "does not say why a clean run proves little: {}",
            clean.detail
        );
    }

    #[test]
    fn a_short_clean_run_says_it_was_short() {
        let mut brief = report(Vec::new(), Ending::Completed, true);
        brief.ran_for_secs = 20;
        let findings = brief.findings();
        let clean = findings
            .iter()
            .find(|finding| finding.id == "stress.clean")
            .unwrap();
        assert!(clean.detail.contains("short run"), "{}", clean.detail);
    }

    #[test]
    fn a_wrong_answer_from_a_core_is_critical_and_names_the_core() {
        let findings = report(vec![cpu_fault()], Ending::Completed, true).findings();
        let fault = findings
            .iter()
            .find(|finding| finding.id == "stress.cpu-wrong-answer")
            .expect("a wrong answer must be reported");
        assert_eq!(fault.severity, Severity::Critical);
        assert_eq!(fault.subject.as_deref(), Some("Core 3"));
        assert_eq!(fault.category, Category::Cpu);
    }

    #[test]
    fn a_run_that_found_a_fault_never_also_reports_being_clean() {
        // These two in one report would be the tool contradicting itself in
        // the one place it matters most.
        let findings = report(vec![cpu_fault()], Ending::Completed, true).findings();
        assert!(!findings.iter().any(|finding| finding.id == "stress.clean"));
    }

    #[test]
    fn a_cancelled_run_is_not_reported_as_a_pass() {
        // Stopping early is normal and is not a result. Saying "nothing went
        // wrong" about a run somebody cut short after thirty seconds would be
        // reassurance the run did not earn.
        let findings = report(Vec::new(), Ending::Cancelled, true).findings();
        assert!(!findings.iter().any(|finding| finding.id == "stress.clean"));
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn overheating_is_reported_as_a_finding_in_its_own_right() {
        let findings = report(
            Vec::new(),
            Ending::TooHot {
                sensor: "Package".into(),
                reached_c: 97.0,
                ceiling_c: 95.0,
            },
            true,
        )
        .findings();
        let hot = findings
            .iter()
            .find(|finding| finding.id == "stress.overheated")
            .expect("stopping for heat is the most useful thing this can find");
        assert_eq!(hot.severity, Severity::High);
        assert!(hot.title.contains("97"), "{}", hot.title);
        // The causes are physical, and the hint should not send somebody
        // looking through their software for them.
        assert!(
            hot.detail.contains("dust") || hot.detail.contains("fan"),
            "{}",
            hot.detail
        );
    }

    #[test]
    fn a_machine_with_no_sensors_is_told_that_nothing_was_watching() {
        // Otherwise an empty temperature list reads as "it never got hot",
        // which is the opposite of what it means.
        let findings = report(Vec::new(), Ending::Completed, false).findings();
        assert!(
            findings
                .iter()
                .any(|finding| finding.id == "stress.no-temperature-readings"),
            "a blind run looked exactly like a cool one"
        );
    }

    #[test]
    fn a_machine_with_sensors_is_not_told_that() {
        let findings = report(Vec::new(), Ending::Completed, true).findings();
        assert!(
            !findings
                .iter()
                .any(|finding| finding.id == "stress.no-temperature-readings")
        );
    }

    #[test]
    fn memory_filled_but_never_fully_read_back_does_not_read_as_checked() {
        // Found by watching a real one-minute run: a region large enough that
        // no pattern finished reported "0" beside "0 bad", under a heading
        // saying the run finished and nothing went wrong. Three true numbers
        // adding up to a false impression -- that the memory had been checked
        // and was fine.
        let mut unfinished = report(Vec::new(), Ending::Completed, true);
        unfinished.memory = Some(MemorySummary::Ran {
            bytes: 8 * 1024 * 1024 * 1024,
            patterns: 0,
            mismatches: Vec::new(),
        });
        let findings = unfinished.findings();
        let clean = findings
            .iter()
            .find(|finding| finding.id == "stress.clean")
            .expect("the run did finish");
        assert!(
            clean.detail.contains("not fully checked"),
            "a run that checked no pattern claims the memory was clean: {}",
            clean.detail
        );
        assert!(
            !clean.detail.contains("read back 0"),
            "still claiming a clean read of nothing: {}",
            clean.detail
        );
    }

    #[test]
    fn memory_that_was_not_tested_says_so_and_says_why() {
        let mut skipped = report(Vec::new(), Ending::Completed, true);
        skipped.memory = Some(MemorySummary::NotRun {
            reason: "Only 700 MB of memory was free.".into(),
        });
        let findings = skipped.findings();
        let note = findings
            .iter()
            .find(|finding| finding.id == "stress.memory-not-tested")
            .expect("a skipped half of the test must be visible");
        assert!(note.detail.contains("700 MB"));
    }

    #[test]
    fn a_report_survives_the_trip_through_json() {
        // Which is the only trip that matters for the window.
        let original = report(vec![cpu_fault()], Ending::Completed, true);
        let json = serde_json::to_string(&original).unwrap();
        let back: StressReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.faults, original.faults);
        assert_eq!(back.ending, original.ending);
        assert_eq!(back.heat, original.heat);
    }

    #[test]
    fn durations_are_said_the_way_a_person_would_say_them() {
        let mut it = report(Vec::new(), Ending::Completed, true);
        it.ran_for_secs = 45;
        assert_eq!(it.ran_for(), "45 seconds");
        it.ran_for_secs = 600;
        assert_eq!(it.ran_for(), "10 minutes");
        it.ran_for_secs = 7200;
        assert_eq!(it.ran_for(), "2.0 hours");
    }

    #[tokio::test]
    async fn a_short_run_actually_works_the_machine_and_comes_back_clean() {
        // The end-to-end check, kept brief. Memory is left out because a test
        // suite should not allocate most of the machine's memory, and the
        // memory path has its own tests next door.
        let platform = crate::platform::detect().expect("a platform");
        let mut test = StressTest::new(Plan {
            cpu: true,
            memory: false,
            duration: Duration::from_millis(600),
            memory_share: 0.0,
            threads: Some(2),
        });
        let report = test.run(platform).await.expect("the run should finish");

        assert_eq!(report.ending, Ending::Completed);
        assert!(report.clean(), "{:?}", report.faults);
        let cpu = report.cpu.expect("the processor was worked");
        assert_eq!(cpu.threads, 2);
        // More than none, and deliberately not more than some number. This
        // asked for more than two blocks in 600 milliseconds, which is a
        // statement about how fast the machine running the test is -- and it
        // failed the first time the rest of the suite happened to be using
        // the processor at the same time. A CI runner is two shared cores
        // doing several things at once, so that assertion was a flake waiting
        // for somebody else's build. What this test is for is that the run
        // came back Completed having actually counted work rather than
        // returning an empty report; that each block is genuinely verifiable
        // arithmetic is the business of the tests in `cpu.rs`.
        assert!(cpu.blocks > 0, "did no work at all");
        assert_eq!(cpu.wrong, 0);
    }

    #[tokio::test]
    async fn stopping_is_immediate() {
        // The rail that matters most: this is the only thing in the tool that
        // heats somebody's computer, and waiting to stop it is not acceptable.
        let platform = crate::platform::detect().expect("a platform");
        let mut test = StressTest::new(Plan {
            cpu: true,
            memory: false,
            // An hour, so that finishing on its own is not a possible
            // explanation for this test passing.
            duration: Duration::from_secs(3600),
            memory_share: 0.0,
            threads: Some(2),
        });
        let cancel = test.cancel_token();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            cancel.cancel();
        });

        // Same reasoning as the heat test: a cancel that no longer works
        // should fail here, not wedge the suite for an hour.
        let started = Instant::now();
        let report = tokio::time::timeout(Duration::from_secs(30), test.run(platform))
            .await
            .expect("the run did not stop when it was cancelled")
            .expect("the run should finish");
        assert_eq!(report.ending, Ending::Cancelled);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "took {:?} to stop",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_machine_getting_too_hot_stops_the_run() {
        // The one rail here that protects hardware. Exercised with invented
        // readings, because the alternative is to only ever find out whether
        // it works by overheating a real computer.
        let platform = crate::platform::detect().expect("a platform");
        let mut test = StressTest::new(Plan {
            cpu: true,
            memory: false,
            // An hour, so that finishing on its own cannot explain a pass.
            duration: Duration::from_secs(3600),
            memory_share: 0.0,
            threads: Some(1),
        })
        .with_thermometer(thermal::Thermometer::scripted(vec![
            vec![thermal::Reading {
                label: "Package".into(),
                celsius: 55.0,
                critical_c: Some(90.0),
            }],
            vec![thermal::Reading {
                label: "Package".into(),
                celsius: 99.0,
                critical_c: Some(90.0),
            }],
        ]));

        // Through a timeout, so that a rail which has stopped working fails
        // this test in seconds rather than hanging CI for the hour the run was
        // asked for.
        let started = Instant::now();
        let report = tokio::time::timeout(Duration::from_secs(30), test.run(platform))
            .await
            .expect("the run did not stop when the machine overheated")
            .expect("the run should finish");

        let Ending::TooHot {
            sensor,
            reached_c,
            ceiling_c,
        } = &report.ending
        else {
            panic!("did not stop for heat: {:?}", report.ending);
        };
        assert_eq!(sensor, "Package");
        assert_eq!(*reached_c, 99.0);
        assert_eq!(*ceiling_c, 87.0);
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "took {:?} to stop a machine that was overheating",
            started.elapsed()
        );
        // And it is in the findings, not only in the ending.
        assert!(
            report
                .findings()
                .iter()
                .any(|finding| finding.id == "stress.overheated")
        );
    }

    #[tokio::test]
    async fn a_machine_that_stays_cool_is_not_stopped() {
        // The other half: a rail that fires on a healthy machine is worse than
        // no rail, because people turn those off.
        let platform = crate::platform::detect().expect("a platform");
        let mut test = StressTest::new(Plan {
            cpu: true,
            memory: false,
            duration: Duration::from_millis(600),
            memory_share: 0.0,
            threads: Some(1),
        })
        .with_thermometer(thermal::Thermometer::scripted(vec![vec![
            thermal::Reading {
                label: "Package".into(),
                celsius: 71.0,
                critical_c: Some(100.0),
            },
        ]]));

        let report = test.run(platform).await.expect("the run should finish");
        assert_eq!(report.ending, Ending::Completed);
        assert!(report.watched_heat);
        assert_eq!(report.heat.first().map(|heat| heat.peak_c), Some(71.0));
    }

    #[test]
    fn the_memory_worker_writes_and_verifies_real_memory() {
        // Deliberately a small region rather than the share of the machine a
        // real run takes: a test suite should not allocate most of the
        // computer it is running on. It is still the same code path, on real
        // memory, doing real work.
        let halt = Arc::new(AtomicBool::new(false));
        let counter = Arc::new(AtomicU64::new(0));
        let (faults, mut heard) = mpsc::unbounded_channel();
        // Stopped the way the supervisor stops it -- as soon as the counter
        // says a pattern has landed -- rather than by waiting out a deadline.
        // That also checks the counter is updated as the work happens, which
        // is what the progress bar is reading.
        let deadline = Instant::now() + Duration::from_secs(120);
        let watcher = {
            let halt = Arc::clone(&halt);
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || {
                while counter.load(Ordering::Relaxed) < 1 {
                    std::thread::sleep(Duration::from_millis(20));
                }
                halt.store(true, Ordering::Relaxed);
            })
        };

        let summary = fill_memory(
            memory::CHUNK_BYTES,
            deadline,
            Arc::clone(&halt),
            Arc::clone(&counter),
            faults,
        );

        let MemorySummary::Ran {
            bytes,
            patterns,
            mismatches,
        } = summary
        else {
            panic!("should have tested something: {summary:?}");
        };
        assert_eq!(bytes, memory::CHUNK_BYTES);
        watcher
            .join()
            .expect("the watcher should have seen a pattern finish");
        assert!(patterns >= 1, "did not finish a single pattern");
        assert!(mismatches.is_empty(), "{mismatches:?}");
        assert!(heard.try_recv().is_err(), "reported a fault on good memory");
        assert_eq!(counter.load(Ordering::Relaxed), patterns);
    }

    #[test]
    fn the_memory_worker_stops_the_moment_it_is_told_to() {
        let halt = Arc::new(AtomicBool::new(true));
        let (faults, _heard) = mpsc::unbounded_channel();
        let started = Instant::now();
        let summary = fill_memory(
            memory::CHUNK_BYTES,
            Instant::now() + Duration::from_secs(3600),
            halt,
            Arc::new(AtomicU64::new(0)),
            faults,
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
        assert!(
            matches!(summary, MemorySummary::Ran { patterns: 0, .. }),
            "{summary:?}"
        );
    }

    #[test]
    fn dropping_the_run_stops_the_machine_burning() {
        // Found by breaking the heat rail on purpose: the suite stopped
        // failing on an assertion and started hanging outright, because a
        // dropped run left its workers going. In the window that is a closed
        // tab leaving a laptop at full load with nothing watching how hot it
        // gets.
        //
        // Built by hand with exactly as many blocking threads as the run will
        // use, so that "are the workers still going?" can be asked directly:
        // with the pool full, nothing else can start. The default pool has
        // hundreds of threads, and against that this test would pass whether
        // the workers stopped or not.
        let threads = 2;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(threads)
            .enable_all()
            .build()
            .expect("a runtime");

        let freed = runtime.block_on(async {
            let platform = crate::platform::detect().expect("a platform");
            let mut test = StressTest::new(Plan {
                cpu: true,
                memory: false,
                duration: Duration::from_secs(3600),
                memory_share: 0.0,
                threads: Some(threads),
            });

            // Dropped, never cancelled -- the point is that giving up on the
            // run is enough on its own.
            tokio::time::timeout(Duration::from_millis(400), test.run(platform))
                .await
                .expect_err("the run should still have been going");

            tokio::time::timeout(Duration::from_secs(20), tokio::task::spawn_blocking(|| {}))
                .await
                .is_ok()
        });

        // Torn down without waiting. Dropping the runtime the ordinary way
        // waits for blocking tasks, so a version of this that had regressed
        // would hang here for the hour the run asked for instead of saying
        // what was wrong.
        runtime.shutdown_background();
        assert!(
            freed,
            "the workers were still burning after the run was dropped"
        );
    }

    #[tokio::test]
    async fn a_plan_with_nothing_in_it_does_nothing_rather_than_failing() {
        let platform = crate::platform::detect().expect("a platform");
        let mut test = StressTest::new(Plan {
            cpu: false,
            memory: false,
            duration: Duration::from_millis(100),
            memory_share: 0.0,
            threads: None,
        });
        let report = test.run(platform).await.expect("should not fail");
        assert!(report.cpu.is_none());
        assert!(report.memory.is_none());
    }
}
