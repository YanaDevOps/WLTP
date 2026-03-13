//! Traceroute implementation for Windows and macOS
//! 
//! This module provides ICMP-based traceroute functionality with
//! continuous measurement capabilities similar to MTR/WinMTR.

use crate::types::*;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

#[cfg(windows)]
use windows::Win32::Networking::WinSock::*;
#[cfg(windows)]
use windows::Win32::Foundation::*;

/// Result type for traceroute operations
pub type TraceResult<T> = Result<T, TraceError>;

/// Errors that can occur during tracing
#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("DNS resolution failed: {0}")]
    DnsResolution(String),
    
    #[error("Socket error: {0}")]
    Socket(String),
    
    #[error("Permission denied (requires administrator/root)")]
    PermissionDenied,
    
    #[error("Invalid target: {0}")]
    InvalidTarget(String),
    
    #[error("Trace already running")]
    AlreadyRunning,
    
    #[error("Trace not running")]
    NotRunning,
    
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Resolve a hostname to an IP address
pub fn resolve_target(target: &str) -> TraceResult<IpAddr> {
    // First, try to parse as IP address
    if let Ok(ip) = target.parse::<IpAddr>() {
        return Ok(ip);
    }
    
    // Perform DNS lookup
    let addr: Vec<SocketAddr> = dns_lookup::lookup_host(target)
        .map_err(|e| TraceError::DnsResolution(format!("{}: {}", target, e)))?;
    
    addr.into_iter()
        .next()
        .map(|sa| sa.ip())
        .ok_or_else(|| TraceError::DnsResolution(format!("No addresses found for {}", target)))
}

/// ICMP packet structure
#[derive(Debug, Clone)]
struct IcmpPacket {
    icmp_type: u8,
    code: u8,
    checksum: u16,
    identifier: u16,
    sequence: u16,
    payload: Vec<u8>,
}

impl IcmpPacket {
    const ECHO_REQUEST: u8 = 8;
    const ECHO_REPLY: u8 = 0;
    const TIME_EXCEEDED: u8 = 11;
    const DESTINATION_UNREACHABLE: u8 = 3;
    
    fn new_echo(identifier: u16, sequence: u16) -> Self {
        let payload = (0..56u8).collect::<Vec<_>>(); // Standard 64-byte packet
        Self {
            icmp_type: Self::ECHO_REQUEST,
            code: 0,
            checksum: 0,
            identifier,
            sequence,
            payload,
        }
    }
    
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + self.payload.len());
        bytes.push(self.icmp_type);
        bytes.push(self.code);
        bytes.extend_from_slice(&self.checksum.to_be_bytes());
        bytes.extend_from_slice(&self.identifier.to_be_bytes());
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }
    
    fn calculate_checksum(&mut self) {
        self.checksum = 0;
        let bytes = self.to_bytes();
        self.checksum = compute_checksum(&bytes);
    }
}

fn compute_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    
    !sum as u16
}

/// Parse an ICMP response from raw bytes
fn parse_icmp_response(data: &[u8]) -> Option<(u8, u8, u16, u16)> {
    if data.len() < 8 {
        return None;
    }
    
    // For time exceeded and destination unreachable, the ICMP header is at offset 20 (IP header) + 8 (original ICMP)
    let icmp_type = data[20];
    let code = data[21];
    
    // For echo replies, the identifier and sequence are in the response
    let identifier = u16::from_be_bytes([data[24], data[25]]);
    let sequence = u16::from_be_bytes([data[26], data[27]]);
    
    Some((icmp_type, code, identifier, sequence))
}

/// Active trace session state
pub struct TraceRunner {
    config: TraceConfig,
    target_ip: IpAddr,
    hops: HashMap<u8, HopSample>,
    running: Arc<AtomicBool>,
    session_id: String,
}

impl TraceRunner {
    pub fn new(session: &TraceSession) -> TraceResult<Self> {
        let target_ip = resolve_target(&session.config.target)?;
        
        Ok(Self {
            config: session.config.clone(),
            target_ip,
            hops: HashMap::new(),
            running: Arc::new(AtomicBool::new(false)),
            session_id: session.id.clone(),
        })
    }
    
    /// Get the resolved target IP
    pub fn target_ip(&self) -> IpAddr {
        self.target_ip
    }
    
    /// Check if the trace is currently running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
    
    /// Stop the trace
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
    
    /// Run the trace, sending events to the provided channel
    pub async fn run(&mut self, event_tx: mpsc::Sender<TraceEvent>) -> TraceResult<()> {
        if self.is_running() {
            return Err(TraceError::AlreadyRunning);
        }
        
        self.running.store(true, Ordering::Relaxed);
        
        // Determine if target is IPv4 or IPv6
        let is_ipv6 = matches!(self.target_ip, IpAddr::V6(_));
        
        if is_ipv6 {
            // IPv6 tracing not yet supported
            self.running.store(false, Ordering::Relaxed);
            return Err(TraceError::Internal("IPv6 tracing not yet supported".to_string()));
        }
        
        // Run platform-specific trace
        #[cfg(windows)]
        let result = self.run_windows(event_tx).await;
        
        #[cfg(target_os = "macos")]
        let result = self.run_macos(event_tx).await;
        
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            self.running.store(false, Ordering::Relaxed);
            return Err(TraceError::Internal("Platform not supported".to_string()));
        }
        
        self.running.store(false, Ordering::Relaxed);
        result
    }
    
    #[cfg(windows)]
    async fn run_windows(&mut self, event_tx: mpsc::Sender<TraceEvent>) -> TraceResult<()> {
        use std::mem;
        
        // Initialize Winsock
        unsafe {
            let mut wsa_data: WSADATA = mem::zeroed();
            let result = WSAStartup(0x0202, &mut wsa_data);
            if result != 0 {
                return Err(TraceError::Socket("Failed to initialize Winsock".to_string()));
            }
        }
        
        // Create raw socket
        let socket = unsafe { socket(AF_INET as i32, SOCK_RAW as i32, IPPROTO_ICMP as i32) };
        if socket == INVALID_SOCKET {
            let err = unsafe { WSAGetLastError() };
            error!("Failed to create raw socket, error: {}", err);
            if err == 10013 {
                return Err(TraceError::PermissionDenied);
            }
            return Err(TraceError::Socket(format!("Failed to create socket: {}", err)));
        }
        
        // Set timeout
        let timeout_ms = self.config.timeout_ms as i32;
        unsafe {
            let result = setsockopt(
                socket,
                SOL_SOCKET,
                SO_RCVTIMEO,
                Some(&timeout_ms as *const i32 as *const _),
                mem::size_of::<i32>() as i32,
            );
            if result == SOCKET_ERROR {
                let err = WSAGetLastError();
                closesocket(socket);
                return Err(TraceError::Socket(format!("Failed to set timeout: {}", err)));
            }
        }
        
        let target_addr = SOCKADDR_IN {
            sin_family: AF_INET as u16,
            sin_port: 0,
            sin_addr: IN_ADDR {
                S_un: match self.target_ip {
                    IpAddr::V4(ip) => unsafe { mem::transmute(ip.octets()) },
                    _ => return Err(TraceError::InvalidTarget("Expected IPv4 address".to_string())),
                },
            },
            sin_zero: [0; 8],
        };
        
        let mut sequence: u16 = 0;
        let identifier = (std::process::id() & 0xFFFF) as u16;
        
        // Discover hops first
        info!("Starting trace to {}", self.target_ip);
        
        for ttl in 1..=self.config.max_hops {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }
            
            // Set TTL
            let ttl_val = ttl as i32;
            unsafe {
                let result = setsockopt(
                    socket,
                    IPPROTO_IP as i32,
                    IP_TTL,
                    Some(&ttl_val as *const i32 as *const _),
                    mem::size_of::<i32>() as i32,
                );
                if result == SOCKET_ERROR {
                    warn!("Failed to set TTL: {}", WSAGetLastError());
                    continue;
                }
            }
            
            // Create and send ICMP packet
            let mut packet = IcmpPacket::new_echo(identifier, sequence);
            packet.calculate_checksum();
            let packet_bytes = packet.to_bytes();
            
            let send_result = unsafe {
                sendto(
                    socket,
                    packet_bytes.as_ptr() as *const _,
                    packet_bytes.len() as i32,
                    0,
                    Some(&target_addr as *const _ as *const _),
                    mem::size_of::<SOCKADDR_IN>() as i32,
                )
            };
            
            if send_result == SOCKET_ERROR {
                warn!("Send failed: {}", unsafe { WSAGetLastError() });
                sequence = sequence.wrapping_add(1);
                continue;
            }
            
            // Receive response
            let mut recv_buf = [0u8; 1024];
            let mut from_addr: SOCKADDR_IN = unsafe { mem::zeroed() };
            let mut from_len = mem::size_of::<SOCKADDR_IN>() as i32;
            
            let start_time = Instant::now();
            
            let recv_result = unsafe {
                recvfrom(
                    socket,
                    recv_buf.as_mut_ptr() as *mut _,
                    recv_buf.len() as i32,
                    0,
                    Some(&mut from_addr as *mut _ as *mut _),
                    &mut from_len as *mut _,
                )
            };
            
            let latency = start_time.elapsed().as_secs_f64() * 1000.0;
            sequence = sequence.wrapping_add(1);
            
            if recv_result == SOCKET_ERROR {
                let err = unsafe { WSAGetLastError() };
                if err == 10060 { // WSAETIMEDOUT
                    // Timeout - record it
                    self.record_timeout(ttl);
                    debug!("Hop {} timed out", ttl);
                } else {
                    warn!("Receive error: {}", err);
                }
            } else {
                // Parse response
                let recv_len = recv_result as usize;
                if let Some((icmp_type, code, resp_id, _)) = parse_icmp_response(&recv_buf[..recv_len]) {
                    if resp_id == identifier {
                        let from_ip = Ipv4Addr::from(unsafe { from_addr.sin_addr.S_un.S_addr.to_be() });
                        
                        match icmp_type {
                            IcmpPacket::ECHO_REPLY => {
                                // Reached destination
                                self.record_response(ttl, IpAddr::V4(from_ip), latency, true);
                                debug!("Reached destination at hop {}, latency: {:.2}ms", ttl, latency);
                                
                                // Emit completion
                                if let Err(e) = event_tx.send(TraceEvent::HopDiscovered {
                                    session_id: self.session_id.clone(),
                                    hop: self.hops.get(&ttl).cloned().unwrap(),
                                }).await {
                                    error!("Failed to send event: {}", e);
                                }
                                break;
                            }
                            IcmpPacket::TIME_EXCEEDED => {
                                // Intermediate hop
                                self.record_response(ttl, IpAddr::V4(from_ip), latency, false);
                                debug!("Hop {} response from {}, latency: {:.2}ms", ttl, from_ip, latency);
                            }
                            IcmpPacket::DESTINATION_UNREACHABLE => {
                                // Destination unreachable
                                self.record_response(ttl, IpAddr::V4(from_ip), latency, false);
                                warn!("Destination unreachable at hop {}, code: {}", ttl, code);
                                break;
                            }
                            _ => {
                                debug!("Unknown ICMP type: {}, code: {}", icmp_type, code);
                            }
                        }
                    }
                }
            }
            
            // Emit hop discovered event
            if let Some(hop) = self.hops.get(&ttl).cloned() {
                if let Err(e) = event_tx.send(TraceEvent::HopDiscovered {
                    session_id: self.session_id.clone(),
                    hop,
                }).await {
                    error!("Failed to send event: {}", e);
                }
            }
            
            // Small delay between probes
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        // Now run continuous measurement
        self.run_continuous_windows(socket, identifier, event_tx).await?;
        
        // Cleanup
        unsafe {
            closesocket(socket);
            WSACleanup();
        }
        
        Ok(())
    }
    
    #[cfg(windows)]
    async fn run_continuous_windows(
        &mut self,
        socket: windows::Win32::Networking::WinSock::SOCKET,
        identifier: u16,
        event_tx: mpsc::Sender<TraceEvent>,
    ) -> TraceResult<()> {
        use std::mem;
        use windows::Win32::Networking::WinSock::*;
        
        let max_ttl = self.hops.keys().copied().max().unwrap_or(self.config.max_hops);
        let mut sequence: u16 = 0;
        let mut packet_count: u32 = 0;
        
        while self.running.load(Ordering::Relaxed) {
            for ttl in 1..=max_ttl {
                if !self.running.load(Ordering::Relaxed) {
                    break;
                }
                
                // Set TTL
                let ttl_val = ttl as i32;
                unsafe {
                    setsockopt(
                        socket,
                        IPPROTO_IP as i32,
                        IP_TTL,
                        Some(&ttl_val as *const i32 as *const _),
                        mem::size_of::<i32>() as i32,
                    );
                }
                
                // Send probe
                let mut packet = IcmpPacket::new_echo(identifier, sequence);
                packet.calculate_checksum();
                let packet_bytes = packet.to_bytes();
                
                let target_addr = SOCKADDR_IN {
                    sin_family: AF_INET as u16,
                    sin_port: 0,
                    sin_addr: IN_ADDR {
                        S_un: match self.target_ip {
                            IpAddr::V4(ip) => unsafe { mem::transmute(ip.octets()) },
                            _ => continue,
                        },
                    },
                    sin_zero: [0; 8],
                };
                
                unsafe {
                    sendto(
                        socket,
                        packet_bytes.as_ptr() as *const _,
                        packet_bytes.len() as i32,
                        0,
                        Some(&target_addr as *const _ as *const _),
                        mem::size_of::<SOCKADDR_IN>() as i32,
                    );
                }
                
                // Receive response
                let mut recv_buf = [0u8; 1024];
                let mut from_addr: SOCKADDR_IN = unsafe { mem::zeroed() };
                let mut from_len = mem::size_of::<SOCKADDR_IN>() as i32;
                
                let start_time = Instant::now();
                
                let recv_result = unsafe {
                    recvfrom(
                        socket,
                        recv_buf.as_mut_ptr() as *mut _,
                        recv_buf.len() as i32,
                        0,
                        Some(&mut from_addr as *mut _ as *mut _),
                        &mut from_len as *mut _,
                    )
                };
                
                let latency = start_time.elapsed().as_secs_f64() * 1000.0;
                sequence = sequence.wrapping_add(1);
                
                if recv_result == SOCKET_ERROR {
                    let err = unsafe { WSAGetLastError() };
                    if err == 10060 {
                        self.record_timeout(ttl);
                        if let Err(e) = event_tx.send(TraceEvent::HopTimeout {
                            session_id: self.session_id.clone(),
                            hop_index: ttl,
                        }).await {
                            error!("Failed to send event: {}", e);
                        }
                    }
                } else if let Some((icmp_type, _, _, _)) = parse_icmp_response(&recv_buf[..recv_result as usize]) {
                    let from_ip = Ipv4Addr::from(unsafe { from_addr.sin_addr.S_un.S_addr.to_be() });
                    
                    if icmp_type == IcmpPacket::ECHO_REPLY || icmp_type == IcmpPacket::TIME_EXCEEDED {
                        self.record_response(ttl, IpAddr::V4(from_ip), latency, icmp_type == IcmpPacket::ECHO_REPLY);
                        
                        if let Some(hop) = self.hops.get(&ttl) {
                            if let Err(e) = event_tx.send(TraceEvent::HopStatsUpdate {
                                session_id: self.session_id.clone(),
                                hop_index: ttl,
                                stats: hop.stats.clone(),
                            }).await {
                                error!("Failed to send event: {}", e);
                            }
                        }
                    }
                }
            }
            
            packet_count += 1;
            if self.config.count > 0 && packet_count >= self.config.count {
                break;
            }
            
            tokio::time::sleep(Duration::from_millis(self.config.interval_ms)).await;
        }
        
        Ok(())
    }
    
    #[cfg(target_os = "macos")]
    async fn run_macos(&mut self, event_tx: mpsc::Sender<TraceEvent>) -> TraceResult<()> {
        // macOS implementation using raw sockets
        // Requires root privileges for raw socket access
        
        use libc::*;
        use std::mem;
        
        let socket = unsafe { socket(AF_INET, SOCK_RAW, IPPROTO_ICMP) };
        if socket < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(1) {
                return Err(TraceError::PermissionDenied);
            }
            return Err(TraceError::Socket(format!("Failed to create socket: {}", err)));
        }
        
        // Set receive timeout
        let timeout = libc::timeval {
            tv_sec: (self.config.timeout_ms / 1000) as i64,
            tv_usec: ((self.config.timeout_ms % 1000) * 1000) as i64,
        };
        unsafe {
            setsockopt(
                socket,
                SOL_SOCKET,
                SO_RCVTIMEO,
                &timeout as *const _ as *const _,
                mem::size_of::<libc::timeval>() as u32,
            );
        }
        
        let target_addr = libc::sockaddr_in {
            sin_family: AF_INET as u8,
            sin_port: 0,
            sin_addr: libc::in_addr {
                s_addr: match self.target_ip {
                    IpAddr::V4(ip) => u32::from_ne_bytes(ip.octets()),
                    _ => return Err(TraceError::InvalidTarget("Expected IPv4 address".to_string())),
                },
            },
            sin_zero: [0; 8],
        };
        
        let mut sequence: u16 = 0;
        let identifier = (std::process::id() & 0xFFFF) as u16;
        
        // Discovery phase
        for ttl in 1..=self.config.max_hops {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }
            
            let ttl_val = ttl as i32;
            unsafe {
                setsockopt(
                    socket,
                    IPPROTO_IP,
                    IP_TTL,
                    &ttl_val as *const _ as *const _,
                    mem::size_of::<i32>() as u32,
                );
            }
            
            let mut packet = IcmpPacket::new_echo(identifier, sequence);
            packet.calculate_checksum();
            let packet_bytes = packet.to_bytes();
            
            let send_result = unsafe {
                libc::sendto(
                    socket,
                    packet_bytes.as_ptr() as *const _,
                    packet_bytes.len(),
                    0,
                    &target_addr as *const _ as *const _,
                    mem::size_of::<libc::sockaddr_in>() as u32,
                )
            };
            
            if send_result < 0 {
                sequence = sequence.wrapping_add(1);
                continue;
            }
            
            let mut recv_buf = [0u8; 1024];
            let mut from_addr: libc::sockaddr_in = unsafe { mem::zeroed() };
            let mut from_len = mem::size_of::<libc::sockaddr_in>() as u32;
            
            let start_time = Instant::now();
            
            let recv_result = unsafe {
                libc::recvfrom(
                    socket,
                    recv_buf.as_mut_ptr() as *mut _,
                    recv_buf.len(),
                    0,
                    &mut from_addr as *mut _ as *mut _,
                    &mut from_len as *mut _,
                )
            };
            
            let latency = start_time.elapsed().as_secs_f64() * 1000.0;
            sequence = sequence.wrapping_add(1);
            
            if recv_result < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EAGAIN) || err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                    self.record_timeout(ttl);
                }
            } else if let Some((icmp_type, code, _, _)) = parse_icmp_response(&recv_buf[..recv_result as usize]) {
                let from_ip = Ipv4Addr::from(u32::from_be(from_addr.sin_addr.s_addr));
                
                match icmp_type {
                    IcmpPacket::ECHO_REPLY => {
                        self.record_response(ttl, IpAddr::V4(from_ip), latency, true);
                        break;
                    }
                    IcmpPacket::TIME_EXCEEDED => {
                        self.record_response(ttl, IpAddr::V4(from_ip), latency, false);
                    }
                    IcmpPacket::DESTINATION_UNREACHABLE => {
                        self.record_response(ttl, IpAddr::V4(from_ip), latency, false);
                        break;
                    }
                    _ => {}
                }
            }
            
            if let Some(hop) = self.hops.get(&ttl).cloned() {
                if let Err(e) = event_tx.send(TraceEvent::HopDiscovered {
                    session_id: self.session_id.clone(),
                    hop,
                }).await {
                    error!("Failed to send event: {}", e);
                }
            }
            
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        // Continuous measurement phase
        let max_ttl = self.hops.keys().copied().max().unwrap_or(self.config.max_hops);
        let mut packet_count: u32 = 0;
        
        while self.running.load(Ordering::Relaxed) {
            for ttl in 1..=max_ttl {
                if !self.running.load(Ordering::Relaxed) {
                    break;
                }
                
                let ttl_val = ttl as i32;
                unsafe {
                    setsockopt(socket, IPPROTO_IP, IP_TTL, &ttl_val as *const _ as *const _, mem::size_of::<i32>() as u32);
                }
                
                let mut packet = IcmpPacket::new_echo(identifier, sequence);
                packet.calculate_checksum();
                let packet_bytes = packet.to_bytes();
                
                unsafe {
                    libc::sendto(
                        socket,
                        packet_bytes.as_ptr() as *const _,
                        packet_bytes.len(),
                        0,
                        &target_addr as *const _ as *const _,
                        mem::size_of::<libc::sockaddr_in>() as u32,
                    );
                }
                
                let mut recv_buf = [0u8; 1024];
                let mut from_addr: libc::sockaddr_in = unsafe { mem::zeroed() };
                let mut from_len = mem::size_of::<libc::sockaddr_in>() as u32;
                
                let start_time = Instant::now();
                
                let recv_result = unsafe {
                    libc::recvfrom(
                        socket,
                        recv_buf.as_mut_ptr() as *mut _,
                        recv_buf.len(),
                        0,
                        &mut from_addr as *mut _ as *mut _,
                        &mut from_len as *mut _,
                    )
                };
                
                let latency = start_time.elapsed().as_secs_f64() * 1000.0;
                sequence = sequence.wrapping_add(1);
                
                if recv_result < 0 {
                    self.record_timeout(ttl);
                    if let Err(e) = event_tx.send(TraceEvent::HopTimeout {
                        session_id: self.session_id.clone(),
                        hop_index: ttl,
                    }).await {
                        error!("Failed to send event: {}", e);
                    }
                } else if let Some((icmp_type, _, _, _)) = parse_icmp_response(&recv_buf[..recv_result as usize]) {
                    let from_ip = Ipv4Addr::from(u32::from_be(from_addr.sin_addr.s_addr));
                    
                    if icmp_type == IcmpPacket::ECHO_REPLY || icmp_type == IcmpPacket::TIME_EXCEEDED {
                        self.record_response(ttl, IpAddr::V4(from_ip), latency, icmp_type == IcmpPacket::ECHO_REPLY);
                        
                        if let Some(hop) = self.hops.get(&ttl) {
                            if let Err(e) = event_tx.send(TraceEvent::HopStatsUpdate {
                                session_id: self.session_id.clone(),
                                hop_index: ttl,
                                stats: hop.stats.clone(),
                            }).await {
                                error!("Failed to send event: {}", e);
                            }
                        }
                    }
                }
            }
            
            packet_count += 1;
            if self.config.count > 0 && packet_count >= self.config.count {
                break;
            }
            
            tokio::time::sleep(Duration::from_millis(self.config.interval_ms)).await;
        }
        
        unsafe {
            close(socket);
        }
        
        Ok(())
    }
    
    fn record_response(&mut self, hop_index: u8, ip: IpAddr, latency_ms: f64, is_destination: bool) {
        let hop = self.hops.entry(hop_index).or_insert_with(|| HopSample::new(hop_index));
        hop.ip = Some(ip);
        hop.stats.add_sample(latency_ms);
        
        // Try reverse DNS lookup (async would be better, but this is simpler)
        if hop.hostname.is_none() {
            if let Ok(hostname) = dns_lookup::lookup_addr(&std::net::SocketAddr::new(ip, 0)) {
                hop.hostname = Some(hostname);
            }
        }
        
        // Update status based on stats
        hop.status = self.determine_hop_status(hop, is_destination);
    }
    
    fn record_timeout(&mut self, hop_index: u8) {
        let hop = self.hops.entry(hop_index).or_insert_with(|| HopSample::new(hop_index));
        hop.stats.add_timeout();
    }
    
    fn determine_hop_status(&self, hop: &HopSample, is_destination: bool) -> Severity {
        let stats = &hop.stats;
        
        if stats.received == 0 {
            return if is_destination { Severity::Critical } else { Severity::Unknown };
        }
        
        // High loss is only critical at destination
        if stats.loss_percent > 20.0 {
            return if is_destination { Severity::Critical } else { Severity::Warning };
        }
        
        // High latency
        if let Some(avg) = stats.avg_ms {
            if avg > 200.0 {
                return Severity::Warning;
            }
            if avg > 500.0 {
                return Severity::Critical;
            }
        }
        
        // High jitter
        if let Some(jitter) = stats.jitter_ms {
            if jitter > 50.0 {
                return Severity::Warning;
            }
        }
        
        Severity::Ok
    }
    
    /// Get current hop data
    pub fn get_hops(&self) -> Vec<HopSample> {
        let mut hops: Vec<_> = self.hops.values().cloned().collect();
        hops.sort_by_key(|h| h.index);
        hops
    }
}
