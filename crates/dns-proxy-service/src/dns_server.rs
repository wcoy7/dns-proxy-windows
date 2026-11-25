//! UDP DNS proxy: blocklisted names get NXDOMAIN/sinkhole; everything else
//! is forwarded to the configured upstream resolver(s).

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use dns_proxy_core::{AppConfig, BlockEngine, BlockMode};
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::{RData, Record, RecordType};
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use parking_lot::RwLock;
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

const MAX_UDP: usize = 4096;
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(3);

pub struct RuntimeState {
    pub config: RwLock<AppConfig>,
    pub blocks: RwLock<BlockEngine>,
}

impl RuntimeState {
    pub fn from_config(config: AppConfig) -> Arc<Self> {
        let blocks = BlockEngine::from_config(&config);
        Arc::new(Self {
            config: RwLock::new(config),
            blocks: RwLock::new(blocks),
        })
    }

    #[allow(dead_code)]
    pub fn reload(&self, config: AppConfig) {
        let blocks = BlockEngine::from_config(&config);
        *self.blocks.write() = blocks;
        *self.config.write() = config;
    }
}

pub async fn run_dns_proxy(state: Arc<RuntimeState>) -> Result<()> {
    let listen = {
        let cfg = state.config.read();
        cfg.listen_addr()?
    };

    let sock = UdpSocket::bind(listen)
        .await
        .with_context(|| {
            format!("failed to bind UDP {listen} (need Administrator for port 53?)")
        })?;
    info!("DNS proxy listening on {listen}");
    run_dns_proxy_on_socket(state, Arc::new(sock)).await
}

/// Serve on an already-bound socket (used by the Windows service after StartPending → bind).
pub async fn run_dns_proxy_on_socket(state: Arc<RuntimeState>, sock: Arc<UdpSocket>) -> Result<()> {
    let mut buf = vec![0u8; MAX_UDP];
    loop {
        let (len, peer) = sock.recv_from(&mut buf).await?;
        let packet = buf[..len].to_vec();
        let state = Arc::clone(&state);
        let sock = Arc::clone(&sock);
        tokio::spawn(async move {
            if let Err(err) = handle_query(&state, &sock, peer, &packet).await {
                debug!(%peer, error = %err, "query handling failed");
            }
        });
    }
}

async fn handle_query(
    state: &RuntimeState,
    sock: &UdpSocket,
    peer: SocketAddr,
    packet: &[u8],
) -> Result<()> {
    let request = Message::from_bytes(packet).context("invalid DNS message")?;
    if request.message_type() != MessageType::Query || request.op_code() != OpCode::Query {
        return Ok(());
    }

    let qname = request
        .queries()
        .first()
        .map(|q| q.name().to_string())
        .unwrap_or_default();
    let qtype = request
        .queries()
        .first()
        .map(|q| q.query_type())
        .unwrap_or(RecordType::A);

    let (blocked, mode, sinkhole, upstreams, verbose) = {
        let cfg = state.config.read();
        let blocked = state.blocks.read().is_blocked(&qname);
        (
            blocked,
            cfg.block.mode,
            cfg.block.sinkhole_ip.clone(),
            cfg.upstream.sockets()?,
            cfg.verbose_logging,
        )
    };

    if blocked {
        if verbose {
            info!(%peer, %qname, ?qtype, "blocked");
        } else {
            debug!(%peer, %qname, ?qtype, "blocked");
        }
        let response = match mode {
            BlockMode::NxDomain => build_nxdomain(&request),
            BlockMode::Sinkhole => build_sinkhole(&request, &sinkhole)?,
        };
        let bytes = response.to_bytes()?;
        sock.send_to(&bytes, peer).await?;
        return Ok(());
    }

    match forward_upstream(&upstreams, packet).await {
        Ok(response_bytes) => {
            sock.send_to(&response_bytes, peer).await?;
            if verbose {
                debug!(%peer, %qname, ?qtype, "forwarded");
            }
        }
        Err(err) => {
            // Never leave the stub resolver hanging — Windows treats silence as timeout
            // and the whole machine looks "offline" when adapters point at 127.0.0.1.
            warn!(%peer, %qname, ?qtype, error = %err, "upstream forward failed; SERVFAIL");
            let response = build_servfail(&request);
            let bytes = response.to_bytes()?;
            sock.send_to(&bytes, peer).await?;
        }
    }
    Ok(())
}

async fn forward_upstream(upstreams: &[SocketAddr], packet: &[u8]) -> Result<Vec<u8>> {
    let mut last_err = None;
    for upstream in upstreams {
        match forward_one(*upstream, packet).await {
            Ok(bytes) => return Ok(bytes),
            Err(err) => {
                warn!(%upstream, error = %err, "upstream failed");
                last_err = Some(err);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no upstreams available")))
}

async fn forward_one(upstream: SocketAddr, packet: &[u8]) -> Result<Vec<u8>> {
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(upstream).await?;
    sock.send(packet).await?;
    let mut buf = vec![0u8; MAX_UDP];
    let len = tokio::time::timeout(UPSTREAM_TIMEOUT, sock.recv(&mut buf))
        .await
        .context("upstream timed out")??;
    Ok(buf[..len].to_vec())
}

fn build_nxdomain(request: &Message) -> Message {
    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_recursion_desired(request.recursion_desired());
    response.set_recursion_available(true);
    response.set_response_code(ResponseCode::NXDomain);
    for q in request.queries() {
        response.add_query(q.clone());
    }
    response
}

fn build_servfail(request: &Message) -> Message {
    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_recursion_desired(request.recursion_desired());
    response.set_recursion_available(true);
    response.set_response_code(ResponseCode::ServFail);
    for q in request.queries() {
        response.add_query(q.clone());
    }
    response
}

fn build_sinkhole(request: &Message, sinkhole_ip: &str) -> Result<Message> {
    let ip = IpAddr::from_str(sinkhole_ip)
        .with_context(|| format!("invalid sinkhole IP {sinkhole_ip}"))?;
    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_recursion_desired(request.recursion_desired());
    response.set_recursion_available(true);
    response.set_response_code(ResponseCode::NoError);
    response.set_authoritative(true);

    for q in request.queries() {
        response.add_query(q.clone());
        let rdata = match (q.query_type(), ip) {
            (RecordType::A, IpAddr::V4(v4)) => Some(RData::A(v4.into())),
            (RecordType::AAAA, IpAddr::V6(v6)) => Some(RData::AAAA(v6.into())),
            (RecordType::A, IpAddr::V6(_)) | (RecordType::AAAA, IpAddr::V4(_)) => None,
            _ => None,
        };
        if let Some(rdata) = rdata {
            let record = Record::from_rdata(q.name().clone(), 60, rdata);
            response.add_answer(record);
        }
    }
    Ok(response)
}
