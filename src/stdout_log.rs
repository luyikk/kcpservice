use anyhow::Context;
use flexi_logger::writers::LogWriter;
use flexi_logger::{style, DeferredNow, TS_DASHES_BLANK_COLONS_DOT_BLANK};
use log::{LevelFilter, Record};
use std::io::Write;
use std::path::Path;

pub struct StdErrLog;

impl StdErrLog {
    pub fn new() -> StdErrLog {
        StdErrLog
    }
}
fn get_file_name(path: Option<&str>) -> anyhow::Result<&str> {
    match path {
        Some(v) => Ok(Path::new(v)
            .file_name()
            .context("<unnamed>")?
            .to_str()
            .context("<unnamed>")?),
        None => Ok("<unnamed>"),
    }
}

impl LogWriter for StdErrLog {
    #[inline]
    fn write(&self, now: &mut DeferredNow, record: &Record) -> std::io::Result<()> {
        let level = record.level();
        write!(
            std::io::stderr(),
            "[{} {} {}:{}] {}\r\n",
            now.format(TS_DASHES_BLANK_COLONS_DOT_BLANK),
            style(level).paint(level.to_string()),
            get_file_name(record.file()).unwrap_or("<unnamed>"),
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
