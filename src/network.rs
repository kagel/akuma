// Network stack with TCP netcat-like server
// Uses smoltcp for TCP/IP and virtio-net for the network device

use smoltcp::iface::{Config, Interface, SocketSet, SocketStorage};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer as TcpSocketBuffer, State as TcpState};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address};
use spinning_top::Spinlock;
use virtio_drivers::device::net::VirtIONetRaw;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};

use crate::console;
use crate::timer;
use crate::virtio_hal::VirtioHal;

// ============================================================================
// Constants
// ============================================================================

const LISTEN_PORT: u16 = 23; // Telnet port, forwarded from host 2323

// ============================================================================
// Static Buffers (required for smoltcp and virtio)
// ============================================================================

// Virtio DMA buffers
static mut VIRTIO_RX_BUFFER: [u8; 2048] = [0; 2048];
static mut VIRTIO_TX_BUFFER: [u8; 2048] = [0; 2048];

// smoltcp socket buffers
static mut TCP_RX_DATA: [u8; 4096] = [0; 4096];
static mut TCP_TX_DATA: [u8; 4096] = [0; 4096];

// smoltcp socket storage
static mut SOCKET_STORAGE: [SocketStorage<'static>; 2] = [SocketStorage::EMPTY; 2];

// ============================================================================
// Global State
// ============================================================================

struct NetStack {
    device: VirtioNetDevice,
    iface: Interface,
    sockets: SocketSet<'static>,
    tcp_handle: smoltcp::iface::SocketHandle,
    rx_pending: bool,
    rx_token: u16,
}

static NET_STACK: Spinlock<Option<NetStack>> = Spinlock::new(None);
static NET_INITIALIZED: Spinlock<bool> = Spinlock::new(false);

// ============================================================================
// Virtio Network Device
// ============================================================================

pub struct VirtioNetDevice {
    inner: VirtIONetRaw<VirtioHal, MmioTransport, 16>,
}

impl VirtioNetDevice {
    fn new(inner: VirtIONetRaw<VirtioHal, MmioTransport, 16>) -> Self {
        Self { inner }
    }
}

// ============================================================================
// smoltcp Device Implementation
// ============================================================================

pub struct VirtioRxToken {
    offset: usize,  // Start of actual Ethernet data (after virtio header)
    len: usize,     // Length of Ethernet frame (not including virtio header)
}

pub struct VirtioTxToken<'a> {
    device: &'a mut VirtioNetDevice,
}

impl RxToken for VirtioRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        unsafe {
            // Pass only the Ethernet frame (skip virtio header)
            f(&mut VIRTIO_RX_BUFFER[self.offset..self.offset + self.len])
        }
    }
}

impl<'a> TxToken for VirtioTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        unsafe {
            let result = f(&mut VIRTIO_TX_BUFFER[..len]);
            let _ = self.device.inner.send(&VIRTIO_TX_BUFFER[..len]);
            result
        }
    }
}

// Pending receive state
static mut RX_PENDING_TOKEN: Option<u16> = None;

impl Device for VirtioNetDevice {
    type RxToken<'a> = VirtioRxToken where Self: 'a;
    type TxToken<'a> = VirtioTxToken<'a> where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        unsafe {
            let buf = &mut VIRTIO_RX_BUFFER[..];
            
            // Check if we have a pending receive
            if let Some(token) = RX_PENDING_TOKEN {
                // Check if it's ready
                if self.inner.poll_receive().is_some() {
                    RX_PENDING_TOKEN = None;
                    match self.inner.receive_complete(token, buf) {
                        Ok((hdr_len, data_len)) => {
                            return Some((
                                VirtioRxToken { offset: hdr_len, len: data_len },
                                VirtioTxToken { device: self },
                            ));
                        }
                        Err(_) => {}
                    }
                }
            } else {
                // Start a new receive
                match self.inner.receive_begin(buf) {
                    Ok(token) => {
                        RX_PENDING_TOKEN = Some(token);
                        // Check if immediately ready
                        if self.inner.poll_receive().is_some() {
                            RX_PENDING_TOKEN = None;
                            match self.inner.receive_complete(token, buf) {
                                Ok((hdr_len, data_len)) => {
                                    return Some((
                                        VirtioRxToken { offset: hdr_len, len: data_len },
                                        VirtioTxToken { device: self },
                                    ));
                                }
                                Err(_) => {}
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(VirtioTxToken { device: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = Medium::Ethernet;
        caps
    }
}

// ============================================================================
// Logging Helper
// ============================================================================

fn log(msg: &str) {
    console::print(msg);
}

// ============================================================================
// Initialization
// ============================================================================

/// QEMU virt machine virtio MMIO addresses
const VIRTIO_MMIO_ADDRS: [usize; 8] = [
    0x0a000000, 0x0a000200, 0x0a000400, 0x0a000600, 
    0x0a000800, 0x0a000a00, 0x0a000c00, 0x0a000e00,
];

pub fn init(_dtb_ptr: usize) -> Result<(), &'static str> {
    log("[Net] Initializing network stack...\n");

    // Find virtio-net device
    let mut device: Option<VirtioNetDevice> = None;
    let mut mac = [0u8; 6];

    for (i, &addr) in VIRTIO_MMIO_ADDRS.iter().enumerate() {
        unsafe {
            // Check device ID (1 = network device)
            let device_id = core::ptr::read_volatile((addr + 0x008) as *const u32);
            if device_id != 1 {
                continue;
            }

            log("[Net] Found virtio-net at slot ");
            console::print(&alloc::format!("{}\n", i));

            // Create MMIO transport
            let header_ptr = match core::ptr::NonNull::new(addr as *mut VirtIOHeader) {
                Some(p) => p,
                None => continue,
            };

            let transport = match MmioTransport::new(header_ptr) {
                Ok(t) => t,
                Err(_) => {
                    log("[Net] Failed to create transport\n");
                    continue;
                }
            };

            // Initialize VirtIO network device
            let net = match VirtIONetRaw::<VirtioHal, MmioTransport, 16>::new(transport) {
                Ok(n) => n,
                Err(_) => {
                    log("[Net] Failed to init virtio device\n");
                    continue;
                }
            };

            mac = net.mac_address();
            device = Some(VirtioNetDevice::new(net));
            break;
        }
    }

    let mut device = device.ok_or("No virtio-net device found")?;

    // Log MAC address
    log("[Net] MAC: ");
    for (i, b) in mac.iter().enumerate() {
        if i > 0 {
            console::print(":");
        }
        console::print(&alloc::format!("{:02x}", b));
    }
    log("\n");

    // Create smoltcp interface
    let hw_addr = EthernetAddress::from_bytes(&mac);
    let config = Config::new(HardwareAddress::Ethernet(hw_addr));
    
    let mut iface = Interface::new(config, &mut device, get_time());
    
    // Configure IP address (10.0.2.15 is the default for QEMU user-mode networking)
    iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24)).ok();
    });
    
    // Set default gateway
    iface.routes_mut().add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2)).ok();

    log("[Net] IP: 10.0.2.15/24, Gateway: 10.0.2.2\n");

    // Create TCP socket with static buffers
    let tcp_socket = unsafe {
        let tcp_rx_buffer = TcpSocketBuffer::new(&mut TCP_RX_DATA[..]);
        let tcp_tx_buffer = TcpSocketBuffer::new(&mut TCP_TX_DATA[..]);
        TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer)
    };

    // Create socket set with static storage
    let mut sockets = unsafe {
        SocketSet::new(&mut SOCKET_STORAGE[..])
    };
    let tcp_handle = sockets.add(tcp_socket);

    // Start listening
    {
        let socket = sockets.get_mut::<TcpSocket>(tcp_handle);
        socket.listen(LISTEN_PORT).map_err(|_| "Failed to listen")?;
    }

    log(&alloc::format!("[Net] Listening on port {}\n", LISTEN_PORT));
    log("[Net] Connect from host: nc localhost 2323\n");

    // Store in global state
    {
        let mut stack = NET_STACK.lock();
        *stack = Some(NetStack {
            device,
            iface,
            sockets,
            tcp_handle,
            rx_pending: false,
            rx_token: 0,
        });
    }
    
    *NET_INITIALIZED.lock() = true;

    log("[Net] Network stack ready\n");
    Ok(())
}

// ============================================================================
// Time Helper
// ============================================================================

fn get_time() -> Instant {
    let us = timer::uptime_us();
    Instant::from_micros(us as i64)
}

// ============================================================================
// Network Polling & TCP Handler
// ============================================================================

static mut BYTES_RX: u64 = 0;
static mut BYTES_TX: u64 = 0;
static mut CONNECTIONS: u64 = 0;
static mut WAS_CONNECTED: bool = false;

/// Poll the network stack - call this regularly from a thread
/// Returns true if there was activity
pub fn poll() -> bool {
    let mut stack_guard = NET_STACK.lock();
    let stack = match stack_guard.as_mut() {
        Some(s) => s,
        _ => return false,
    };

    let timestamp = get_time();
    
    // Process network interface
    let activity = stack.iface.poll(timestamp, &mut stack.device, &mut stack.sockets);

    // Handle TCP socket
    let socket = stack.sockets.get_mut::<TcpSocket>(stack.tcp_handle);
    
    // Check for new connection
    if socket.state() == TcpState::Established {
        unsafe {
            if !WAS_CONNECTED {
                WAS_CONNECTED = true;
                CONNECTIONS += 1;
                // Release lock before printing
                drop(stack_guard);
                log("\n[Net] *** Client connected! ***\n");
                log("[Net] Type something and press Enter (echo server)\n");
                log("[Net] Type 'quit' to disconnect\n\n");
                return true;
            }
        }
    } else if socket.state() == TcpState::Listen || socket.state() == TcpState::Closed {
        unsafe {
            WAS_CONNECTED = false;
        }
    }

    // Echo any received data back
    if socket.can_recv() {
        let mut buf = [0u8; 512];
        match socket.recv_slice(&mut buf) {
            Ok(len) if len > 0 => {
                unsafe { BYTES_RX += len as u64; }
                
                // Check for 'quit' command
                let data = &buf[..len];
                if len >= 4 && (data.starts_with(b"quit") || data.starts_with(b"exit")) {
                    let _ = socket.send_slice(b"Goodbye!\r\n");
                    socket.close();
                    drop(stack_guard);
                    log("[Net] Client disconnected (quit)\n");
                    return true;
                }

                // Echo back with prefix
                if socket.can_send() {
                    let _ = socket.send_slice(b"echo: ");
                    let _ = socket.send_slice(&buf[..len]);
                    unsafe { BYTES_TX += (6 + len) as u64; }
                }
                return true;
            }
            _ => {}
        }
    }

    // Re-listen if socket closed
    if socket.state() == TcpState::Closed {
        let _ = socket.listen(LISTEN_PORT);
    }

    activity
}

/// Thread entry point for network server
#[unsafe(no_mangle)]
pub extern "C" fn netcat_server_entry() -> ! {
    log("[Net] Netcat server thread started\n");
    
    loop {
        poll();
        // Yield to other threads
        crate::threading::yield_now();
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Run the network handler in a loop (call from a dedicated thread)
pub fn run_netcat_server() {
    log("[Net] Starting netcat echo server...\n");
    
    loop {
        poll();
        crate::threading::yield_now();
    }
}

/// Print network statistics
pub fn stats() {
    unsafe {
        let connections = core::ptr::read_volatile(core::ptr::addr_of!(CONNECTIONS));
        let bytes_rx = core::ptr::read_volatile(core::ptr::addr_of!(BYTES_RX));
        let bytes_tx = core::ptr::read_volatile(core::ptr::addr_of!(BYTES_TX));
        log(&alloc::format!(
            "[Net] Stats: {} connections, {} bytes RX, {} bytes TX\n",
            connections, bytes_rx, bytes_tx
        ));
    }
}

/// Check if network is initialized
pub fn is_ready() -> bool {
    *NET_INITIALIZED.lock()
}
