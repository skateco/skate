use std::error::Error;
use std::{panic, process, thread};
use log::{error, info, LevelFilter};
use syslog::{BasicLogger, Facility, Formatter3164};
#[cfg(target_os = "linux")]
use skate::netavark;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let formatter = Formatter3164 {
        facility: Facility::LOG_USER,
        hostname: None,
        process: "skatelet-netavark".to_string(),
        pid: process::id(),
    };
    let logger = match syslog::unix(formatter) {
        Err(e) => return Err(e.into()),
        Ok(logger) => logger,
    };

    log::set_boxed_logger(Box::new(BasicLogger::new(logger)))
        .map(|()| log::set_max_level(LevelFilter::Debug))?;

    panic::set_hook(Box::new(move |info| {

        let thread = thread::current();
        let thread = thread.name().unwrap_or("<unnamed>");

        let msg = match info.payload().downcast_ref::<&'static str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => &**s,
                None => "Box<Any>",
            },
        };

        match info.location() {
            Some(location) => {
                error!(
                        target: "panic", "thread '{}' panicked at '{}': {}:{}",
                        thread,
                        msg,
                        location.file(),
                        location.line(),
                    );
            }
            None => error!(
                    target: "panic",
                    "thread '{}' panicked at '{}'",
                    thread,
                    msg,
                ),
        }
    }));
    info!("starting skatelet-netavark");
    #[cfg(target_os = "linux")]
    netavark();
    Ok(())
}
