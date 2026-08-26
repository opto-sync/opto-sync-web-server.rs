#![forbid(unsafe_code)]

use opto_sync_web_server::{config::WebConfig, server};

fn main() {
    let cfg = WebConfig::from_env();
    server::run(&cfg);
}

