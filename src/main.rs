use anyhow::Result;
use pingora::{prelude::Server, proxy::http_proxy_service};
use simple_proxy::DualWriteProxy;
use std::collections::HashSet;
use std::sync::Mutex;
use tracing::info;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let mut my_server = Server::new(None).unwrap();
    my_server.bootstrap();
    let proxy_addr = "0.0.0.0:8080";
    let mut lb = http_proxy_service(
        &my_server.configuration,
        DualWriteProxy {
            executed_requests: Mutex::new(HashSet::new()),
        },
    );
    lb.add_tcp(proxy_addr);
    info!("DualWriteProxy listening on {}", proxy_addr);
    my_server.add_service(lb);
    my_server.run_forever();
}
