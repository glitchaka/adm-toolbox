use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Result, anyhow};
use ferrisetw::{
    EventRecord, SchemaLocator,
    parser::Parser,
    provider::Provider,
    trace::{TraceTrait, UserTrace, stop_trace_by_name},
};

use crate::core::{
    models::network::ByteCounters,
    ports::{TrafficMonitor, TrafficMonitorFactory},
};

const NETWORK_PROVIDER: &str = "7dd42a49-5329-4832-8dfd-43d979153a88";
const TRACE_NAME: &str = "SST-Traffic";

pub struct EtwTrafficMonitorFactory;

impl TrafficMonitorFactory for EtwTrafficMonitorFactory {
    fn start(&self) -> Result<Box<dyn TrafficMonitor>> {
        Ok(Box::new(EtwTrafficMonitor::start()?))
    }

    fn rates(
        &self,
        before: &HashMap<u32, ByteCounters>,
        after: &HashMap<u32, ByteCounters>,
        elapsed: Duration,
    ) -> HashMap<u32, ByteCounters> {
        rates(before, after, elapsed)
    }
}

struct EtwTrafficMonitor {
    counters: Arc<Mutex<HashMap<u32, ByteCounters>>>,
    trace: Option<UserTrace>,
    processor: Option<JoinHandle<()>>,
}

impl EtwTrafficMonitor {
    fn start() -> Result<Self> {
        let counters = Arc::new(Mutex::new(HashMap::new()));
        let callback_counters = Arc::clone(&counters);

        let provider = Provider::by_guid(NETWORK_PROVIDER)
            .add_callback(move |record, schema_locator| {
                process_network_event(record, schema_locator, &callback_counters);
            })
            .build();

        let _ = stop_trace_by_name(TRACE_NAME);

        let (trace, trace_handle) = UserTrace::new()
            .named(TRACE_NAME.to_owned())
            .enable(provider)
            .start()
            .map_err(|error| anyhow!("no se pudo iniciar ETW: {error:?}"))?;

        let processor = thread::Builder::new()
            .name("sst-etw-network".to_owned())
            .spawn(move || {
                let _ = UserTrace::process_from_handle(trace_handle);
            })
            .map_err(|error| anyhow!("no se pudo iniciar el consumidor ETW: {error}"))?;

        Ok(Self {
            counters,
            trace: Some(trace),
            processor: Some(processor),
        })
    }
}

impl TrafficMonitor for EtwTrafficMonitor {
    fn snapshot(&self) -> HashMap<u32, ByteCounters> {
        self.counters
            .lock()
            .map(|counters| counters.clone())
            .unwrap_or_default()
    }
}

impl Drop for EtwTrafficMonitor {
    fn drop(&mut self) {
        drop(self.trace.take());

        if let Some(processor) = self.processor.take() {
            let _ = processor.join();
        }
    }
}

fn process_network_event(
    record: &EventRecord,
    schema_locator: &SchemaLocator,
    counters: &Arc<Mutex<HashMap<u32, ByteCounters>>>,
) {
    let direction = match record.event_id() {
        10 | 12 | 13 | 14 | 16 | 26 | 28 | 29 | 30 | 32 | 42 | 58 => Direction::Sent,
        11 | 15 | 27 | 31 | 43 | 59 => Direction::Received,
        _ => return,
    };

    let Ok(schema) = schema_locator.event_schema(record) else {
        return;
    };

    let parser = Parser::create(record, &schema);
    let pid = parser
        .try_parse::<u32>("PID")
        .or_else(|_| parser.try_parse::<u32>("pid"));
    let size = parser
        .try_parse::<u32>("size")
        .or_else(|_| parser.try_parse::<u32>("Size"));

    let (Ok(pid), Ok(size)) = (pid, size) else {
        return;
    };

    if pid == 0 || size == 0 {
        return;
    }

    let Ok(mut counters) = counters.lock() else {
        return;
    };

    let entry = counters.entry(pid).or_default();

    match direction {
        Direction::Sent => entry.sent = entry.sent.saturating_add(u64::from(size)),
        Direction::Received => entry.received = entry.received.saturating_add(u64::from(size)),
    }
}

enum Direction {
    Sent,
    Received,
}

fn rates(
    before: &HashMap<u32, ByteCounters>,
    after: &HashMap<u32, ByteCounters>,
    elapsed: Duration,
) -> HashMap<u32, ByteCounters> {
    let seconds = elapsed.as_secs_f64().max(0.001);
    let mut result = HashMap::new();

    for (&pid, current) in after {
        let previous = before.get(&pid).copied().unwrap_or_default();

        result.insert(
            pid,
            ByteCounters {
                sent: ((current.sent.saturating_sub(previous.sent)) as f64 / seconds) as u64,
                received: ((current.received.saturating_sub(previous.received)) as f64 / seconds) as u64,
            },
        );
    }

    result
}
