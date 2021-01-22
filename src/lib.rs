use anyhow::{Context, Result};
use pnet::packet::{
    ip::IpNextHeaderProtocols,
    tcp::{MutableTcpPacket, TcpFlags},
    Packet,
};
use pnet::transport::{self, TransportChannelType, TransportProtocol, TransportSender};
use pnet::util;
use rand::{rngs::ThreadRng, Rng};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::fmt::{self, Display};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Condvar, Mutex, RwLock, RwLockWriteGuard};
use std::time::SystemTime;
use std::{cmp, ops::Range, str, thread};

const TCP_HEADER_SIZE: usize = 20;

#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub struct SocketID(pub Ipv4Addr, pub Ipv4Addr, pub u16, pub u16);

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
        flag: u16,
        payload: &[u8],
    ) -> Result<usize> {
        let mut buffer = vec![0; TCP_HEADER_SIZE + payload.len()];
        let mut tcp_packet = MutableTcpPacket::new(&mut buffer).unwrap();
        tcp_packet.set_source(self.local_port);
        tcp_packet.set_destination(self.remote_port);
        tcp_packet.set_sequence(seq);
        tcp_packet.set_acknowledgement(ack);
        tcp_packet.set_data_offset(5);
        tcp_packet.set_flags(flag.into());
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
            .send_to(tcp_packet, IpAddr::V4(self.remote_addr))
            .context(format!("failed to send: {:?}", self.get_sock_id()))?;

        // dbg!("sent", &tcp_packet);
        // if payload.is_empty() && tcp_packet.get_flag() == TcpFlags::ACK {
        //     return Ok(sent_size);
        // }
        Ok(sent_size)
    }

    pub fn get_sock_id(&self) -> SocketID {
        SocketID(
            self.local_addr,
            self.remote_addr,
            self.local_port,
            self.remote_port,
        )
    }
}

pub struct TCP {
    sockets: RwLock<HashMap<SocketID, Socket>>,
}

impl TCP {
    pub fn new() -> Arc<Self> {
        let sockets = RwLock::new(HashMap::new());
        let tcp = Arc::new(Self { sockets });
        let cloned_tcp = tcp.clone();
        std::thread::spawn(move || {
            // パケットの受信用スレッド
            cloned_tcp.receive_handler().unwrap();
        });
        tcp
    }

    /// ターゲットに接続し，接続済みソケットのIDを返す
    pub fn connect(&self, addr: Ipv4Addr, port: u16) -> Result<SocketID> {
        let mut rng = rand::thread_rng();
        let mut socket = Socket::new(
            // get_source_addr_to(addr)?,
            "127.0.0.1".parse()?,
            addr,
            55555,
            port,
            TcpStatus::SynSent,
        )?;
        socket.send_param.initial_seq = rng.gen_range(1..1 << 31);
        socket.send_tcp_packet(socket.send_param.initial_seq, 0, TcpFlags::SYN, &[])?;
        socket.send_param.unacked_seq = socket.send_param.initial_seq;
        socket.send_param.next = socket.send_param.initial_seq + 1;
        let mut table = self.sockets.write().unwrap();
        let sock_id = socket.get_sock_id();
        table.insert(sock_id, socket);
        // ロックを外してイベントの待機．受信スレッドがロックを取得できるようにするため．
        drop(table);
        // self.wait_event(sock_id, TCPEventKind::ConnectionCompleted);
        Ok(sock_id)
    }

    fn receive_handler(&self) -> Result<()> {
        Ok(())
    }
}
