use anyhow::{Context, Result};
use pnet::packet::{ip::IpNextHeaderProtocols, Packet};
use pnet::transport::{self, TransportChannelType, TransportProtocol, TransportSender};
use pnet::util;
use std::collections::VecDeque;
use std::fmt::{self, Display};
use std::net::{IpAddr, Ipv4Addr};
use std::time::SystemTime;

fn main() {
    println!("Hello, world!");
    // 送信
}

pub struct Socket {
    pub local_addr: Ipv4Addr,
    pub remote_addr: Ipv4Addr,
    pub local_port: u16,
    pub remote_port: u16,
    pub send_param: SendParam,
    pub recv_param: RecvParam,
    pub status: TcpStatus,
    pub sender: TransportSender,
}

#[derive(Clone, Debug)]
pub struct SendParam {
    pub unacked_seq: u32, // 送信後まだackされていないseqの先頭
    pub next: u32,        // 次の送信
    pub initial_seq: u32, // 初期送信seq
}

#[derive(Clone, Debug)]
pub struct RecvParam {
    pub next: u32,        // 次受信するseq
    pub initial_seq: u32, // 初期受信seq
    pub tail: u32,        // 受信seqの最後尾
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TcpStatus {
    Listen,
    SynSent,
    SynRcvd,
    Established,
    FinWait1,
    FinWait2,
    TimeWait,
    CloseWait,
    LastAck,
}

impl Socket {
    pub fn new(
        local_addr: Ipv4Addr,
        remote_addr: Ipv4Addr,
        local_port: u16,
        remote_port: u16,
        status: TcpStatus,
    ) -> Result<Self> {
        let (sender, _) = transport::transport_channel(
            65535,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?;
        Ok(Self {
            local_addr,
            remote_addr,
            local_port,
            remote_port,
            send_param: SendParam {
                unacked_seq: 0,
                initial_seq: 0,
                next: 0,
            },
            recv_param: RecvParam {
                initial_seq: 0,
                next: 0,
                tail: 0,
            },
            status,
            sender,
        })
    }

    pub fn send_tcp_packet(
        &mut self,
        seq: u32,
        ack: u32,
        flag: u8,
        payload: &[u8],
    ) -> Result<usize> {
        let mut tcp_packet = TCPPacket::new(payload.len());
        tcp_packet.set_src(self.local_port);
        tcp_packet.set_dest(self.remote_port);
        tcp_packet.set_seq(seq);
        tcp_packet.set_ack(ack);
        tcp_packet.set_data_offset(5);
        tcp_packet.set_flag(flag);
        tcp_packet.set_payload(payload);
        tcp_packet.set_checksum(util::ipv4_checksum(
            &tcp_packet.packet(),
            8,
            &[],
            &self.local_addr,
            &self.remote_addr,
            IpNextHeaderProtocols::Tcp,
        ));
        let sent_size = self
            .sender
            .send_to(tcp_packet.clone(), IpAddr::V4(self.remote_addr))
            .context(format!("failed to send: \n{:?}", tcp_packet))?;

        dbg!("sent", &tcp_packet);
        if payload.is_empty() && tcp_packet.get_flag() == tcpflags::ACK {
            return Ok(sent_size);
        }
        Ok(sent_size)
    }

    pub fn get_sock_id(&self) -> SockID {
        SockID(
            self.local_addr,
            self.remote_addr,
            self.local_port,
            self.remote_port,
        )
    }
}
