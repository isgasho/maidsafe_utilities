// Copyright 2015 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under (1) the MaidSafe.net Commercial License,
// version 1.0 or later, or (2) The General Public License (GPL), version 3, depending on which
// licence you accepted on initial access to the Software (the "Licences").
//
// By contributing code to the SAFE Network Software, or to this project generally, you agree to be
// bound by the terms of the MaidSafe Contributor Agreement, version 1.0.  This, along with the
// Licenses can be found in the root directory of this project at LICENSE, COPYING and CONTRIBUTOR.
//
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.
//
// Please review the Licences for the specific language governing permissions and limitations
// relating to use of the SAFE Network Software.

//! These functions can initialise logging for output to stdout only, or to a file and
//! stdout. For more fine-grained control, create file called `log.toml` in the root
//! directory of the project, or in the same directory where the executable is.
//! See http://sfackler.github.io/log4rs/doc/v0.3.3/log4rs/index.html for details
//! about format and structure of this file.
//!
//! An example of a log message is:
//!
//! ```
//! # fn main() { /*
//! W 19:33:49.245434 <main> [example:src/main.rs:50] Warning level message.
//! ^        ^          ^        ^          ^                    ^
//! |    timestamp      | top-level module  |                 message
//! |                   |                   |
//! |              thread name       file and line no.
//! |
//! level (E, W, I, D, or T for error, warn, info, debug or trace respectively)
//! # */}
//! ```
//!
//! Logging of the thread name is enabled or disabled via the `show_thread_name` parameter.  If
//! enabled, and the thread executing the log statement is unnamed, the thread name is shown as
//! `???`.
//!
//! The functions can safely be called multiple times concurrently.
//!
//! #Examples
//!
//! ```
//! #[macro_use]
//! extern crate log;
//! #[macro_use]
//! extern crate maidsafe_utilities;
//! use std::thread;
//! use maidsafe_utilities::thread::RaiiThreadJoiner;
//!
//! fn main() {
//!     maidsafe_utilities::log::init(true);
//!
//!     warn!("A warning");
//!
//!     let unnamed = thread::spawn(move || info!("Message in unnamed thread"));
//!     let _ = unnamed.join();
//!
//!     let _named = RaiiThreadJoiner::new(thread!("Worker",
//!                      move || error!("Message in named thread")));
//!
//!     // W 12:24:07.064746 <main> [example:src/main.rs:11] A warning
//!     // I 12:24:07.065746 ??? [example:src/main.rs:13] Message in unnamed thread
//!     // E 12:24:07.065746 Worker [example:src/main.rs:16] Message in named thread
//! }
//! ```
//!
//! Environment variable `RUST_LOG` can be set and fine-tuned to get various modules logging to
//! different levels. E.g. `RUST_LOG=mod0,mod1=debug,mod2,mod3` will have `mod0` & `mod1` logging at
//! `Debug` and more severe levels while `mod2` & `mod3` logging at default (currently `Warn`) and
//! more severe levels. `RUST_LOG=trace,mod0=error,mod1` is going to change the default log level to
//! `Trace` and more severe. Thus `mod0` will log at `Error` level and `mod1` at `Trace` and more
//! severe ones.

use log4rs;
use log4rs::appender::{ConsoleAppender, FileAppender};
use log4rs::config::{Appender, Config, Logger, Root};
use log4rs::pattern::PatternLayout;
use log4rs::toml::Creator;

use std::fmt::{self, Display, Formatter};
use std::path::Path;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Once, ONCE_INIT};

use async_log::{AsyncConsoleAppender, AsyncConsoleAppenderCreator, AsyncFileAppender, AsyncFileAppenderCreator,
                AsyncServerAppenderCreator, AsyncAppender};
use logger::LogLevelFilter;

static INITIALISE_LOGGER: Once = ONCE_INIT;
static CONFIG_FILE: &'static str = "log.toml";
static DEFAULT_LOG_LEVEL_FILTER: LogLevelFilter = LogLevelFilter::Warn;

/// Initialises the env_logger for output to stdout.
///
/// For further details, see the [module docs](index.html).
pub fn init(show_thread_name: bool) -> Result<(), String> {
    let mut result = Err("Logger already initialised".to_owned());

    INITIALISE_LOGGER.call_once(|| {
        let config_path = Path::new(CONFIG_FILE);

        result = if config_path.is_file() {
            let mut creator = Creator::default();
            creator.add_appender("async_console", Box::new(AsyncConsoleAppenderCreator));
            creator.add_appender("async_file", Box::new(AsyncFileAppenderCreator));
            creator.add_appender("async_server", Box::new(AsyncServerAppenderCreator));

            log4rs::init_file(config_path, creator).map_err(|e| format!("{}", e))
        } else {
            let pattern = make_pattern(show_thread_name);

            let appender = ConsoleAppender::builder().pattern(pattern).build();
            let appender = Appender::builder("console".to_owned(), Box::new(appender)).build();

            let (default_level, loggers) = parse_loggers_from_env().expect("failed to parse RUST_LOG env variable");

            let root = Root::builder(default_level).appender("console".to_owned()).build();
            let config = match Config::builder(root)
                                   .appender(appender)
                                   .loggers(loggers)
                                   .build()
                                   .map_err(|e| format!("{}", e)) {
                Ok(config) => config,
                Err(e) => {
                    result = Err(e);
                    return;
                }
            };

            log4rs::init_config(config).map_err(|e| format!("{}", e))
        };
    });

    result
}

/// Initialises the env_logger for output to a file and to stdout.
///
/// This function will create the logfile at `file_path` if it does not exist, and will truncate it
/// if it does.  For further details, see the [module docs](index.html).
///
/// #Examples
///
/// ```
/// #[macro_use]
/// extern crate log;
/// extern crate maidsafe_utilities;
///
/// fn main() {
///     assert!(maidsafe_utilities::log::init_to_file(true, "target/test.log").is_ok());
///     error!("An error!");
///     assert_eq!(maidsafe_utilities::log::init_to_file(true, "target/test.log").unwrap_err(),
///         "Logger already initialised".to_owned());
///
///     // E 22:38:05.499016 <main> [example:main.rs:7] An error!
/// }
/// ```
pub fn init_to_file<P: AsRef<Path>>(show_thread_name: bool, file_path: P) -> Result<(), String> {
    let mut result = Err("Logger already initialised".to_owned());

    INITIALISE_LOGGER.call_once(|| {
        let file_appender = FileAppender::builder(file_path)
                                .pattern(make_pattern(show_thread_name))
                                .append(false)
                                .build();
        let file_appender = match file_appender {
            Ok(appender) => appender,
            Err(error) => {
                result = Err(format!("{}", error));
                return;
            }
        };
        let file_appender = Appender::builder("file".to_owned(), Box::new(file_appender)).build();

        let console_appender = ConsoleAppender::builder()
                                   .pattern(make_pattern(show_thread_name))
                                   .build();
        let console_appender = Appender::builder("console".to_owned(), Box::new(console_appender)).build();

        let (default_level, loggers) = match parse_loggers_from_env() {
            Ok((level, loggers)) => (level, loggers),
            Err(error) => {
                result = Err(format!("{}", error));
                return;
            }
        };

        let root = Root::builder(default_level)
                       .appender("console".to_owned())
                       .appender("file".to_owned())
                       .build();

        let config = match Config::builder(root)
                               .appender(console_appender)
                               .appender(file_appender)
                               .loggers(loggers)
                               .build()
                               .map_err(|e| format!("{}", e)) {
            Ok(config) => config,
            Err(e) => {
                result = Err(e);
                return;
            }
        };

        result = log4rs::init_config(config).map_err(|e| format!("{}", e))
    });

    result
}

/// Initialises the env_logger for output to a file and optionally to the
/// console asynchronously.
pub fn init_to_file_async<P: AsRef<Path>>(show_thread_name: bool,
                                          file_path: P,
                                          log_to_console: bool)
                                          -> Result<(), String> {
    let mut result = Err("Logger already initialised".to_owned());

    INITIALISE_LOGGER.call_once(|| {
        let (default_level, loggers) = match parse_loggers_from_env() {
            Ok((level, loggers)) => (level, loggers),
            Err(error) => {
                result = Err(format!("{}", error));
                return;
            }
        };

        let mut root = Root::builder(default_level).appender("file".to_owned());

        if log_to_console {
            root = root.appender("console".to_owned());
        }

        let root = root.build();

        let mut config = Config::builder(root).loggers(loggers);

        let file_appender = AsyncFileAppender::builder(file_path)
                                .pattern(make_pattern(show_thread_name))
                                .append(false)
                                .build();
        let file_appender = match file_appender {
            Ok(appender) => appender,
            Err(error) => {
                result = Err(format!("{}", error));
                return;
            }
        };
        let file_appender = Appender::builder("file".to_owned(), Box::new(file_appender)).build();

        config = config.appender(file_appender);

        if log_to_console {
            let console_appender = AsyncConsoleAppender::builder()
                                       .pattern(make_pattern(show_thread_name))
                                       .build();
            let console_appender = Appender::builder("console".to_owned(), Box::new(console_appender)).build();

            config = config.appender(console_appender);
        }

        let config = match config.build().map_err(|e| format!("{}", e)) {
            Ok(config) => config,
            Err(e) => {
                result = Err(e);
                return;
            }
        };
        result = log4rs::init_config(config).map_err(|e| format!("{}", e))
    });

    result
}

/// Initialises the env_logger for output to a server and optionally to the
/// console asynchronously.
pub fn init_to_server_async<A: ToSocketAddrs>(server_addr: A,
                                              show_thread_name: bool,
                                              log_to_console: bool)
                                              -> Result<(), String> {
    let mut result = Err("Logger already initialised".to_owned());

    INITIALISE_LOGGER.call_once(|| {
        use net2::TcpStreamExt;

        let (default_level, loggers) = match parse_loggers_from_env() {
            Ok((level, loggers)) => (level, loggers),
            Err(error) => {
                result = Err(format!("{}", error));
                return;
            }
        };

        let mut root = Root::builder(default_level).appender("server".to_owned());

        if log_to_console {
            root = root.appender("console".to_owned());
        }

        let root = root.build();

        let mut config = Config::builder(root).loggers(loggers);

        let pattern = make_pattern(show_thread_name);

        let stream = match TcpStream::connect(server_addr).map_err(|e| format!("{}", e)) {
            Ok(stream) => {
                match stream.set_nodelay(true) {
                    Ok(()) => stream,
                    Err(e) => {
                        result = Err(format!{"{}", e});
                        return;
                    }
                }
            }
            Err(e) => {
                result = Err(e);
                return;
            }
        };
        let server_appender = Appender::builder("server".to_owned(),
                                                Box::new(AsyncAppender::new(stream, pattern)))
                                  .build();

        config = config.appender(server_appender);

        if log_to_console {
            let console_appender = AsyncConsoleAppender::builder()
                                       .pattern(make_pattern(show_thread_name))
                                       .build();
            let console_appender = Appender::builder("console".to_owned(), Box::new(console_appender)).build();

            config = config.appender(console_appender);
        }

        let config = match config.build().map_err(|e| format!("{}", e)) {
            Ok(config) => config,
            Err(e) => {
                result = Err(e);
                return;
            }
        };
        result = log4rs::init_config(config).map_err(|e| format!("{}", e))
    });

    result
}

fn make_pattern(show_thread_name: bool) -> PatternLayout {
    let pattern = if show_thread_name {
        "%l %d %T [%M ##%f##:%L] %m"
    } else {
        "%l %d [%M ##%f##:%L] %m"
    };

    unwrap_result!(PatternLayout::new(pattern))
}

#[derive(Debug)]
struct ParseLoggerError;

impl Display for ParseLoggerError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "ParseLoggerError")
    }
}

impl From<()> for ParseLoggerError {
    fn from(_: ()) -> Self {
        ParseLoggerError
    }
}

fn parse_loggers_from_env() -> Result<(LogLevelFilter, Vec<Logger>), ParseLoggerError> {
    use std::env;

    if let Ok(var) = env::var("RUST_LOG") {
        parse_loggers(&var)
    } else {
        Ok((DEFAULT_LOG_LEVEL_FILTER, Vec::new()))
    }
}

fn parse_loggers(input: &str) -> Result<(LogLevelFilter, Vec<Logger>), ParseLoggerError> {
    use std::collections::VecDeque;

    let mut loggers = Vec::new();
    let mut grouped_modules = VecDeque::new();
    let mut default_level = DEFAULT_LOG_LEVEL_FILTER;

    for sub_input in input.split(',')
                          .map(str::trim)
                          .filter(|d| !d.is_empty()) {
        let mut parts = sub_input.trim().split('=');
        match (parts.next(), parts.next()) {
            (Some(module_name), Some(level)) => {
                let level_filter = try!(level.parse());
                while let Some(module) = grouped_modules.pop_front() {
                    loggers.push(Logger::builder(module, level_filter).build());
                }
                loggers.push(Logger::builder(module_name.to_owned(), level_filter).build());
            }
            (Some(module), None) => {
                if let Ok(level_filter) = module.parse::<LogLevelFilter>() {
                    default_level = level_filter;
                } else {
                    grouped_modules.push_back(module.to_owned());
                }
            }
            _ => return Err(ParseLoggerError),
        }
    }

    while let Some(module) = grouped_modules.pop_front() {
        loggers.push(Logger::builder(module, default_level).build());
    }


    Ok((default_level, loggers))
}

#[cfg(test)]
mod test {
    use super::*;
    use super::parse_loggers;

    use std::str;
    use std::thread;
    use std::sync::mpsc;
    use std::time::Duration;
    use std::net::TcpListener;
    use logger::LogLevelFilter;
    use thread::RaiiThreadJoiner;
    use async_log::MSG_TERMINATOR;

    #[test]
    fn test_parse_loggers() {
        let (level, loggers) = parse_loggers("").unwrap();
        assert_eq!(level, LogLevelFilter::Warn);
        assert!(loggers.is_empty());

        let (level, loggers) = parse_loggers("foo").unwrap();
        assert_eq!(level, LogLevelFilter::Warn);
        assert_eq!(loggers.len(), 1);
        assert_eq!(loggers[0].name(), "foo");
        assert_eq!(loggers[0].level(), LogLevelFilter::Warn);

        let (level, loggers) = parse_loggers("info").unwrap();
        assert_eq!(level, LogLevelFilter::Info);
        assert!(loggers.is_empty());

        let (level, loggers) = parse_loggers("foo::bar=warn").unwrap();
        assert_eq!(level, LogLevelFilter::Warn);
        assert_eq!(loggers.len(), 1);
        assert_eq!(loggers[0].name(), "foo::bar");
        assert_eq!(loggers[0].level(), LogLevelFilter::Warn);

        let (level, loggers) = parse_loggers("foo::bar=error,baz=debug,qux").unwrap();
        assert_eq!(level, LogLevelFilter::Warn);
        assert_eq!(loggers.len(), 3);

        assert_eq!(loggers[0].name(), "foo::bar");
        assert_eq!(loggers[0].level(), LogLevelFilter::Error);

        assert_eq!(loggers[1].name(), "baz");
        assert_eq!(loggers[1].level(), LogLevelFilter::Debug);

        assert_eq!(loggers[2].name(), "qux");
        assert_eq!(loggers[2].level(), LogLevelFilter::Warn);

        let (level, loggers) = parse_loggers("info,foo::bar,baz=debug,a0,a1, a2 , a3").unwrap();
        assert_eq!(level, LogLevelFilter::Info);
        assert_eq!(loggers.len(), 6);

        assert_eq!(loggers[0].name(), "foo::bar");
        assert_eq!(loggers[0].level(), LogLevelFilter::Debug);

        assert_eq!(loggers[1].name(), "baz");
        assert_eq!(loggers[1].level(), LogLevelFilter::Debug);

        assert_eq!(loggers[2].name(), "a0");
        assert_eq!(loggers[2].level(), LogLevelFilter::Info);

        assert_eq!(loggers[3].name(), "a1");
        assert_eq!(loggers[3].level(), LogLevelFilter::Info);

        assert_eq!(loggers[4].name(), "a2");
        assert_eq!(loggers[4].level(), LogLevelFilter::Info);

        assert_eq!(loggers[5].name(), "a3");
        assert_eq!(loggers[5].level(), LogLevelFilter::Info);
    }

    #[test]
    fn server_logging() {
        const MSG_COUNT: usize = 3;

        let (tx, rx) = mpsc::channel();

        // Start Log Message Server
        let _raii_joiner = RaiiThreadJoiner::new(thread!("LogMessageServer", move || {
            use std::io::Read;

            let listener = unwrap_result!(TcpListener::bind("127.0.0.1:55555"));
            unwrap_result!(tx.send(()));
            let (mut stream, _) = unwrap_result!(listener.accept());

            let mut log_msgs = Vec::with_capacity(MSG_COUNT);

            let mut read_buf = Vec::with_capacity(1024);
            let mut scratch_buf = [0u8; 1024];
            let mut search_frm_index = 0;

            while log_msgs.len() < MSG_COUNT {
                let bytes_rxd = unwrap_result!(stream.read(&mut scratch_buf));
                if bytes_rxd == 0 {
                    unreachable!("Should not have encountered shutdown yet");
                }

                read_buf.extend_from_slice(&scratch_buf[..bytes_rxd]);

                while read_buf.len() - search_frm_index >= MSG_TERMINATOR.len() {
                    let mut found = true;
                    for i in search_frm_index..search_frm_index + MSG_TERMINATOR.len() {
                        if read_buf[i] != MSG_TERMINATOR[i - search_frm_index] {
                            search_frm_index += 1;
                            found = false;
                            break;
                        }
                    }

                    if found {
                        log_msgs.push(unwrap_result!(str::from_utf8(&read_buf[..search_frm_index])).to_owned());
                        read_buf = read_buf.split_off(search_frm_index + MSG_TERMINATOR.len());
                        search_frm_index = 0;
                    }
                }
            }

            for it in log_msgs.iter().enumerate() {
                assert!(it.1.contains(&format!("This is message {}", it.0)[..]));
                assert!(!it.1.contains("#"));
            }
        }));

        unwrap_result!(rx.recv());

        unwrap_result!(init_to_server_async("127.0.0.1:55555", true, false));

        info!("This message should not be found by default log level");
        warn!("This is message 0");
        trace!("This message should not be found by default log level");
        warn!("This is message 1");

        // Some interval before the 3rd message to test if server logic above works fine with
        // separate arrival of messages. Without sleep it will usually receive all 3 messages in a
        // single read cycle
        thread::sleep(Duration::from_millis(500));

        debug!("This message should not be found by default log level");
        error!("This is message 2");
    }
}
