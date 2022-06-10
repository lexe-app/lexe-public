use chrono::Utc;
use lightning::util::logger::{Logger, Record};

pub struct StdOutLogger {}
impl Logger for StdOutLogger {
    /// Just print to stdout and let the runner capture the log output.
    fn log(&self, record: &Record) {
        let raw_log = record.args.to_string();
        println!(
            "{} {:<5} [{}:{}] {}\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            record.level,
            record.module_path,
            record.line,
            raw_log
        );
    }
}
