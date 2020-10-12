use flexi_logger::writers::{LogWriter};
use flexi_logger::{DeferredNow, style};
use log::{LevelFilter, Record};
use std::io::Write;


pub struct StdErrLog;

impl StdErrLog {
    pub fn new()-> StdErrLog {
        StdErrLog
    }
}

impl LogWriter for StdErrLog {
    fn write(&self, now: &mut DeferredNow, record: &Record) -> std::io::Result<()> {
        let level = record.level();
        write!(
            std::io::stderr(),
            "[{}] {} [{}:{}] {}\r\n",
            now.now().format("%Y-%m-%d %H:%M:%S%.6f %:z"),
            style(level, level),
            record.file().unwrap_or("<unnamed>"),
            record.line().unwrap_or(0),
            record.args()
        )
    }

    fn flush(&self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }

    fn max_log_level(&self) -> LevelFilter {
        log::LevelFilter::Error
    }
}

