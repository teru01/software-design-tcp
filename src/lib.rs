use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use pnet::packet::{
    ip::IpNextHeaderProtocols,
    tcp::{self, MutableTcpPacket, TcpFlags, TcpPacket},
    Packet,
};
use pnet::transport::{self, TransportChannelType, TransportProtocol, TransportSender};
use pnet::util;
use rand::Rng;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::{cmp, thread};

const MSS: usize = 1460;
const TCP_HEADER_SIZE: usize = 20;
static MY_IP_ADDR: Lazy<Ipv4Addr> = Lazy::new(|| "192.168.100.1".parse().unwrap());

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
        // 送信元のポート番号
        tcp_packet.set_source(self.local_port);
        // 宛先のポート番号
        tcp_packet.set_destination(self.remote_port);
        // シーケンス番号。単位はオクテット
        tcp_packet.set_sequence(seq);
        // 確認応答番号。単位はオクテット
        tcp_packet.set_acknowledgement(ack);
        // セグメント中のペイロード領域のオフセット。単位は4オクテット
        tcp_packet.set_data_offset(5);
        // 制御フラグ
        tcp_packet.set_flags(flag.into());
        // ウィンドウサイズ。単位はオクテット。今回は適当な値を入れている
        tcp_packet.set_window(5000);
        // ペイロード
        tcp_packet.set_payload(payload);
        // チェックサム。UDPと同様に擬似ヘッダを利用する。
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
            *MY_IP_ADDR,
            addr,
            rng.gen_range(50000..60000), // 送信元のポート。適当に選ぶ
            port,
            TcpStatus::SynSent,
        )?;
        socket.send_param.initial_seq = rng.gen_range(1..1 << 31);
        let sock_id = socket.get_sock_id();
        let mut table = self.sockets.write().unwrap();
        table.insert(sock_id, socket);
        drop(table);
        loop {
            // コネクションが確立するまでSYNを送り続ける。
            // 本来なら最大送信回数を定めて、そこで打ち切るべき
            let mut table = self.sockets.write().unwrap();
            let mut socket = table.get_mut(&sock_id).context("no such socket")?;
            if socket.status == TcpStatus::Established {
                break;
            }
            socket.send_tcp_packet(socket.send_param.initial_seq, 0, TcpFlags::SYN, &[])?;
            socket.send_param.unacked_seq = socket.send_param.initial_seq;
            socket.send_param.next = socket.send_param.initial_seq + 1;
            drop(table);
            thread::sleep(Duration::from_secs(1));
        }
        Ok(sock_id)
    }

    /// バッファのデータを送信する．必要であれば複数のパケットに分割して送信する．
    pub fn send(&self, sock_id: SocketID, buffer: &[u8]) -> Result<()> {
        let mut cursor = 0;
        while cursor < buffer.len() {
            let send_size = cmp::min(MSS, buffer.len() - cursor);
            let mut table = self.sockets.write().unwrap();
            let mut socket = table.get_mut(&sock_id).unwrap();
            let mut sent = false;
            let current_seq = socket.send_param.next;
            while !(sent && socket.send_param.unacked_seq == socket.send_param.next) {
                // 送信したものがackされていない時、再送
                socket.send_tcp_packet(
                    current_seq,
                    socket.recv_param.next,
                    TcpFlags::ACK,
                    &buffer[cursor..cursor + send_size],
                )?;
                sent = true;
                socket.send_param.next = current_seq + send_size as u32;
                // 少しの間ロックを外して待機し，受信スレッドがACKを受信できるようにしている．
                drop(table);
                thread::sleep(Duration::from_millis(100));
                table = self.sockets.write().unwrap();
                socket = table.get_mut(&sock_id).unwrap();
            }
            cursor += send_size;
        }
        Ok(())
    }

    /// 受信スレッド用のハンドラ
    fn receive_handler(&self) -> Result<()> {
        dbg!("begin recv thread");
        let (_, mut receiver) = transport::transport_channel(
            65535,
            TransportChannelType::Layer3(IpNextHeaderProtocols::Tcp), // IPアドレスが必要なので，IPパケットレベルで取得．
        )
        .unwrap();
        let mut packet_iter = transport::ipv4_packet_iter(&mut receiver);
        loop {
            let (packet, remote_addr) = match packet_iter.next() {
                Ok((p, r)) => (p, r),
                Err(_) => continue,
            };
            let local_addr = packet.get_destination();
            // pnetのTcpPacketを生成
            let tcp_packet = match TcpPacket::new(packet.payload()) {
                Some(p) => p,
                None => {
                    continue;
                }
            };
            let remote_addr = match remote_addr {
                IpAddr::V4(addr) => addr,
                _ => {
                    continue;
                }
            };
            let mut table = self.sockets.write().unwrap();
            let socket = match table.get_mut(&SocketID(
                local_addr,
                remote_addr,
                tcp_packet.get_destination(),
                tcp_packet.get_source(),
            )) {
                Some(socket) => socket,
                None => continue,
            };
            if !is_correct_checksum(&tcp_packet, packet.get_source(), packet.get_destination()) {
                dbg!("invalid checksum");
                continue;
            }
            if let Err(error) = match socket.status {
                TcpStatus::SynSent => self.synsent_handler(socket, &tcp_packet),
                TcpStatus::Established => self.established_handler(socket, &tcp_packet),
                _ => {
                    dbg!("not implemented state");
                    Ok(())
                }
            } {
                dbg!(error);
            }
        }
    }

    /// SYNSENT状態のソケットに到着したパケットの処理
    fn synsent_handler(&self, socket: &mut Socket, packet: &TcpPacket) -> Result<()> {
        dbg!("synsent handler");
        if packet.get_flags() & TcpFlags::ACK > 0
            && socket.send_param.unacked_seq <= packet.get_acknowledgement()
            && packet.get_acknowledgement() <= socket.send_param.next
            && packet.get_flags() & TcpFlags::SYN > 0
        {
            socket.recv_param.next = packet.get_sequence() + 1;
            socket.recv_param.initial_seq = packet.get_sequence();
            socket.send_param.unacked_seq = packet.get_acknowledgement();
            if socket.send_param.unacked_seq > socket.send_param.initial_seq {
                socket.status = TcpStatus::Established;
                socket.send_tcp_packet(
                    socket.send_param.next,
                    socket.recv_param.next,
                    TcpFlags::ACK,
                    &[],
                )?;
                dbg!("status: synsent ->", &socket.status);
            }
        }
        Ok(())
    }

    /// ESTABLISHED状態のソケットに到着したパケットの処理
    fn established_handler(&self, socket: &mut Socket, packet: &TcpPacket) -> Result<()> {
        dbg!("established handler");
        if packet.get_flags() & TcpFlags::ACK > 0
            && socket.send_param.unacked_seq < packet.get_acknowledgement()
            && packet.get_acknowledgement() <= socket.send_param.next
        {
            dbg!("ACK recved");
            socket.send_param.unacked_seq = packet.get_acknowledgement();
        }
        Ok(())
    }
}

fn is_correct_checksum(tcp_packet: &TcpPacket, src_ip: Ipv4Addr, dest_ip: Ipv4Addr) -> bool {
    return tcp_packet.get_checksum() == tcp::ipv4_checksum(tcp_packet, &src_ip, &dest_ip);
}
