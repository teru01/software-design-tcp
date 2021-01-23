use anyhow::Result;
use std::{env, io, net::Ipv4Addr, str};
use tcp::TCP;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let addr: Ipv4Addr = args[1].parse()?;
    let port: u16 = args[2].parse()?;
    client(addr, port)?;
    Ok(())
}

fn client(remote_addr: Ipv4Addr, remote_port: u16) -> Result<()> {
    let tcp = TCP::new();
    let sock_id = tcp.connect(remote_addr, remote_port)?;
    ctrlc::set_handler(move || {
        std::process::exit(0);
    })?;
    loop {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        tcp.send(sock_id, input.as_bytes())?;
    }
}
